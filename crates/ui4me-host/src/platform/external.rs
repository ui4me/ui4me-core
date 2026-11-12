//! External opening uses native brokers, never command-line interpolation.
//! An optional real parent is retained until the service finishes. Linux
//! chooser cancellation closes its portal request before releasing that parent.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use winit::window::Window;

pub enum Operation {
    Open(String),
    Reveal(PathBuf),
}

struct Cancellation {
    cancelled: AtomicBool,
    #[cfg(target_os = "linux")]
    signal: async_channel::Sender<()>,
}
impl Cancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        self.signal.close();
    }
    fn requested(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub fn start(
    parent: Option<Arc<dyn Window>>,
    operation: Operation,
    completion: Box<dyn FnOnce(Result<Value>) + Send>,
) -> Result<super::dialogs::DialogHandle> {
    // Win32 handles are obtainable only on the event-loop thread in winit.
    #[cfg(target_os = "windows")]
    let hwnd = {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match parent.as_ref().map(|p| p.window_handle()).transpose()? {
            Some(handle) => match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => bail!("External opening requires a Win32 parent"),
            },
            None => None,
        }
    };
    #[cfg(target_os = "linux")]
    let (sender, receiver) = async_channel::bounded(1);
    let state = Arc::new(Cancellation {
        cancelled: AtomicBool::new(false),
        #[cfg(target_os = "linux")]
        signal: sender,
    });
    let worker = state.clone();
    std::thread::Builder::new()
        .name("ui4me-external-service".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #[cfg(target_os = "linux")]
                {
                    futures_lite::future::block_on(linux::run(
                        parent.as_ref(),
                        operation,
                        &worker,
                        receiver,
                    ))
                }
                #[cfg(target_os = "windows")]
                {
                    windows::run(hwnd, operation, &worker)
                }
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("External service worker panicked")));
            completion(result);
            drop(parent);
        })?;
    Ok(super::dialogs::DialogHandle::from_cancel(Arc::new(
        move || state.cancel(),
    )))
}

