use super::*;
use serde::Deserialize;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::monitor::Fullscreen;
use winit::window::{UserAttentionType, WindowAttributes};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct WindowLimits {
    min: Option<(u32, u32)>,
    max: Option<(u32, u32)>,
}

impl WindowLimits {
    fn validate(self) -> Result<Self, RpcError> {
        for size in [self.min, self.max].into_iter().flatten() {
            if !(1..=8192).contains(&size.0) || !(1..=8192).contains(&size.1) {
                return Err(RpcError::invalid_params(
                    "Window constraints must be 1..8192 CSS pixels",
                ));
            }
        }
        if let (Some(min), Some(max)) = (self.min, self.max) {
            if min.0 > max.0 || min.1 > max.1 {
                return Err(RpcError::invalid_params(
                    "Minimum size must not exceed maximum size",
                ));
            }
        }
        Ok(self)
    }
    fn clamp(self, width: u32, height: u32) -> (u32, u32) {
        let min = self.min.unwrap_or((1, 1));
        let max = self.max.unwrap_or((8192, 8192));
        (width.clamp(min.0, max.0), height.clamp(min.1, max.1))
    }
    fn value(self) -> Value {
        json!({"min_width":self.min.map(|s|s.0),"min_height":self.min.map(|s|s.1),
            "max_width":self.max.map(|s|s.0),"max_height":self.max.map(|s|s.1)})
    }
}

