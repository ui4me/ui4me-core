use super::*;
use serde::Deserialize;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::monitor::Fullscreen;
use winit::window::WindowAttributes;

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
    if !(160..=8192).contains(&width) || !(120..=8192).contains(&height) {
        Err(RpcError::invalid_params(
            "Logical window size must be 160..8192 by 120..8192",
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
    minimized: Option<bool>,
    maximized: Option<bool>,
    fullscreen: Option<bool>,
    focus: Option<bool>,
    x: Option<i32>,
    y: Option<i32>,
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
            "desktop.open_external" => {
                if self.headless {
                    return Err(RpcError::unsupported(
                        "External open requires a desktop session",
                    ));
                }
                let target: Target = decode(params)?;
                validate_external_target(&target.target)?;
                self.start_job(reply, None, JobKind::External, move || {
                    platform::open_external(&target.target)
                        .map_err(|e| RpcError::unavailable(e.to_string()))?;
                    Ok(json!({"accepted":true}))
                })?;
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
            let attributes = WindowAttributes::default()
                .with_title(request.title.clone())
                .with_window_icon(Some(self.icon.clone()))
                .with_surface_size(LogicalSize::new(request.width, request.height))
                .with_min_surface_size(LogicalSize::new(480, 320))
                .with_resizable(request.resizable)
                .with_visible(false);
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
                pointer: (0.0, 0.0),
                document_epoch: None,
                composing: false,
                ime_resets: 0,
                ime_quiet_until: None,
                preedit_cleared: false,
                ime_sent: None,
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
        Ok(
            json!({"window_id":id,"title":record.title,"width":record.width,"height":record.height,
            "visible":record.visible,"native":native.is_some(),"scale_factor":native.map_or(1.0, |n| n.window.scale_factor()),
            "physical_size":native.map(|n| {let s=n.window.surface_size(); json!({"width":s.width,"height":s.height})}),
            "closing":record.closing}),
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
        let width = request.width.unwrap_or(window.width);
        let height = request.height.unwrap_or(window.height);
        size_ok(width, height)?;
        if let Some(title) = &request.title {
            title_ok(title)?;
        }
        if request.x.is_some() != request.y.is_some() {
            return Err(RpcError::invalid_params("Position requires both x and y"));
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
            if let Some(title) = &request.title {
                native.window.set_title(title);
            }
            if request.width.is_some() || request.height.is_some() {
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
        if let Some(visible) = request.visible {
            window.visible = visible;
        }
        self.refresh(&request.window_id);
        self.describe_window(&request.window_id)
    }

    fn start_job<F>(
        &mut self,
        reply: Option<Reply>,
        parent: Option<String>,
        kind: JobKind,
        work: F,
    ) -> Result<(), RpcError>
    where
        F: FnOnce() -> Result<Value, RpcError> + Send + 'static,
    {
        if self.jobs.len() >= 16 || self.jobs.len() + self.transfers.len() >= protocol::MAX_PENDING
        {
            return Err(RpcError::busy(
                "Native service worker or pending request limit reached",
            ));
        }
        let id = self.next_job;
        self.next_job += 1;
        let sender = self.job_tx.clone();
        let wake = self.wake.clone();
        std::thread::Builder::new()
            .name("ui4me-native-service".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err(RpcError::internal("Native service worker panicked")));
                let _ = sender.send(JobResult { id, value: result });
                wake();
            })
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        self.jobs.insert(
            id,
            PendingJob {
                reply,
                parent,
                kind,
                cancelled: false,
                dialog: None,
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
    #[cfg(unix)]
    #[test]
    fn non_unicode_dialog_path_returns_error_instead_of_panicking_worker() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 255]));
        assert!(dialog_paths(Some(vec![path])).is_err());
    }
}
