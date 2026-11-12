//! The owner-thread seam between documents and native services. A completion
//! always names its document epoch as well as its window; replacement cannot
//! redirect an outstanding paste, picker, accessibility action or drop.
use super::*;
use ui4me_renderer::{ClipboardRequest, Document, Input};
use winit::{
    cursor::CursorIcon,
    dpi::{LogicalPosition, LogicalSize},
    event::{Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key, PhysicalKey},
    window::{
        ImeCapabilities, ImeEnableRequest, ImeHint, ImePurpose, ImeRequest, ImeRequestData,
        ImeSurroundingText,
    },
};

pub(super) fn update_ime(engine: &Document, native: &mut NativeWindow) {
    let resets = engine.ime_reset_count();
    if resets != native.ime_resets {
        native.ime_resets = resets;
        if native.ime_enabled {
            let _ = native.window.request_ime_update(ImeRequest::Disable);
            native.ime_enabled = false;
            native.composing = false;
            native.preedit_cleared = false;
            native.ime_quiet_until = Some(Instant::now() + Duration::from_millis(250));
        }
    }
    let Some(ime) = engine.ime_request() else {
        if native.ime_enabled {
            let _ = native.window.request_ime_update(ImeRequest::Disable);
            native.ime_enabled = false;
        }
        native.composing = false;
        return;
    };
    let Some(surrounding) = engine.ime_surrounding() else {
        return;
    };
    let (x, y, width, height) = ime.cursor_area;
    let scale = native.window.scale_factor();
    let metadata = engine.ime_hints();
    let mut state = super::ImeState::new(
        &surrounding.text,
        surrounding.cursor,
        surrounding.anchor,
        (x * scale, y * scale, width * scale, height * scale),
        ime.password,
    );
    state.hints = metadata.clone();
    if native.ime_enabled && native.ime_sent.as_ref() == Some(&state) {
        return;
    }
    let Ok(surrounding) =
        ImeSurroundingText::new(surrounding.text, surrounding.cursor, surrounding.anchor)
    else {
        return;
    };
    let (hint, purpose) = ime_policy(&metadata, ime.password);
    let data = ImeRequestData::default()
        .with_hint_and_purpose(hint, purpose)
        .with_cursor_area(
            LogicalPosition::new(x, y).into(),
            LogicalSize::new(width.max(1.0), height.max(1.0)).into(),
        )
        .with_surrounding_text(surrounding);
    let request = if native.ime_enabled {
        ImeRequest::Update(data)
    } else {
        let capabilities = ImeCapabilities::new()
            .with_hint_and_purpose()
            .with_cursor_area()
            .with_surrounding_text();
        let Some(enable) = ImeEnableRequest::new(capabilities, data) else {
            return;
        };
        ImeRequest::Enable(enable)
    };
    match native.window.request_ime_update(request) {
        Ok(()) => {
            native.ime_enabled = true;
            native.ime_sent = Some(state);
        }
        Err(error) => tracing::debug!(%error, "Document IME unavailable"),
    }
}

/// This is advisory metadata. Winit currently delivers purpose/hints on
/// Wayland; its Windows adapter still supplies ordinary IME composition only.
fn ime_policy(metadata: &Value, password: bool) -> (ImeHint, ImePurpose) {
    let mode = metadata
        .get("inputmode")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .or_else(|| metadata.get("input_type").and_then(Value::as_str));
    if password {
        return (
            ImeHint::HIDDEN_TEXT | ImeHint::SENSITIVE_DATA,
            if matches!(mode, Some("numeric" | "decimal")) {
                ImePurpose::Pin
            } else {
                ImePurpose::Password
            },
        );
    }
    let purpose = match mode {
        Some("numeric" | "decimal" | "number") => ImePurpose::Number,
        Some("tel") => ImePurpose::Phone,
        Some("url") => ImePurpose::Url,
        Some("email") => ImePurpose::Email,
        Some("date") => ImePurpose::Date,
        Some("time") => ImePurpose::Time,
        Some("datetime-local") => ImePurpose::DateTime,
        _ => ImePurpose::Normal,
    };
    let mut hint = ImeHint::NONE;
    if metadata.get("multiline").and_then(Value::as_bool) == Some(true) {
        hint |= ImeHint::MULTILINE;
    }
    if metadata.get("spellcheck").and_then(Value::as_bool) == Some(true) {
        hint |= ImeHint::SPELLCHECK;
    }
    if metadata.get("autocomplete").and_then(Value::as_str) == Some("on") {
        hint |= ImeHint::COMPLETION;
    }
    (hint, purpose)
}

