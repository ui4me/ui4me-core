//! Native preferences are observed by a dedicated worker. The GUI reads a
//! coalesced snapshot; it never calls D-Bus, the registry, or an OS settings API.
//! Missing settings stay unknown, rather than becoming an invented observation.
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PreferenceSnapshot {
    pub dark: Option<bool>,
    pub reduced_motion: Option<bool>,
    pub high_contrast: Option<bool>,
    pub available: bool,
    pub source: &'static str,
    pub revision: u64,
}

impl PreferenceSnapshot {
    fn empty(source: &'static str) -> Self {
        Self {
            dark: None,
            reduced_motion: None,
            high_contrast: None,
            available: false,
            source,
            revision: 0,
        }
    }
    pub fn availability(&self) -> Option<bool> {
        // A worker that has not observed the service yet cannot report that it
        // is unavailable. Once a first read succeeds or fails this is known.
        (self.source != "pending").then_some(self.available)
    }
    pub fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "color_scheme": self.dark.map(|dark| if dark { "dark" } else { "light" }),
            "reduced_motion": self.reduced_motion,
            "high_contrast": self.high_contrast,
            "available": self.availability(),
            "source": self.source,
            "revision": self.revision,
            "scope": "observed native preferences; document application overrides take precedence"
        })
    }
}

struct Shared {
    value: Mutex<PreferenceSnapshot>,
    stopped: AtomicBool,
    #[cfg(target_os = "windows")]
    thread_id: Mutex<Option<u32>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn publish(&self, mut value: PreferenceSnapshot) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let mut previous = self.value.lock().unwrap_or_else(|e| e.into_inner());
        value.revision = previous.revision;
        if *previous == value {
            return;
        }
        value.revision = previous.revision.saturating_add(1);
        *previous = value;
        drop(previous);
        (self.wake)();
    }
}

pub struct PreferenceWatch {
    shared: Arc<Shared>,
    #[cfg(target_os = "linux")]
    stop: Option<async_channel::Sender<()>>,
}

