//! Platform accessibility adapters over a host-owned, synchronously available
//! tree. Accessibility callbacks never call the language backend or wait on IPC.
#[cfg(target_os = "linux")]
use accesskit::DeactivationHandler;
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use std::sync::{mpsc::SyncSender, Arc, Mutex};
use winit::event::WindowEvent;
use winit::window::Window;

pub struct ActionEvent {
    pub window_id: String,
    pub request: ActionRequest,
}

struct Activation(Arc<Mutex<TreeUpdate>>);
impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }
}
struct Actions {
    window_id: String,
    sender: SyncSender<ActionEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // AT callbacks may occur on native worker threads. Never block them on
        // a full queue; overflow is observable and does not corrupt the tree.
        if self
            .sender
            .try_send(ActionEvent {
                window_id: self.window_id.clone(),
                request,
            })
            .is_err()
        {
            tracing::warn!("accessibility action queue full");
        }
        (self.wake)();
    }
}
#[cfg(target_os = "linux")]
struct Deactivation;
#[cfg(target_os = "linux")]
impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

pub struct NativeAccessibility {
    cache: Arc<Mutex<TreeUpdate>>,
    #[cfg(target_os = "linux")]
    adapter: accesskit_unix::Adapter,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
}

impl NativeAccessibility {
    pub fn new(
        window_id: &str,
        window: &dyn Window,
        tree: TreeUpdate,
        sender: SyncSender<ActionEvent>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let cache = Arc::new(Mutex::new(tree));
        let activation = Activation(cache.clone());
        let actions = Actions {
            window_id: window_id.into(),
            sender,
            wake,
        };
        #[cfg(target_os = "linux")]
        let adapter = {
            let _ = window;
            accesskit_unix::Adapter::new(activation, actions, Deactivation)
        };
        #[cfg(target_os = "windows")]
        let adapter = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let hwnd = match window.window_handle()?.as_raw() {
                RawWindowHandle::Win32(handle) => {
                    accesskit_windows::HWND(handle.hwnd.get() as *mut _)
                }
                _ => anyhow::bail!("Windows accessibility requires a Win32 window"),
            };
            accesskit_windows::SubclassingAdapter::new(hwnd, activation, actions)
        };
        Ok(Self { cache, adapter })
    }

    pub fn update(&mut self, tree: TreeUpdate) {
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = tree.clone();
        #[cfg(target_os = "linux")]
        self.adapter.update_if_active(|| tree);
        #[cfg(target_os = "windows")]
        if let Some(events) = self.adapter.update_if_active(|| tree) {
            events.raise();
        }
    }

    pub fn event(&mut self, window: &dyn Window, event: &WindowEvent) {
        #[cfg(target_os = "linux")]
        match event {
            WindowEvent::Focused(focused) => self.adapter.update_window_focus_state(*focused),
            WindowEvent::Moved(_)
            | WindowEvent::SurfaceResized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                let outer = window.outer_position().unwrap_or_default();
                let inner = window.surface_position();
                let outer_size = window.outer_size();
                let inner_size = window.surface_size();
                self.adapter.set_root_window_bounds(
                    accesskit::Rect::from_origin_size(
                        (f64::from(outer.x), f64::from(outer.y)),
                        (f64::from(outer_size.width), f64::from(outer_size.height)),
                    ),
                    accesskit::Rect::from_origin_size(
                        (f64::from(inner.x), f64::from(inner.y)),
                        (f64::from(inner_size.width), f64::from(inner_size.height)),
                    ),
                );
            }
            _ => {}
        }
        #[cfg(target_os = "windows")]
        let _ = (window, event); // SubclassingAdapter receives native focus/bounds events.
    }
}