fn cursor(name: &str) -> CursorIcon {
    match name {
        "text" => CursorIcon::Text,
        "vertical-text" => CursorIcon::VerticalText,
        "pointer" => CursorIcon::Pointer,
        "crosshair" => CursorIcon::Crosshair,
        "move" => CursorIcon::Move,
        "wait" => CursorIcon::Wait,
        "progress" => CursorIcon::Progress,
        "help" => CursorIcon::Help,
        "not-allowed" => CursorIcon::NotAllowed,
        "no-drop" => CursorIcon::NoDrop,
        "grab" => CursorIcon::Grab,
        "grabbing" => CursorIcon::Grabbing,
        "ew-resize" | "col-resize" => CursorIcon::EwResize,
        "ns-resize" | "row-resize" => CursorIcon::NsResize,
        "nesw-resize" => CursorIcon::NeswResize,
        "nwse-resize" => CursorIcon::NwseResize,
        "zoom-in" => CursorIcon::ZoomIn,
        "zoom-out" => CursorIcon::ZoomOut,
        _ => CursorIcon::Default,
    }
}

impl Host {
    pub(super) fn apply_native_preferences(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        let observation = self.preferences.snapshot();
        if observation.revision == self.preference_revision {
            return;
        }
        self.preference_revision = observation.revision;
        let mut results = Vec::new();
        for (id, window) in &mut self.windows {
            let dark = observation.dark.or_else(|| {
                window
                    .native
                    .as_ref()
                    .and_then(|n| n.window.theme())
                    .map(|t| t == winit::window::Theme::Dark)
            });
            if let Some(document) = &mut window.document {
                let result = document.engine.set_system_preferences(
                    dark,
                    observation.reduced_motion,
                    observation.high_contrast,
                );
                results.push((
                    id.clone(),
                    document.id.clone(),
                    result,
                    document.engine.environment(),
                ));
            }
        }
        for (id, document_id, result, environment) in results {
            match result {
                Ok(true) => {
                    self.emit_document_events(
                        &id,
                        &document_id,
                        vec![json!({"type":"environment_changed","environment":environment})],
                    );
                    self.document_changed(&id, event_loop);
                }
                Ok(false) => {}
                Err(error) => self.document_error(&id, &document_id, error),
            }
        }
        self.notify("desktop.preferences_changed", observation.value());
    }

    fn document_error(
        &mut self,
        window_id: &str,
        document_id: &str,
        error: impl std::fmt::Display,
    ) {
        tracing::warn!(window = window_id, document = document_id, %error, "Document operation failed");
        self.notify(
            "document.error",
            json!({"window_id":window_id,"document_id":document_id,
            "message":error.to_string().chars().take(2048).collect::<String>()}),
        );
    }

    pub(super) fn emit_document_events(
        &mut self,
        window_id: &str,
        document_id: &str,
        events: Vec<Value>,
    ) {
        let current: Vec<_> = self
            .windows
            .get(window_id)
            .and_then(|w| w.document.as_ref())
            .filter(|d| d.id == document_id)
            .map(|d| {
                events
                    .into_iter()
                    .filter(|e| d.engine.event_is_current(e))
                    .collect()
            })
            .unwrap_or_default();
        for event in current {
            let params = json!({"window_id":window_id,"document_id":document_id,"event":event});
            if protocol::encoded_size(&protocol::notification("document.event", params.clone()))
                .is_err()
            {
                self.document_error(window_id, document_id, "Document event exceeded the encoded 2 MiB protocol limit; its payload was not delivered");
                continue;
            }
            self.notify("document.event", params);
        }
    }

    pub(super) fn document_input(
        &mut self,
        id: &str,
        input: Input,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) {
        let Some((document_id, result)) = self
            .windows
            .get_mut(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_mut())
            .map(|d| (d.id.clone(), d.engine.handle_input(input)))
        else {
            return;
        };
        match result {
            Ok(events) => self.emit_document_events(id, &document_id, events),
            Err(error) => self.document_error(id, &document_id, error),
        }
        self.document_changed(id, event_loop);
    }