impl PreferenceWatch {
    pub fn new(native: bool, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared = Arc::new(Shared {
            value: Mutex::new(PreferenceSnapshot::empty(if native {
                "pending"
            } else {
                "headless"
            })),
            stopped: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            thread_id: Mutex::new(None),
            wake,
        });
        #[cfg(target_os = "linux")]
        let stop = if native {
            let (tx, rx) = async_channel::bounded(1);
            let state = shared.clone();
            match std::thread::Builder::new()
                .name("ui4me-preferences".into())
                .spawn(move || {
                    futures_lite::future::block_on(linux::watch(state, rx));
                }) {
                Ok(_) => Some(tx),
                Err(error) => {
                    tracing::warn!(%error, "Cannot start native preference observer");
                    shared.publish(PreferenceSnapshot::empty("unavailable"));
                    None
                }
            }
        } else {
            None
        };
        #[cfg(target_os = "windows")]
        if native {
            let state = shared.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("ui4me-preferences".into())
                .spawn(move || windows::watch(state))
            {
                tracing::warn!(%error, "Cannot start native preference observer");
                shared.publish(PreferenceSnapshot::empty("unavailable"));
            }
        }
        Self {
            shared,
            #[cfg(target_os = "linux")]
            stop,
        }
    }
    pub fn snapshot(&self) -> PreferenceSnapshot {
        self.shared
            .value
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for PreferenceWatch {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        if let Some(stop) = &self.stop {
            stop.close();
        }
        #[cfg(target_os = "windows")]
        {
            let thread = self
                .shared
                .thread_id
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(id) = *thread {
                // The worker creates its message queue before publishing its id.
                // Drop before that point is covered by its stopped flag check.
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                        id,
                        windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                        0,
                        0,
                    );
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use futures_lite::{future, StreamExt};
    use std::{collections::HashMap, time::Duration};
    use zbus::zvariant::OwnedValue;
    const NS: &str = "org.freedesktop.appearance";

    fn assign(snapshot: &mut PreferenceSnapshot, key: &str, value: Option<u32>) {
        // Only 0 and 1 are standardized for contrast and reduced-motion.
        // Preserve unrecognized observations as unknown; the effective
        // document preference still falls back to its no-preference default.
        let boolean_preference = value.and_then(|v| match v {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        });
        match key {
            "color-scheme" => {
                snapshot.dark = value.and_then(|v| match v {
                    1 => Some(true),
                    2 => Some(false),
                    _ => None,
                })
            }
            "reduced-motion" => snapshot.reduced_motion = boolean_preference,
            "contrast" => snapshot.high_contrast = boolean_preference,
            _ => {}
        }
    }

    async fn connected(shared: &Shared, stop: &async_channel::Receiver<()>) -> anyhow::Result<()> {
        let connection = zbus::connection::Builder::session()?
            .method_timeout(Duration::from_secs(3))
            .build()
            .await?;
        let settings = zbus::Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await?;
        // Subscribe before the initial read, so changes during setup cannot vanish.
        let mut changes = settings.receive_signal("SettingChanged").await?;
        let mut owners = settings.receive_owner_changed().await?;
        let mut snapshot = read_all(&settings).await?;
        shared.publish(snapshot.clone());
        loop {
            enum Update {
                Setting(Option<zbus::Message>),
                Owner(bool),
                Stop,
            }
            let update = future::race(
                future::race(async { Update::Setting(changes.next().await) }, async {
                    Update::Owner(owners.next().await.flatten().is_some())
                }),
                async {
                    let _ = stop.recv().await;
                    Update::Stop
                },
            )
            .await;
            match update {
                Update::Stop | Update::Setting(None) | Update::Owner(false) => return Ok(()),
                Update::Owner(true) => {
                    // A restarted portal need not emit every setting again.
                    snapshot = read_all(&settings).await?;
                    shared.publish(snapshot.clone());
                }
                Update::Setting(Some(message)) => {
                    let (namespace, key, value): (String, String, OwnedValue) =
                        message.body().deserialize()?;
                    if namespace == NS {
                        assign(&mut snapshot, &key, u32::try_from(&value).ok());
                        shared.publish(snapshot.clone());
                    }
                }
            }
        }
    }

    async fn read_all(settings: &zbus::Proxy<'_>) -> anyhow::Result<PreferenceSnapshot> {
        let all: HashMap<String, HashMap<String, OwnedValue>> =
            settings.call("ReadAll", &(vec![NS],)).await?;
        let mut snapshot = PreferenceSnapshot::empty("xdg-desktop-portal");
        snapshot.available = true;
        if let Some(values) = all.get(NS) {
            for key in ["color-scheme", "reduced-motion", "contrast"] {
                assign(
                    &mut snapshot,
                    key,
                    values.get(key).and_then(|v| u32::try_from(v).ok()),
                );
            }
        }
        Ok(snapshot)
    }

    pub(super) async fn watch(shared: Arc<Shared>, stop: async_channel::Receiver<()>) {
        loop {
            if shared.stopped.load(Ordering::Acquire) {
                return;
            }
            // A stalled bus handshake is also cancellable; no GUI-thread join is required.
            let result = future::race(connected(&shared, &stop), async {
                let _ = stop.recv().await;
                Ok(())
            })
            .await;
            if shared.stopped.load(Ordering::Acquire) {
                return;
            }
            if let Err(error) = result {
                tracing::debug!(%error, "Native preference service unavailable");
            }
            shared.publish(PreferenceSnapshot::empty("unavailable"));
            // Reconnect after portal/session-bus restart. Never poll a healthy portal.
            future::race(
                async {
                    async_io::Timer::after(Duration::from_secs(5)).await;
                },
                async {
                    let _ = stop.recv().await;
                },
            )
            .await;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn portal_preferences_distinguish_missing_from_no_preference() {
            let mut s = PreferenceSnapshot::empty("test");
            assign(&mut s, "reduced-motion", None);
            assert_eq!(s.reduced_motion, None);
            assign(&mut s, "reduced-motion", Some(0));
            assert_eq!(s.reduced_motion, Some(false));
            assign(&mut s, "reduced-motion", Some(1));
            assert_eq!(s.reduced_motion, Some(true));
            assign(&mut s, "reduced-motion", Some(99));
            assert_eq!(s.reduced_motion, None);
            assign(&mut s, "reduced-motion", Some(2));
            assert_eq!(s.reduced_motion, None);
            assign(&mut s, "color-scheme", Some(1));
            assert_eq!(s.dark, Some(true));
            assign(&mut s, "color-scheme", Some(2));
            assert_eq!(s.dark, Some(false));
            assign(&mut s, "color-scheme", Some(0));
            assert_eq!(s.dark, None);
            assign(&mut s, "contrast", Some(1));
            assert_eq!(s.high_contrast, Some(true));
            assign(&mut s, "contrast", Some(0));
            assert_eq!(s.high_contrast, Some(false));
            assign(&mut s, "contrast", Some(2));
            assert_eq!(s.high_contrast, None);
            assign(&mut s, "contrast", Some(99));
            assert_eq!(s.high_contrast, None);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::{
        mem::size_of,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::*,
        },
    };
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn read() -> PreferenceSnapshot {
        let mut value = PreferenceSnapshot::empty("win32");
        let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let name = wide("AppsUseLightTheme");
        let mut light = 0u32;
        let mut bytes = size_of::<u32>() as u32;
        if unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                (&mut light as *mut u32).cast(),
                &mut bytes,
            )
        } == 0
        {
            value.dark = match light {
                0 => Some(true),
                1 => Some(false),
                _ => None,
            };
        }
        let mut animation = 0i32;
        if unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut animation as *mut i32).cast(),
                0,
            )
        } != 0
        {
            value.reduced_motion = Some(animation == 0);
        }
        let mut contrast: HIGHCONTRASTW = unsafe { std::mem::zeroed() };
        contrast.cbSize = size_of::<HIGHCONTRASTW>() as u32;
        if unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&mut contrast as *mut HIGHCONTRASTW).cast(),
                0,
            )
        } != 0
        {
            value.high_contrast = Some(contrast.dwFlags & HCF_HIGHCONTRASTON != 0);
        }
        value.available =
            value.dark.is_some() || value.reduced_motion.is_some() || value.high_contrast.is_some();
        value
    }

    unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if message == WM_NCCREATE {
            let create = unsafe { &*(l as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
        }
        if matches!(
            message,
            WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE
        ) {
            let shared = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
            if !shared.is_null() {
                unsafe { &*shared }.publish(read());
            }
        }
        if message == WM_NCDESTROY {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
        }
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }

    pub(super) fn watch(shared: Arc<Shared>) {
        struct Registration<'a>(&'a Shared);
        impl Drop for Registration<'_> {
            fn drop(&mut self) {
                *self.0.thread_id.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            // Force the message queue to exist before exposing its thread id to Drop.
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            *shared.thread_id.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(GetCurrentThreadId());
            // Clearing and posting share one mutex, so a recycled thread id
            // cannot receive this observer's late WM_QUIT on any exit path.
            let _registration = Registration(&shared);
            if shared.stopped.load(Ordering::Acquire) {
                return;
            }
            let instance = GetModuleHandleW(null());
            let name = wide(&format!("UI4ME.PreferenceObserver.{}", std::process::id()));
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: name.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassW(&class) == 0 {
                shared.publish(PreferenceSnapshot::empty("unavailable"));
                return;
            }
            // A hidden top-level window receives broadcast preference changes.
            // HWND_MESSAGE would not receive WM_SETTINGCHANGE broadcasts.
            let window = CreateWindowExW(
                0,
                name.as_ptr(),
                name.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                Arc::as_ptr(&shared).cast(),
            );
            if window.is_null() {
                shared.publish(PreferenceSnapshot::empty("unavailable"));
                UnregisterClassW(name.as_ptr(), instance);
                return;
            }
            shared.publish(read());
            while !shared.stopped.load(Ordering::Acquire)
                && GetMessageW(&mut message, null_mut(), 0, 0) > 0
            {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            DestroyWindow(window);
            UnregisterClassW(name.as_ptr(), instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_availability_is_unknown_until_an_observation() {
        let pending = PreferenceSnapshot::empty("pending");
        assert_eq!(pending.availability(), None);
        assert!(pending.value()["available"].is_null());
        let unavailable = PreferenceSnapshot::empty("unavailable");
        assert_eq!(unavailable.availability(), Some(false));
        assert_eq!(unavailable.value()["available"], false);
        let mut observed = PreferenceSnapshot::empty("test");
        observed.available = true;
        assert_eq!(observed.availability(), Some(true));
        assert_eq!(observed.value()["available"], true);
    }
    #[test]
    fn headless_preferences_remain_unknown() {
        let watch = PreferenceWatch::new(false, Arc::new(|| {}));
        let s = watch.snapshot();
        assert!(!s.available);
        assert_eq!(s.dark, None);
        assert_eq!(s.reduced_motion, None);
        assert_eq!(s.source, "headless");
        assert_eq!(s.availability(), Some(false));
    }
    #[test]
    fn unchanged_observation_does_not_wake_or_advance_revision() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let count = calls.clone();
        let watch = PreferenceWatch::new(
            false,
            Arc::new(move || {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let mut value = PreferenceSnapshot::empty("test");
        value.dark = Some(true);
        watch.shared.publish(value.clone());
        watch.shared.publish(value);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(watch.snapshot().revision, 1);
    }
}