/// Missing means unchanged, explicit null means remove a constraint.
fn patch_value<'de, D>(deserializer: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer).map(Some)
}
fn paired_size(width: Option<u32>, height: Option<u32>) -> Result<Option<(u32, u32)>, RpcError> {
    match (width, height) {
        (None, None) => Ok(None),
        (Some(w), Some(h)) => Ok(Some((w, h))),
        _ => Err(RpcError::invalid_params(
            "Size constraints require both width and height",
        )),
    }
}
fn patched_size(
    previous: Option<(u32, u32)>,
    width: Option<Option<u32>>,
    height: Option<Option<u32>>,
) -> Result<Option<(u32, u32)>, RpcError> {
    match (width, height) {
        (None, None) => Ok(previous),
        (Some(w), Some(h)) => paired_size(w, h),
        _ => Err(RpcError::invalid_params(
            "Update or clear both dimensions of a size constraint",
        )),
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RpcError> {
    serde_json::from_value(value).map_err(|e| RpcError::invalid_params(e.to_string()))
}
fn title_ok(title: &str) -> Result<(), RpcError> {
    if title.len() > 4096 || title.contains('\0') {
        Err(RpcError::invalid_params(
            "Title is too long or contains NUL",
        ))
    } else {
        Ok(())
    }
}
fn size_ok(width: u32, height: u32) -> Result<(), RpcError> {
    if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
        Err(RpcError::invalid_params(
            "Logical window size must be 1..8192 by 1..8192 CSS pixels",
        ))
    } else {
        Ok(())
    }
}
fn yes() -> bool {
    true
}
fn default_width() -> u32 {
    900
}
fn default_height() -> u32 {
    560
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateWindow {
    #[serde(default)]
    title: String,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "yes")]
    visible: bool,
    #[serde(default = "yes")]
    resizable: bool,
    #[serde(default = "yes")]
    decorations: bool,
    min_width: Option<u32>,
    min_height: Option<u32>,
    max_width: Option<u32>,
    max_height: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowKey {
    window_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetWindow {
    window_id: String,
    title: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    visible: Option<bool>,
    resizable: Option<bool>,
    decorations: Option<bool>,
    minimized: Option<bool>,
    maximized: Option<bool>,
    fullscreen: Option<bool>,
    focus: Option<bool>,
    x: Option<i32>,
    y: Option<i32>,
    #[serde(default, deserialize_with = "patch_value")]
    min_width: Option<Option<u32>>,
    #[serde(default, deserialize_with = "patch_value")]
    min_height: Option<Option<u32>>,
    #[serde(default, deserialize_with = "patch_value")]
    max_width: Option<Option<u32>>,
    #[serde(default, deserialize_with = "patch_value")]
    max_height: Option<Option<u32>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attention {
    window_id: String,
    kind: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetText {
    window_id: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    target: String,
    window_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealFile {
    path: PathBuf,
    window_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DialogParams {
    window_id: String,
    title: Option<String>,
    directory: Option<PathBuf>,
    file_name: Option<String>,
    #[serde(default)]
    filters: Vec<DialogFilter>,
    #[serde(default = "yes")]
    multiple: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DialogFilter {
    name: String,
    extensions: Vec<String>,
}

impl Host {
    pub(super) fn dispatch(
        &mut self,
        method: &str,
        params: Value,
        reply: Option<Reply>,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) -> Result<Option<Value>, RpcError> {
        if matches!(
            self.lifecycle.phase(),
            Phase::Quitting | Phase::Failed | Phase::Stopped
        ) && !matches!(
            method,
            "host.capabilities" | "host.diagnostics" | "request.cancel" | "app.force_quit"
        ) {
            return Err(RpcError::invalid_state(
                "Application is shutting down or failed",
            ));
        }
        let value = match method {
            "bitmap.create" | "bitmap.present" | "bitmap.close" => {
                self.dispatch_bitmap(method, params, event_loop)?
            }
            "document.load" | "document.get" | "document.unload" | "document.command" => {
                let window_id = params
                    .get("window_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let result = self.dispatch_document(method, params);
                if let Some(id) = window_id {
                    self.prune_document_work(&id);
                    self.document_changed(&id, event_loop);
                }
                result?
            }
            "host.capabilities" => self.capabilities(),
            "host.diagnostics" => {
                json!({"phase":self.lifecycle.phase(),"windows":self.windows.keys().collect::<Vec<_>>(),
                "pending_jobs":self.jobs.len(),"pending_transfers":self.transfers.len(),"pid":std::process::id(),
                "backend_pid":self.backend.as_ref().map(Backend::id),"containment":platform::containment_description(),
                "logging":crate::logging::diagnostics()})
            }
            "app.ready" => {
                if self.windows.is_empty() {
                    return Err(RpcError::invalid_state(
                        "v0.1 requires at least one application window",
                    ));
                }
                self.lifecycle
                    .ready()
                    .map_err(|e| RpcError::invalid_state(e.to_string()))?;
                json!({"ready":true})
            }
            "app.request_quit" => {
                self.request_quit("backend_request")?;
                json!({"requested":true})
            }
            "app.force_quit" => {
                self.force_shutdown();
                json!({"requested":true})
            }
            "window.create" => {
                if self.lifecycle.phase() == Phase::QuitPending {
                    return Err(RpcError::busy(
                        "Window creation is frozen during application quit preflight",
                    ));
                }
                self.create_window(params, event_loop)?
            }
            "window.set" => self.set_window(params)?,
            "window.request_attention" => self.request_attention(params)?,
            "window.get" => {
                let key: WindowKey = decode(params)?;
                self.describe_window(&key.window_id)?
            }
            "window.list" => Value::Array(
                self.windows
                    .keys()
                    .map(|id| self.describe_window(id).expect("existing window"))
                    .collect(),
            ),
            "window.request_close" => {
                let key: WindowKey = decode(params)?;
                self.request_close(&key.window_id, "backend_request")?;
                json!({"requested":true})
            }
            "probe.set_text" => {
                let request: SetText = decode(params)?;
                let window = self
                    .windows
                    .get_mut(&request.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                window
                    .probe
                    .set_text(&request.text)
                    .map_err(RpcError::invalid_params)?;
                self.refresh(&request.window_id);
                json!({"updated":true})
            }
            "probe.get_text" => {
                let key: WindowKey = decode(params)?;
                let window = self
                    .windows
                    .get(&key.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                json!({"text":window.probe.text,"cursor":window.probe.cursor,"anchor":window.probe.anchor,
                    "preedit":window.probe.preedit,"focused":window.probe.focused})
            }
            "desktop.paths" => platform::standard_paths(&self.config.app.id)
                .map_err(|e| RpcError::unavailable(e.to_string()))?,
            "desktop.preferences" => self.preferences.snapshot().value(),
            "desktop.open_external" => {
                if self.headless {
                    return Err(RpcError::unsupported(
                        "External open requires a desktop session",
                    ));
                }
                let target: Target = decode(params)?;
                validate_external_target(&target.target)?;
                self.start_external(
                    reply,
                    target.window_id,
                    platform::external::Operation::Open(target.target),
                )?;
                return Ok(None);
            }
            "desktop.reveal_file" => {
                let request: RevealFile = decode(params)?;
                let text = request
                    .path
                    .to_str()
                    .ok_or_else(|| RpcError::invalid_params("Reveal requires a Unicode path"))?;
                if !request.path.is_absolute()
                    || text.is_empty()
                    || text.len() > 32768
                    || text.chars().any(char::is_control)
                {
                    return Err(RpcError::invalid_params(
                        "Reveal requires a bounded absolute local path",
                    ));
                }
                self.start_external(
                    reply,
                    request.window_id,
                    platform::external::Operation::Reveal(request.path),
                )?;
                return Ok(None);
            }
            "clipboard.write_text" => {
                let text: Text = decode(params)?;
                if text.text.len() > 1024 * 1024 {
                    return Err(RpcError::invalid_params("Clipboard text exceeds 1 MiB"));
                }
                self.write_clipboard(event_loop, text.text)?;
                json!({"written":true})
            }
            "clipboard.read_text" => {
                let reply = reply.ok_or_else(|| {
                    RpcError::invalid_request("Clipboard read requires a request ID")
                })?;
                self.read_clipboard(event_loop, TransferTarget::Rpc(reply))?;
                return Ok(None);
            }
            "dialog.open_files" | "dialog.save_file" | "dialog.choose_folder" => {
                self.start_dialog(method, params, reply)?;
                return Ok(None);
            }
            "request.cancel" => {
                let cancel: Cancel = decode(params)?;
                let connection = self.client.unwrap_or(0);
                let target = (connection, cancel.id);
                let mut found = false;
                for job in self.jobs.values_mut() {
                    if job.reply.as_ref() == Some(&target) {
                        job.cancelled = true;
                        job.reply = None;
                        found = true;
                        if let Some(dialog) = &job.dialog {
                            dialog.cancel();
                        }
                    }
                }
                let serial =
                    self.transfers
                        .iter()
                        .find_map(|(id, pending)| match &pending.target {
                            TransferTarget::Rpc(reply) if reply == &target => Some(*id),
                            _ => None,
                        });
                if let Some(serial) = serial {
                    self.transfers.remove(&serial);
                    found = true;
                }
                if found {
                    self.respond(&target, Err(RpcError::cancelled("Request cancelled")));
                }
                json!({"cancelled":found})
            }
            _ => return Err(RpcError::method_not_found(method)),
        };
        Ok(Some(value))
    }

    pub(super) fn create_window(
        &mut self,
        params: Value,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) -> Result<Value, RpcError> {
        if self.windows.len() >= self.config.host.max_windows {
            return Err(RpcError::busy("Application window limit reached"));
        }
        let mut request: CreateWindow = decode(params)?;
        if request.title.is_empty() {
            request.title = self.config.app.name.clone();
        }
        title_ok(&request.title)?;
        size_ok(request.width, request.height)?;
        if !self.headless && cfg!(target_os = "linux") && !request.visible {
            return Err(RpcError::unsupported(
                "The pinned Wayland backend cannot create a hidden top-level window",
            ));
        }
        let limits = WindowLimits {
            min: paired_size(request.min_width, request.min_height)?,
            max: paired_size(request.max_width, request.max_height)?,
        }
        .validate()?;
        (request.width, request.height) = limits.clamp(request.width, request.height);
        let id = format!("w{}", self.next_window);
        self.next_window += 1;
        let mut probe = Probe::default();
        probe.status = format!(
            "{} • {} • {}",
            self.config.app.id,
            std::env::consts::OS,
            if self.headless {
                "headless protocol mode"
            } else {
                "native desktop"
            }
        );
        let native = if self.headless {
            None
        } else {
            let event_loop =
                event_loop.ok_or_else(|| RpcError::unavailable("Native surfaces are not ready"))?;
            let mut attributes = WindowAttributes::default()
                .with_title(request.title.clone())
                .with_window_icon(Some(self.icon.clone()))
                .with_surface_size(LogicalSize::new(request.width, request.height))
                .with_resizable(request.resizable)
                .with_decorations(request.decorations)
                .with_visible(false);
            if let Some((w, h)) = limits.min {
                attributes = attributes.with_min_surface_size(LogicalSize::new(w, h));
            }
            if let Some((w, h)) = limits.max {
                attributes = attributes.with_max_surface_size(LogicalSize::new(w, h));
            }
            #[cfg(target_os = "linux")]
            let attributes = attributes.with_platform_attributes(Box::new(
                winit::platform::wayland::WindowAttributesWayland::default()
                    .with_name(&self.config.app.id, &self.config.app.id),
            ));
            let window: Arc<dyn Window> = event_loop
                .create_window(attributes)
                .map_err(|e| RpcError::unavailable(format!("Create native window: {e}")))?
                .into();
            // One presentation context per host. On Wayland each context binds
            // a registry and wl_shm (v1 has no destroy request), so a context per
            // window grew memory with every window created.
            if self.presentation.is_none() {
                self.presentation = Some(
                    softbuffer::Context::new(event_loop.owned_display_handle()).map_err(|e| {
                        RpcError::unavailable(format!("Create presentation context: {e}"))
                    })?,
                );
            }
            let context = self.presentation.as_ref().expect("presentation context");
            let surface = Surface::new(context, window.clone())
                .map_err(|e| RpcError::unavailable(format!("Create presentation surface: {e}")))?;
            let size = window.surface_size();
            let accessibility = NativeAccessibility::new(
                &id,
                window.as_ref(),
                probe.tree(
                    &request.title,
                    size.width,
                    size.height,
                    window.scale_factor(),
                ),
                self.action_tx.clone(),
                self.wake.clone(),
            )
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
            self.native_ids.insert(window.id(), id.clone());
            let mut native = NativeWindow {
                accessibility,
                surface,
                window,
                modifiers: ModifiersState::empty(),
                ime_enabled: false,
                drag_selecting: false,
                presented: false,
                resized_at: None,
                pointer: (0.0, 0.0),
                last_titlebar_press: None,
                document_epoch: None,
                composing: false,
                ime_resets: 0,
                ime_quiet_until: None,
                preedit_cleared: false,
                ime_sent: None,
                incoming_drop: None,
            };
            native::update_ime(&probe, &mut native);
            native.window.set_visible(request.visible);
            native.window.request_redraw();
            Some(native)
        };
        self.windows.insert(
            id.clone(),
            WindowRecord {
                title: request.title,
                width: request.width,
                height: request.height,
                visible: request.visible,
                resizable: request.resizable,
                decorations: request.decorations,
                limits,
                native,
                probe,
                document: None,
                closing: false,
            },
        );
        self.describe_window(&id)
    }

    fn describe_window(&self, id: &str) -> Result<Value, RpcError> {
        let record = self
            .windows
            .get(id)
            .ok_or_else(|| RpcError::closed("Window does not exist"))?;
        let native = record.native.as_ref();
        let observed = json!({
            "focused":native.map(|n|n.window.has_focus()),
            "minimized":native.and_then(|n|n.window.is_minimized()),
            "maximized":native.map(|n|n.window.is_maximized()),
            "fullscreen":native.map(|n|n.window.fullscreen().is_some()),
            "resizable":native.map(|n|n.window.is_resizable()),
            "decorations":native.map(|n|n.window.is_decorated()),
            "visible":native.and_then(|n|n.window.is_visible()),
            "theme":native.and_then(|n|n.window.theme()).map(|t|if t==winit::window::Theme::Dark {"dark"}else{"light"}),
            "position":native.and_then(|n|n.window.outer_position().ok()).map(|p|json!({"x":p.x,"y":p.y,"unit":"physical_pixels"})),
        });
        Ok(
            json!({"window_id":id,"title":record.title,"width":record.width,"height":record.height,
            "visible":record.visible,"native":native.is_some(),"scale_factor":native.map_or(1.0, |n| n.window.scale_factor()),
            "physical_size":native.map(|n| {let s=n.window.surface_size(); json!({"width":s.width,"height":s.height})}),
            "requested":{"visible":record.visible,"resizable":record.resizable,"decorations":record.decorations,"size_constraints":record.limits.value()},
            "observed":observed,"closing":record.closing}),
        )
    }

    pub(super) fn set_window(&mut self, params: Value) -> Result<Value, RpcError> {
        let request: SetWindow = decode(params)?;
        let window = self
            .windows
            .get_mut(&request.window_id)
            .ok_or_else(|| RpcError::closed("Window does not exist"))?;
        if window.closing {
            return Err(RpcError::closed("Window is closing"));
        }
        let width = request.width.unwrap_or(window.width.max(1));
        let height = request.height.unwrap_or(window.height.max(1));
        size_ok(width, height)?;
        let limits = WindowLimits {
            min: patched_size(window.limits.min, request.min_width, request.min_height)?,
            max: patched_size(window.limits.max, request.max_width, request.max_height)?,
        }
        .validate()?;
        let (width, height) = limits.clamp(width, height);
        if let Some(title) = &request.title {
            title_ok(title)?;
        }
        if request.x.is_some() != request.y.is_some() {
            return Err(RpcError::invalid_params("Position requires both x and y"));
        }
        if window.native.is_some() && cfg!(target_os = "linux") {
            if request.visible.is_some() {
                return Err(RpcError::unsupported(
                    "The pinned Wayland backend cannot show or hide an existing top-level window",
                ));
            }
            if request.focus == Some(true) {
                return Err(RpcError::unsupported("Wayland focus is compositor-owned; use request_attention for an advisory request"));
            }
        }
        if !cfg!(target_os = "windows") && request.x.is_some() {
            return Err(RpcError::unsupported(
                "Wayland compositor owns top-level window placement",
            ));
        }
        if !cfg!(target_os = "windows") && request.minimized == Some(false) {
            // xdg-shell has no request to restore a minimized window; winit
            // silently ignores it. Report that instead of fabricating success.
            return Err(RpcError::unsupported(
                "Wayland clients cannot restore a minimized window; the user or compositor restores it",
            ));
        }
        if let Some(native) = &window.native {
            if limits != window.limits {
                // Clear the old opposite bound first: both changes were validated
                // atomically above, and a shrinking max must not cross an old min.
                native.window.set_min_surface_size(None);
                native.window.set_max_surface_size(None);
                native
                    .window
                    .set_min_surface_size(limits.min.map(|(w, h)| LogicalSize::new(w, h).into()));
                native
                    .window
                    .set_max_surface_size(limits.max.map(|(w, h)| LogicalSize::new(w, h).into()));
            }
            if let Some(title) = &request.title {
                native.window.set_title(title);
            }
            if request.width.is_some() || request.height.is_some() || limits != window.limits {
                let _ = native
                    .window
                    .request_surface_size(LogicalSize::new(width, height).into());
            }
            if let Some(visible) = request.visible {
                native.window.set_visible(visible);
            }
            if let Some(resizable) = request.resizable {
                native.window.set_resizable(resizable);
            }
            if let Some(decorations) = request.decorations {
                native.window.set_decorations(decorations);
            }
            if let Some(minimized) = request.minimized {
                native.window.set_minimized(minimized);
            }
            if let Some(maximized) = request.maximized {
                native.window.set_maximized(maximized);
            }
            if let Some(fullscreen) = request.fullscreen {
                native.window.set_fullscreen(if fullscreen {
                    Some(Fullscreen::Borderless(None))
                } else {
                    None
                });
            }
            if request.focus == Some(true) {
                native.window.focus_window();
            }
            if let (Some(x), Some(y)) = (request.x, request.y) {
                native
                    .window
                    .set_outer_position(LogicalPosition::new(x, y).into());
            }
        }
        if let Some(title) = request.title {
            window.title = title;
        }
        window.width = width;
        window.height = height;
        window.limits = limits;
        if let Some(resizable) = request.resizable {
            window.resizable = resizable;
        }
        if let Some(decorations) = request.decorations {
            window.decorations = decorations;
        }
        if let Some(visible) = request.visible {
            window.visible = visible;
        }
        self.refresh(&request.window_id);
        self.describe_window(&request.window_id)
    }

    fn request_attention(&self, params: Value) -> Result<Value, RpcError> {
        let request: Attention = decode(params)?;
        let kind = match request.kind.as_deref() {
            Some("critical") => Some(UserAttentionType::Critical),
            Some("informational") => Some(UserAttentionType::Informational),
            None => None,
            _ => {
                return Err(RpcError::invalid_params(
                    "Attention kind must be critical, informational, or null",
                ))
            }
        };
        let record = self
            .windows
            .get(&request.window_id)
            .ok_or_else(|| RpcError::closed("Window does not exist"))?;
        if record.closing {
            return Err(RpcError::closed("Window is closing"));
        }
        let native = record
            .native
            .as_ref()
            .ok_or_else(|| RpcError::unsupported("Attention requires a native desktop window"))?;
        #[cfg(target_os = "linux")]
        {
            if kind.is_none() {
                return Err(RpcError::unsupported(
                    "Wayland leaves clearing an attention request to the compositor",
                ));
            }
            // The portable attention method returns no result. A token request
            // first verifies that this compositor actually has xdg-activation;
            // its unused completion is harmless and never changes focus.
            let window = native
                .window
                .cast_ref::<winit::platform::wayland::Window>()
                .ok_or_else(|| {
                    RpcError::unsupported("Attention requires the Wayland native backend")
                })?;
            window.request_activation_token().map_err(|e| {
                RpcError::unsupported(format!("Compositor attention unavailable: {e}"))
            })?;
        }
        native.window.request_user_attention(kind);
        Ok(
            json!({"requested":true,"kind":request.kind,"presented":null,
            "policy":"The window manager decides whether and how to present attention"}),
        )
    }

    fn start_external(
        &mut self,
        reply: Option<Reply>,
        parent: Option<String>,
        operation: platform::external::Operation,
    ) -> Result<(), RpcError> {
        if self.headless {
            return Err(RpcError::unsupported(
                "External services require a desktop session",
            ));
        }
        if self.jobs.len() >= 16 || self.jobs.len() + self.transfers.len() >= protocol::MAX_PENDING
        {
            return Err(RpcError::busy(
                "Native service worker or pending request limit reached",
            ));
        }
        let native_parent = parent
            .as_ref()
            .map(|id| {
                let record = self
                    .windows
                    .get(id)
                    .ok_or_else(|| RpcError::closed("External-service parent does not exist"))?;
                if record.closing {
                    return Err(RpcError::closed("External-service parent is closing"));
                }
                record
                    .native
                    .as_ref()
                    .map(|n| n.window.clone())
                    .ok_or_else(|| RpcError::unavailable("Native parent unavailable"))
            })
            .transpose()?;
        let id = self.next_job;
        self.next_job += 1;
        let sender = self.job_tx.clone();
        let wake = self.wake.clone();
        let handle = platform::external::start(
            native_parent,
            operation,
            Box::new(move |result| {
                let result =
                    result.map_err(|e| RpcError::unavailable(format!("External service: {e:#}")));
                let _ = sender.send(JobResult { id, value: result });
                wake();
            }),
        )
        .map_err(|e| RpcError::unavailable(e.to_string()))?;
        self.jobs.insert(
            id,
            PendingJob {
                reply,
                parent,
                kind: JobKind::External,
                cancelled: false,
                dialog: Some(handle),
            },
        );
        Ok(())
    }

    pub(super) fn start_dialog(
        &mut self,
        method: &str,
        params: Value,
        reply: Option<Reply>,
    ) -> Result<(), RpcError> {
        if self.headless {
            return Err(RpcError::unsupported(
                "Native file dialogs are unavailable in headless mode",
            ));
        }
        let request: DialogParams = decode(params)?;
        let record = self
            .windows
            .get(&request.window_id)
            .ok_or_else(|| RpcError::closed("Dialog parent does not exist"))?;
        if record.closing {
            return Err(RpcError::closed("Dialog parent is closing"));
        }
        if self
            .jobs
            .values()
            .any(|j| j.kind.is_dialog() && j.parent.as_ref() == Some(&request.window_id))
        {
            return Err(RpcError::busy("This window already has a native dialog"));
        }
        if let Some(title) = &request.title {
            title_ok(title)?;
        }
        let parent = record
            .native
            .as_ref()
            .ok_or_else(|| RpcError::unavailable("Native parent unavailable"))?
            .window
            .clone();
        if self.jobs.len() >= 16 || self.jobs.len() + self.transfers.len() >= protocol::MAX_PENDING
        {
            return Err(RpcError::busy(
                "Native service worker or pending request limit reached",
            ));
        }
        use platform::dialogs::{DialogFilter, DialogKind, DialogOptions};
        let options = DialogOptions {
            kind: match method {
                "dialog.open_files" => DialogKind::OpenFiles,
                "dialog.save_file" => DialogKind::SaveFile,
                _ => DialogKind::ChooseFolder,
            },
            title: request.title,
            multiple: request.multiple,
            directory: request.directory,
            file_name: request.file_name,
            filters: request
                .filters
                .into_iter()
                .map(|f| DialogFilter {
                    name: f.name,
                    extensions: f.extensions,
                })
                .collect(),
        };
        let id = self.next_job;
        self.next_job += 1;
        let sender = self.job_tx.clone();
        let wake = self.wake.clone();
        let handle = platform::dialogs::start(
            parent,
            options,
            Box::new(move |result| {
                let value = result
                    .map_err(|e| RpcError::unavailable(format!("Native dialog: {e:#}")))
                    .and_then(dialog_paths);
                let _ = sender.send(JobResult { id, value });
                wake();
            }),
        )
        .map_err(|e| RpcError::unavailable(e.to_string()))?;
        self.jobs.insert(
            id,
            PendingJob {
                reply,
                parent: Some(request.window_id),
                kind: JobKind::Dialog,
                cancelled: false,
                dialog: Some(handle),
            },
        );
        Ok(())
    }
}

fn dialog_paths(paths: Option<Vec<PathBuf>>) -> Result<Value, RpcError> {
    let cancelled = paths.is_none();
    let paths = paths.unwrap_or_default();
    if paths.len() > 1024 {
        return Err(RpcError::unavailable("File selection exceeds 1024 entries"));
    }
    let mut total = 0usize;
    let values: Result<Vec<_>, _> = paths
        .into_iter()
        .map(|path| {
            let value = path.into_os_string().into_string().map_err(|_| {
                RpcError::unsupported("Protocol version 1 requires Unicode file paths")
            })?;
            total += value.len();
            if total > 1024 * 1024 {
                return Err(RpcError::unavailable("File selection exceeds 1 MiB"));
            }
            Ok(value)
        })
        .collect();
    Ok(json!({"cancelled":cancelled,"paths":values?}))
}

fn validate_external_target(target: &str) -> Result<(), RpcError> {
    if target.is_empty() || target.len() > 32768 || target.chars().any(char::is_control) {
        return Err(RpcError::invalid_params(
            "External target must be nonempty and bounded",
        ));
    }
    let path = std::path::Path::new(target);
    if path.is_absolute() {
        return Ok(());
    }
    let uri = url::Url::parse(target)
        .map_err(|_| RpcError::invalid_params("Use an absolute path or supported URI"))?;
    if !matches!(uri.scheme(), "https" | "http" | "mailto" | "file") {
        return Err(RpcError::denied(
            "Only http, https, mailto, file and absolute local paths are enabled",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_open_is_data_and_rejects_executable_uri_schemes() {
        assert!(validate_external_target("https://example.com/a?b=c").is_ok());
        assert!(validate_external_target("javascript:alert(1)").is_err());
        assert!(validate_external_target("relative/file").is_err());
        assert!(validate_external_target("https://example.com/\nrun").is_err());
    }
    #[test]
    fn window_input_is_validated_before_native_calls() {
        assert!(size_ok(0, 100).is_err());
        assert!(title_ok("bad\0title").is_err());
        assert!(decode::<CreateWindow>(json!({"width":-1})).is_err());
        assert!(decode::<CreateWindow>(json!({"unsupported":true})).is_err());
    }
    #[test]
    fn constraints_are_atomic_paired_and_explicitly_clearable() {
        let limit = WindowLimits {
            min: Some((300, 200)),
            max: Some((600, 500)),
        }
        .validate()
        .unwrap();
        assert_eq!(limit.clamp(100, 900), (300, 500));
        assert!(WindowLimits {
            min: Some((700, 200)),
            ..limit
        }
        .validate()
        .is_err());
        assert!(paired_size(Some(100), None).is_err());
        let unchanged: SetWindow = decode(json!({"window_id":"w1"})).unwrap();
        assert_eq!(
            patched_size(limit.min, unchanged.min_width, unchanged.min_height).unwrap(),
            limit.min
        );
        let clear: SetWindow =
            decode(json!({"window_id":"w1","min_width":null,"min_height":null})).unwrap();
        assert_eq!(
            patched_size(limit.min, clear.min_width, clear.min_height).unwrap(),
            None
        );
        assert!(patched_size(limit.min, Some(None), None).is_err());
    }
    #[test]
    fn headless_state_does_not_invent_os_observations() {
        let mut host = super::super::tests::controller(true);
        let window = host
            .create_window(
                json!({"width":50,"height":40,"min_width":100,"min_height":80}),
                None,
            )
            .unwrap();
        let id = window["window_id"].as_str().unwrap();
        assert_eq!(window["width"], 100);
        assert_eq!(window["height"], 80);
        assert_eq!(window["requested"]["size_constraints"]["min_width"], 100);
        for key in [
            "focused",
            "minimized",
            "maximized",
            "fullscreen",
            "visible",
            "resizable",
            "position",
        ] {
            assert!(
                window["observed"][key].is_null(),
                "{key} must remain unknown"
            );
        }
        assert!(host.set_window(json!({"window_id":id,"min_width":600,"min_height":400,"max_width":200,"max_height":100})).is_err());
        assert_eq!(
            host.describe_window(id).unwrap()["requested"]["size_constraints"]["min_width"],
            100
        );
        let cleared = host
            .set_window(json!({"window_id":id,"min_width":null,"min_height":null}))
            .unwrap();
        assert!(cleared["requested"]["size_constraints"]["min_width"].is_null());
        assert!(host
            .request_attention(json!({"window_id":id,"kind":"informational"}))
            .is_err());
        assert_eq!(
            host.capabilities()["features"]["file_dialogs"]["available"],
            false
        );
    }
    #[cfg(unix)]
    #[test]
    fn non_unicode_dialog_path_returns_error_instead_of_panicking_worker() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 255]));
        assert!(dialog_paths(Some(vec![path])).is_err());
    }
}