fn cancelled() -> Value {
    json!({"accepted":false,"cancelled":true})
}
fn accepted(parent_forwarded: bool) -> Value {
    json!({"accepted":true,"cancelled":false,"parent_forwarded":parent_forwarded,
        "presented":null})
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::Context;
    use futures_lite::{future, StreamExt};
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
    use std::{
        collections::HashMap, fs::File, os::fd::AsFd, sync::atomic::AtomicU64, time::Duration,
    };
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value as Variant};

    pub(super) async fn run(
        parent: Option<&Arc<dyn Window>>,
        operation: Operation,
        cancellation: &Cancellation,
        stop: async_channel::Receiver<()>,
    ) -> Result<Value> {
        if cancellation.requested() {
            return Ok(cancelled());
        }
        let connection = zbus::connection::Builder::session()?
            .method_timeout(Duration::from_secs(5))
            .build()
            .await?;
        if let Operation::Reveal(path) = operation {
            // FileManager1 has no parent-window argument. Do not claim parenting
            // merely because the API accepted a window_id for lifetime scoping.
            path.symlink_metadata()
                .context("Reveal target does not exist")?;
            let uri = url::Url::from_file_path(path)
                .map_err(|_| anyhow::anyhow!("Invalid local reveal path"))?;
            if cancellation.requested() {
                return Ok(cancelled());
            }
            let _: zbus::Message = connection
                .call_method(
                    Some("org.freedesktop.FileManager1"),
                    "/org/freedesktop/FileManager1",
                    Some("org.freedesktop.FileManager1"),
                    "ShowItems",
                    &(vec![uri.as_str()], ""),
                )
                .await
                .context("File manager does not provide ShowItems")?;
            return Ok(accepted(false));
        }
        let Operation::Open(target) = operation else {
            unreachable!()
        };
        let identifier = if let Some(parent) = parent {
            let raw = parent.window_handle()?.as_raw();
            let display = parent.display_handle()?.as_raw();
            if !matches!(raw, RawWindowHandle::Wayland(_)) {
                bail!("External open parent must be a Wayland window");
            }
            Some(
                future::race(
                    async {
                        ashpd::WindowIdentifier::from_raw_handle(&raw, Some(&display))
                            .await
                            .context("Compositor cannot export the external-open parent")
                    },
                    async {
                        let _ = stop.recv().await;
                        Err(anyhow::anyhow!(
                            "External open cancelled during parent export"
                        ))
                    },
                )
                .await?,
            )
        } else {
            None
        };
        if cancellation.requested() {
            return Ok(cancelled());
        }
        let parent_id = identifier
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let unique = connection
            .unique_name()
            .context("Session bus has no unique name")?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = format!(
            "ui4me_open_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.portal.Desktop")?
            .interface("org.freedesktop.portal.Request")?
            .member("Response")?
            .path_namespace(format!("/org/freedesktop/portal/desktop/request/{unique}"))?
            .build();
        let mut responses = zbus::MessageStream::for_match_rule(rule, &connection, Some(8)).await?;
        let portal = zbus::Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.OpenURI",
        )
        .await?;
        let mut options: HashMap<&str, Variant<'_>> = HashMap::new();
        options.insert("handle_token", Variant::from(token.as_str()));
        let parsed = url::Url::parse(&target);
        // An absolute path wins over a URL parse (a Win32 drive letter would
        // otherwise look like a URI scheme; local native validation is shared).
        let path = if std::path::Path::new(&target).is_absolute() {
            Some(PathBuf::from(&target))
        } else if let Ok(uri) = &parsed {
            if uri.scheme() == "file" {
                Some(
                    uri.to_file_path()
                        .map_err(|_| anyhow::anyhow!("Invalid local file URL"))?,
                )
            } else {
                None
            }
        } else {
            bail!("External target is not a local path or supported URI")
        };
        let request_path: OwnedObjectPath = if let Some(path) = path {
            let file = File::open(&path).context("Cannot open external local target")?;
            let descriptor = zbus::zvariant::Fd::from(file.as_fd());
            // OpenFile opens both files and directories. OpenDirectory means
            // reveal the containing folder, which is a different operation.
            // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html
            portal
                .call("OpenFile", &(parent_id, descriptor, options))
                .await?
        } else {
            portal
                .call("OpenURI", &(parent_id, target, options))
                .await?
        };
        let request = zbus::Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            request_path.clone(),
            "org.freedesktop.portal.Request",
        )
        .await?;
        loop {
            let next = future::race(async { responses.next().await }, async {
                let _ = stop.recv().await;
                None
            })
            .await;
            if cancellation.requested() {
                let result: zbus::Result<()> = request.call("Close", &()).await;
                if let Err(error) = result {
                    if !matches!(&error,zbus::Error::MethodError(name,_,_) if name.as_str()=="org.freedesktop.DBus.Error.UnknownObject")
                    {
                        return Err(error).context("Cancel external chooser");
                    }
                }
                return Ok(cancelled());
            }
            let message = next.context("Desktop portal disconnected before responding")??;
            if message.header().path().map(|p| p.as_str()) != Some(request_path.as_str()) {
                continue;
            }
            let (status, _): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
            return match status {
                0 => Ok(accepted(identifier.is_some())),
                1 => Ok(cancelled()),
                other => Err(anyhow::anyhow!(
                    "Desktop portal rejected external open with response {other}"
                )),
            };
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use ::windows::core::HSTRING;
    use ::windows::Win32::UI::Shell::{ILFree, SHOpenFolderAndSelectItems, SHParseDisplayName};
    use anyhow::Context;
    use windows_sys::Win32::{
        System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    pub(super) fn run(
        parent: Option<isize>,
        operation: Operation,
        cancellation: &Cancellation,
    ) -> Result<Value> {
        if cancellation.requested() {
            return Ok(cancelled());
        }
        let hr = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                COINIT_APARTMENTTHREADED as u32 | COINIT_DISABLE_OLE1DDE as u32,
            )
        };
        if hr < 0 {
            bail!("Cannot initialize external service STA: HRESULT {hr:#x}");
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        match operation {
            Operation::Open(target) => {
                let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
                let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
                let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
                info.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
                info.hwnd = parent.map_or(std::ptr::null_mut(), |h| h as _);
                info.lpVerb = verb.as_ptr();
                info.lpFile = target.as_ptr();
                info.nShow = SW_SHOWNORMAL;
                if cancellation.requested() {
                    return Ok(cancelled());
                }
                if unsafe { ShellExecuteExW(&mut info) } == 0 {
                    return Err(std::io::Error::last_os_error())
                        .context("Windows Shell rejected external open");
                }
                Ok(accepted(parent.is_some()))
            }
            Operation::Reveal(path) => {
                path.symlink_metadata()
                    .context("Reveal target does not exist")?;
                let mut id = std::ptr::null_mut();
                unsafe {
                    SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut id, 0, None)
                }?;
                if id.is_null() {
                    bail!("Windows Shell returned no item identifier");
                }
                // cidl=0 opens the parent and selects this fully qualified item.
                let result = if cancellation.requested() {
                    Ok(())
                } else {
                    unsafe { SHOpenFolderAndSelectItems(id, None, 0) }
                };
                unsafe {
                    ILFree(Some(id));
                }
                result?;
                if cancellation.requested() {
                    Ok(cancelled())
                } else {
                    Ok(accepted(false))
                }
            }
        }
    }
}