    /// Coalesce backend updates within one pump batch. Geometry needed by an
    /// explicit query is flushed by the engine, but ordinary DOM writes must
    /// not force a full accessibility/layout traversal for every token.
    pub(super) fn refresh_document(&mut self, id: &str) {
        let outcome = (|| -> Result<(), ui4me_renderer::Error> {
            let Some(window) = self.windows.get_mut(id) else {
                return Ok(());
            };
            let Some(document) = &mut window.document else {
                return Ok(());
            };
            let viewport =
                window
                    .native
                    .as_ref()
                    .map_or((window.width, window.height, 1.0), |native| {
                        let size = native.window.surface_size();
                        (size.width, size.height, native.window.scale_factor())
                    });
            if viewport.0 == 0 || viewport.1 == 0 {
                return Ok(());
            }
            if viewport != document.viewport {
                document
                    .engine
                    .set_viewport(viewport.0, viewport.1, viewport.2)?;
                document.viewport = viewport;
            }
            if let Some(native) = &mut window.native {
                // Invalidate an old native composition immediately, before a
                // later IME event in this same dispatch batch can reach new focus.
                let replaced = native.document_epoch.as_deref() != Some(document.id.as_str());
                if replaced {
                    let _ = native.window.request_ime_update(ImeRequest::Disable);
                    native.document_epoch = Some(document.id.clone());
                    native.ime_enabled = false;
                    native.composing = false;
                    native.preedit_cleared = false;
                    native.ime_resets = document.engine.ime_reset_count();
                    native.ime_quiet_until = Some(Instant::now() + Duration::from_millis(250));
                }
                if replaced || native.ime_resets != document.engine.ime_reset_count() {
                    update_ime(&document.engine, native);
                }
                native
                    .window
                    .set_cursor_visible(document.engine.cursor() != "none");
                native
                    .window
                    .set_cursor(cursor(document.engine.cursor()).into());
                document.native_refresh_pending = true;
                if document.engine.needs_repaint() || document.a11y.is_none() {
                    native.window.request_redraw();
                }
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            if let Some(document_id) = self
                .windows
                .get(id)
                .and_then(|w| w.document.as_ref())
                .map(|d| d.id.clone())
            {
                self.document_error(id, &document_id, error);
            }
        }
    }

    pub(super) fn flush_document_refreshes(&mut self) {
        let ids: Vec<_> = self
            .windows
            .iter_mut()
            .filter_map(|(id, window)| {
                let document = window.document.as_mut()?;
                if std::mem::take(&mut document.native_refresh_pending) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for id in ids {
            self.flush_document_refresh(&id);
        }
    }

    fn flush_document_refresh(&mut self, id: &str) {
        let outcome = (|| -> Result<(), ui4me_renderer::Error> {
            let Some(window) = self.windows.get_mut(id) else {
                return Ok(());
            };
            let Some(document) = &mut window.document else {
                return Ok(());
            };
            let viewport =
                window
                    .native
                    .as_ref()
                    .map_or((window.width, window.height, 1.0), |native| {
                        let size = native.window.surface_size();
                        (size.width, size.height, native.window.scale_factor())
                    });
            if viewport.0 == 0 || viewport.1 == 0 {
                return Ok(());
            }
            if viewport != document.viewport {
                document
                    .engine
                    .set_viewport(viewport.0, viewport.1, viewport.2)?;
                document.viewport = viewport;
            }
            let Some(native) = &mut window.native else {
                return Ok(());
            };
            let repaint = document.engine.needs_repaint();
            if repaint || document.a11y.is_none() {
                let tree = document.engine.accessibility_tree_titled(&window.title)?;
                // The adapter maintains a synchronous full tree plus monotonic
                // native IDs and emits only changed nodes to a running AT client.
                native
                    .accessibility
                    .update_document(&document.id, ui4me_renderer::a11y::full_update(&tree));
                document.a11y = Some(tree);
            }
            native
                .window
                .set_cursor_visible(document.engine.cursor() != "none");
            native
                .window
                .set_cursor(cursor(document.engine.cursor()).into());
            update_ime(&document.engine, native);
            if repaint {
                native.window.request_redraw();
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            if let Some(document_id) = self
                .windows
                .get(id)
                .and_then(|w| w.document.as_ref())
                .map(|d| d.id.clone())
            {
                self.document_error(id, &document_id, error);
            }
        }
    }

    /// Drain each engine queue exactly once and hand native work to bounded,
    /// asynchronous host services. No renderer/platform callback blocks IPC.
    pub(super) fn document_changed(&mut self, id: &str, event_loop: Option<&dyn ActiveEventLoop>) {
        let Some((document_id, events, clipboard, files)) = self
            .windows
            .get_mut(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_mut())
            .map(|d| {
                (
                    d.id.clone(),
                    d.engine.drain_events(),
                    d.engine.take_clipboard_requests(),
                    d.engine.take_file_requests(),
                )
            })
        else {
            return;
        };
        self.emit_document_events(id, &document_id, events);
        for request in clipboard {
            match request {
                ClipboardRequest::Read { id: request_id } => {
                    let target = TransferTarget::DocumentClipboard {
                        window_id: id.into(),
                        document_id: document_id.clone(),
                        request_id,
                    };
                    if let Err(error) = self.read_clipboard(event_loop, target) {
                        self.complete_document_clipboard(
                            id,
                            &document_id,
                            request_id,
                            Err(error.to_string()),
                        );
                    }
                }
                ClipboardRequest::Write {
                    id: request_id,
                    text,
                } => {
                    let result = self
                        .write_clipboard(event_loop, text)
                        .map(|_| None)
                        .map_err(|e| e.to_string());
                    self.complete_document_clipboard(id, &document_id, request_id, result);
                }
            }
        }
        for request in files {
            let filters = file_filters(&request.accept);
            let job_id = self.next_job;
            let result = self.start_dialog(
                "dialog.open_files",
                json!({"window_id":id,
                "title":"Choose file","multiple":request.multiple,"filters":filters}),
                None,
            );
            match result {
                Ok(()) => {
                    if let Some(job) = self.jobs.get_mut(&job_id) {
                        job.kind = JobKind::DocumentDialog {
                            document_id: document_id.clone(),
                            node: request.input,
                        };
                    }
                }
                Err(error) => {
                    self.document_error(id, &document_id, &error);
                    self.complete_document_dialog(
                        id,
                        &document_id,
                        request.input,
                        Ok(json!({"cancelled":true,"paths":[]})),
                        event_loop,
                    );
                }
            }
        }
        self.refresh(id);
    }

    pub(super) fn complete_document_clipboard(
        &mut self,
        id: &str,
        document_id: &str,
        request_id: u64,
        result: Result<Option<String>, String>,
    ) {
        let transport_error = result.as_ref().err().cloned();
        let Some(result) = self
            .windows
            .get_mut(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_mut())
            .filter(|d| d.id == document_id)
            .map(|d| d.engine.complete_clipboard(request_id, result))
        else {
            return;
        };
        if let Some(error) = transport_error {
            self.document_error(id, document_id, format!("Native clipboard: {error}"));
        }
        match result {
            Ok(events) => self.emit_document_events(id, document_id, events),
            Err(error) => self.document_error(id, document_id, error),
        }
        self.refresh(id);
    }

    pub(super) fn complete_document_dialog(
        &mut self,
        id: &str,
        document_id: &str,
        node: ui4me_renderer::NodeId,
        result: Result<Value, RpcError>,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) {
        if !self
            .windows
            .get(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_ref())
            .is_some_and(|d| d.id == document_id)
        {
            return;
        }
        let paths = match result {
            Ok(value) if value["cancelled"] == false => Some(
                value["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
            Ok(_) => None,
            Err(error) => {
                self.document_error(id, document_id, error);
                None
            }
        };
        let Some(result) = self
            .windows
            .get_mut(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_mut())
            .filter(|d| d.id == document_id)
            .map(|d| d.engine.complete_file_input(node, paths))
        else {
            return;
        };
        if let Err(error) = result {
            self.document_error(id, document_id, error);
        }
        self.document_changed(id, event_loop);
    }

    pub(super) fn prune_document_work(&mut self, id: &str) {
        let epoch = self
            .windows
            .get(id)
            .and_then(|w| w.document.as_ref())
            .map(|d| d.id.as_str());
        let keep = |target: &TransferTarget| match target {
            TransferTarget::DocumentClipboard {
                window_id,
                document_id,
                ..
            }
            | TransferTarget::DocumentDrop {
                window_id,
                document_id,
                ..
            } => window_id != id || Some(document_id.as_str()) == epoch,
            _ => true,
        };
        self.transfers.retain(|_, t| keep(&t.target));
        self.deferred_clipboard_reads.retain(keep);
        for job in self
            .jobs
            .values_mut()
            .filter(|j| j.parent.as_deref() == Some(id))
        {
            if matches!(&job.kind, JobKind::DocumentDialog { document_id, .. } if Some(document_id.as_str()) != epoch)
            {
                job.cancelled = true;
                if let Some(dialog) = &job.dialog {
                    dialog.cancel();
                }
            }
        }
    }

    pub(super) fn poll_documents(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        let ids: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| !w.closing && w.document.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let Some((document_id, result)) = self
                .windows
                .get_mut(&id)
                .and_then(|w| w.document.as_mut())
                .map(|d| (d.id.clone(), d.engine.poll_resources()))
            else {
                continue;
            };
            match result {
                Ok(true) => self.document_changed(&id, event_loop),
                Ok(false) => {}
                Err(error) => self.document_error(&id, &document_id, error),
            }
        }
    }

    pub(super) fn document_wait_interval(&self, host_interval: Duration) -> Duration {
        self.windows
            .values()
            .filter(|w| !w.closing)
            .filter_map(|w| w.document.as_ref())
            .filter_map(|d| d.engine.next_wake_in())
            .fold(host_interval, Duration::min)
            .max(Duration::from_millis(1))
    }

    /// Empty preedit followed by Commit is one IME transaction. Only cancel
    /// it at the end of the native dispatch batch if no Commit followed.
    pub(super) fn finish_document_preedits(&mut self, event_loop: &dyn ActiveEventLoop) {
        let ids: Vec<_> = self
            .windows
            .iter_mut()
            .filter_map(|(id, w)| {
                let native = w.native.as_mut()?;
                if w.document.is_some() && std::mem::take(&mut native.preedit_cleared) {
                    native.composing = false;
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for id in ids {
            self.document_input(&id, Input::ImeDisabled, Some(event_loop));
        }
    }

    pub(super) fn document_keyboard(
        &mut self,
        id: &str,
        event: &KeyEvent,
        event_loop: &dyn ActiveEventLoop,
    ) {
        let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) else {
            return;
        };
        let m = native.modifiers;
        if event.state.is_pressed() {
            native.ime_quiet_until = None;
        }
        let composing = native.composing;
        let key = match &event.logical_key {
            Key::Character(text) => text.to_string(),
            Key::Named(named) => format!("{named:?}"),
            Key::Dead(_) => "Dead".into(),
            _ => "Unidentified".into(),
        };
        if event.state.is_pressed()
            && m.control_key()
            && m.shift_key()
            && key.eq_ignore_ascii_case("q")
        {
            self.force_shutdown();
            return;
        }
        self.window_event_message(
            id,
            "key",
            json!({"key":key,"state":if event.state.is_pressed(){"pressed"}else{"released"},
            "repeat":event.repeat,"physical_key":format!("{:?}",event.physical_key)}),
        );
        let code = match event.physical_key {
            PhysicalKey::Code(code) => format!("{code:?}"),
            _ => "Unidentified".into(),
        };
        self.document_input(
            id,
            Input::Key {
                key,
                code,
                pressed: event.state.is_pressed(),
                repeat: event.repeat,
                shift: m.shift_key(),
                ctrl: m.control_key(),
                alt: m.alt_key(),
                meta: m.meta_key(),
            },
            Some(event_loop),
        );
        // AltGr is commonly represented by Ctrl+Alt on Windows. A normal Ctrl
        // shortcut must never insert its key text, and preedit owns composing text.
        if event.state.is_pressed()
            && !composing
            && !m.meta_key()
            && ((!m.control_key() && !m.alt_key()) || (m.control_key() && m.alt_key()))
        {
            if let Some(text) = &event.text {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                if !text.is_empty() {
                    self.document_input(id, Input::Text(text), Some(event_loop));
                }
            }
        }
    }

    /// Returns true only for input consumed by a hosted document. Window
    /// lifecycle, presentation, scale and native data transfers stay in Host.
    pub(super) fn document_window_event(
        &mut self,
        id: &str,
        event: &WindowEvent,
        event_loop: &dyn ActiveEventLoop,
    ) -> bool {
        if !self.windows.get(id).is_some_and(|w| w.document.is_some()) {
            return false;
        }
        let scale = self
            .windows
            .get(id)
            .and_then(|w| w.native.as_ref())
            .map_or(1.0, |n| n.window.scale_factor());
        let input = match event {
            WindowEvent::KeyboardInput { event, .. } => {
                self.document_keyboard(id, event, event_loop);
                return true;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let m = modifiers.state();
                if let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) {
                    native.modifiers = m;
                }
                Some(Input::Modifiers {
                    shift: m.shift_key(),
                    ctrl: m.control_key(),
                    alt: m.alt_key(),
                    meta: m.meta_key(),
                })
            }
            WindowEvent::ThemeChanged(theme) => {
                let observation = self.preferences.snapshot();
                if let Some((document_id, result, environment)) = self
                    .windows
                    .get_mut(id)
                    .and_then(|w| w.document.as_mut())
                    .map(|d| {
                        (
                            d.id.clone(),
                            d.engine.set_system_preferences(
                                observation
                                    .dark
                                    .or(Some(*theme == winit::window::Theme::Dark)),
                                observation.reduced_motion,
                                observation.high_contrast,
                            ),
                            d.engine.environment(),
                        )
                    })
                {
                    match result {
                        Ok(_) => self.emit_document_events(
                            id,
                            &document_id,
                            vec![json!({"type":"environment_changed","environment":environment})],
                        ),
                        Err(error) => self.document_error(id, &document_id, error),
                    }
                    self.document_changed(id, Some(event_loop));
                }
                return true;
            }
            WindowEvent::Focused(focused) => {
                self.window_event_message(id, "focused", json!({"focused":focused}));
                if !focused {
                    if let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) {
                        native.composing = false;
                        native.preedit_cleared = false;
                    }
                }
                Some(Input::Focus(*focused))
            }
            WindowEvent::PointerMoved {
                position,
                primary: true,
                ..
            }
            | WindowEvent::PointerEntered {
                position,
                primary: true,
                ..
            } => {
                let (x, y) = (position.x / scale, position.y / scale);
                if let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) {
                    native.pointer = (x, y);
                }
                Some(Input::PointerMoved { x, y })
            }
            WindowEvent::PointerButton {
                state,
                position,
                primary: true,
                button,
                ..
            } => {
                let button = match button.clone().mouse_button() {
                    Some(MouseButton::Left) => 0,
                    Some(MouseButton::Middle) => 1,
                    Some(MouseButton::Right) => 2,
                    Some(MouseButton::Back) => 3,
                    Some(MouseButton::Forward) => 4,
                    _ => return true,
                };
                let (x, y) = (position.x / scale, position.y / scale);
                if let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) {
                    native.pointer = (x, y);
                }
                Some(Input::PointerButton {
                    pressed: state.is_pressed(),
                    button,
                    x,
                    y,
                })
            }
            WindowEvent::PointerLeft { primary: true, .. } => Some(Input::PointerLeft),
            WindowEvent::PointerMoved { .. }
            | WindowEvent::PointerEntered { .. }
            | WindowEvent::PointerButton { .. }
            | WindowEvent::PointerLeft { .. } => return true,
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (-f64::from(*x) * 40.0, -f64::from(*y) * 40.0)
                    }
                    MouseScrollDelta::PixelDelta(p) => (-p.x / scale, -p.y / scale),
                    _ => return true,
                };
                let (x, y) = self
                    .windows
                    .get(id)
                    .and_then(|w| w.native.as_ref())
                    .map_or((0.0, 0.0), |n| n.pointer);
                Some(Input::Wheel { dx, dy, x, y })
            }
            WindowEvent::Ime(ime) => {
                let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) else {
                    return true;
                };
                if native
                    .ime_quiet_until
                    .is_some_and(|until| Instant::now() < until)
                    && matches!(
                        ime,
                        Ime::Preedit(..) | Ime::Commit(..) | Ime::DeleteSurrounding { .. }
                    )
                {
                    return true;
                }
                match ime {
                    Ime::Enabled => Some(Input::ImeEnabled),
                    Ime::Disabled => {
                        native.composing = false;
                        native.preedit_cleared = false;
                        Some(Input::ImeDisabled)
                    }
                    Ime::Preedit(text, cursor) => {
                        if text.len() > ui4me_renderer::MAX_TEXT {
                            return true;
                        }
                        native.composing = !text.is_empty();
                        native.preedit_cleared = text.is_empty();
                        Some(Input::ImePreedit {
                            text: text.clone(),
                            cursor: cursor.filter(|(a, b)| {
                                text.is_char_boundary(*a) && text.is_char_boundary(*b)
                            }),
                        })
                    }
                    Ime::Commit(text) => {
                        native.composing = false;
                        native.preedit_cleared = false;
                        if text.len() > ui4me_renderer::MAX_TEXT {
                            return true;
                        }
                        Some(Input::ImeCommit(text.clone()))
                    }
                    Ime::DeleteSurrounding {
                        before_bytes,
                        after_bytes,
                    } => {
                        let result = self
                            .windows
                            .get_mut(id)
                            .and_then(|w| w.document.as_mut())
                            .map(|d| {
                                (
                                    d.id.clone(),
                                    d.engine.ime_delete_surrounding(*before_bytes, *after_bytes),
                                )
                            });
                        if let Some((doc, result)) = result {
                            match result {
                                Ok(events) => self.emit_document_events(id, &doc, events),
                                Err(e) => self.document_error(id, &doc, e),
                            }
                            self.document_changed(id, Some(event_loop));
                        }
                        return true;
                    }
                }
            }
            WindowEvent::DragEntered {
                position: Some(position),
                ..
            }
            | WindowEvent::DragPosition { position, .. } => {
                if let Some(native) = self.windows.get_mut(id).and_then(|w| w.native.as_mut()) {
                    native.pointer = (position.x / scale, position.y / scale);
                }
                return false;
            }
            _ => return false,
        };
        if let Some(input) = input {
            self.document_input(id, input, Some(event_loop));
        }
        true
    }

    pub(super) fn incoming_drag(
        &mut self,
        id: &str,
        transfer: winit::data_transfer::DataTransferId,
        position: Option<(f64, f64)>,
        phase: &str,
        event_loop: &dyn ActiveEventLoop,
    ) {
        use winit::{data_transfer::TypeHint, event_loop::DndAction};
        let formats = event_loop
            .data_transfer(transfer)
            .ok()
            .filter(|_| self.jobs.len() + self.transfers.len() < protocol::MAX_PENDING)
            .map(|data| {
                (
                    data.has_type(&TypeHint::UriList),
                    data.has_type(&TypeHint::Plaintext),
                )
            });
        let Some(record) = self.windows.get_mut(id).filter(|w| !w.closing) else {
            return;
        };
        let Some(native) = &mut record.native else {
            return;
        };
        let point = position.map(|(x, y)| {
            (
                x / native.window.scale_factor(),
                y / native.window.scale_factor(),
            )
        });
        let mut files = None;
        let mut target = None;
        if let (Some((x, y)), Some((has_files, has_text))) = (point, formats) {
            if let Some(document) = &mut record.document {
                if has_files {
                    target = document.engine.native_drop_target(x, y, true);
                    if target.is_some() {
                        files = Some(true);
                    }
                }
                if target.is_none() && has_text {
                    target = document.engine.native_drop_target(x, y, false);
                    if target.is_some() {
                        files = Some(false);
                    }
                }
            } else if has_files || has_text {
                files = Some(has_files);
            }
        }
        let actions = if files.is_some() {
            &[DndAction::Copy][..]
        } else {
            &[][..]
        };
        if let Err(error) = event_loop.set_valid_dnd_actions(transfer, actions) {
            tracing::debug!(%error,"Native drop acceptance unavailable");
            files = None;
            target = None;
        }
        let state = IncomingDrop {
            transfer,
            document_id: record.document.as_ref().map(|d| d.id.clone()),
            target,
            files,
            point: point.unwrap_or((0.0, 0.0)),
        };
        let changed = native.incoming_drop.as_ref().is_none_or(|old| {
            old.transfer != state.transfer
                || old.document_id != state.document_id
                || old.target != state.target
                || old.files != state.files
        });
        native.incoming_drop = Some(state.clone());
        // Report transitions rather than unbounded pointer-motion messages to
        // a slow language backend. Geometry remains current in the saved state.
        if changed {
            self.emit_drop_feedback(id, &state, phase);
        }
    }

    pub(super) fn emit_drop_feedback(&mut self, id: &str, state: &IncomingDrop, phase: &str) {
        if let Some(document_id) = &state.document_id {
            self.emit_document_events(
                id,
                document_id,
                vec![json!({"type":"drag_feedback", "phase":phase,
                "accepted":phase!="leave" && state.files.is_some(),
                "kind":state.files.map(|files|if files {"files"}else{"text"}),
                "element":state.target.as_ref().map(|t|&t["element"]),
                "node":state.target.as_ref().map(|t|&t["node"]),
                "x":state.point.0,"y":state.point.1,"external":true})],
            );
        }
    }

    // The separate document epoch, receiver token and point are deliberate:
    // native completion must not reacquire a target after an asynchronous read.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn complete_document_drop(
        &mut self,
        id: &str,
        document_id: &str,
        target: &Value,
        files: bool,
        point: (f64, f64),
        result: Result<String, RpcError>,
    ) {
        let Some(document) = self
            .windows
            .get_mut(id)
            .filter(|w| !w.closing)
            .and_then(|w| w.document.as_mut())
            .filter(|d| d.id == document_id)
        else {
            return;
        };
        if !document.engine.native_drop_accepts(target, files) {
            return;
        }
        let (x, y) = point;
        let element = &target["element"];
        let node = &target["node"];
        match result {
            Ok(text) => {
                let event = if files {
                    let lines: Vec<_> = text
                        .lines()
                        .filter(|l| !l.starts_with('#') && !l.is_empty())
                        .collect();
                    if lines.len() > 1024 {
                        self.document_error(id, document_id, "Native drop exceeds 1024 files");
                        return;
                    }
                    let paths: Result<Vec<String>, _> = lines
                        .into_iter()
                        .map(|line| {
                            url::Url::parse(line)
                                .ok()
                                .and_then(|uri| uri.to_file_path().ok())
                                .and_then(|path| path.into_os_string().into_string().ok())
                                .ok_or("Native file drop contains a non-local or non-Unicode path")
                        })
                        .collect();
                    let paths = match paths {
                        Ok(paths) => paths,
                        Err(error) => {
                            self.document_error(id, document_id, error);
                            return;
                        }
                    };
                    json!({"type":"drop","element":element,"node":node,"x":x,"y":y,"paths":paths,"external":true})
                } else {
                    json!({"type":"drop","element":element,"node":node,"x":x,"y":y,"text":text,"external":true})
                };
                self.emit_document_events(id, document_id, vec![event]);
            }
            Err(error) => self.document_error(id, document_id, error),
        }
    }
}

/// Both native adapters accept extension filters. Map the common MIME forms
/// explicitly; unknown MIME types leave the picker unrestricted instead of
/// silently excluding files. Accept is a chooser hint, never file validation.
fn file_filters(accept: &[String]) -> Vec<Value> {
    let mut extensions = Vec::new();
    for value in accept {
        let values: Vec<&str> = if let Some(ext) = value.strip_prefix('.') {
            vec![ext]
        } else {
            match value.as_str() {
                "image/*" => vec!["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"],
                "image/png" => vec!["png"],
                "image/jpeg" => vec!["jpg", "jpeg"],
                "image/svg+xml" => vec!["svg"],
                "text/plain" => vec!["txt"],
                "text/csv" => vec!["csv"],
                "application/json" => vec!["json"],
                "application/pdf" => vec!["pdf"],
                _ => return Vec::new(),
            }
        };
        for ext in values {
            if ext.is_empty()
                || ext.len() > 32
                || !ext
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Vec::new();
            }
            if !extensions.iter().any(|e| e == ext) {
                extensions.push(ext.to_owned());
            }
        }
    }
    if extensions.is_empty() {
        Vec::new()
    } else {
        vec![json!({"name":"Accepted files","extensions":extensions})]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ime_hints_use_inputmode_and_never_offer_password_completions() {
        let (h, p) = ime_policy(
            &json!({"input_type":"email","inputmode":"numeric","autocomplete":"on","spellcheck":true}),
            true,
        );
        assert_eq!(p, ImePurpose::Pin);
        assert!(h.contains(ImeHint::SENSITIVE_DATA | ImeHint::HIDDEN_TEXT));
        assert!(!h.intersects(ImeHint::COMPLETION | ImeHint::SPELLCHECK));
        let (h, p) = ime_policy(
            &json!({"input_type":"textarea","multiline":true,"spellcheck":true}),
            false,
        );
        assert_eq!(p, ImePurpose::Normal);
        assert!(h.contains(ImeHint::MULTILINE | ImeHint::SPELLCHECK));
        assert_eq!(
            ime_policy(&json!({"input_type":"email"}), false).1,
            ImePurpose::Email
        );
    }
    #[test]
    fn oversized_document_event_reports_error_without_disconnect_or_truncation() {
        let mut host = super::super::tests::controller(true);
        super::super::tests::add_window(&mut host, "w1");
        let document = host
            .dispatch_document(
                "document.load",
                json!({"window_id":"w1","html":"<p>test</p>"}),
            )
            .unwrap();
        let id = document["document_id"].as_str().unwrap();
        host.observed_notifications.clear();
        host.emit_document_events(
            "w1",
            id,
            vec![json!({"type":"drop","text":"\u{1}".repeat(400_000)})],
        );
        assert_eq!(host.observed_notifications.len(), 1);
        assert_eq!(host.observed_notifications[0]["method"], "document.error");
        assert_eq!(host.client, Some(7));
    }
    #[test]
    fn native_accept_filters_are_hints_without_pattern_injection() {
        assert_eq!(
            file_filters(&[".png".into(), "image/png".into()])[0]["extensions"],
            json!(["png"])
        );
        assert!(file_filters(&["application/x-custom".into()]).is_empty());
        assert!(file_filters(&[".*;exe".into()]).is_empty());
    }
}
