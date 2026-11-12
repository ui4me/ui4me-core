use super::*;
use crate::presentation::{ProbeAction, EDITOR, TEXT_RUN};
use accesskit::{Action, ActionData};
use std::io::Read;
use winit::{
    application::ApplicationHandler,
    data_transfer::{DataTransferSendBuilder, TypeHint, TypedData},
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ButtonSource, Ime, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ControlFlow, DndAction},
    keyboard::{Key, NamedKey},
    window::{
        ImeCapabilities, ImeEnableRequest, ImeHint, ImePurpose, ImeRequest, ImeRequestData,
        ImeSurroundingText,
    },
};

pub(super) fn update_ime(probe: &Probe, native: &mut NativeWindow) {
    if probe.focus_index != 0 {
        if native.ime_enabled {
            let _ = native.window.request_ime_update(ImeRequest::Disable);
            native.ime_enabled = false;
        }
        return;
    }
    let (text, cursor, anchor) = probe.surrounding();
    let Ok(surrounding) = ImeSurroundingText::new(text, cursor, anchor) else {
        return;
    };
    let (x, y, width, height) = probe.caret_rect(native.window.scale_factor());
    let data = ImeRequestData::default()
        .with_hint_and_purpose(ImeHint::NONE, ImePurpose::Normal)
        .with_cursor_area(
            PhysicalPosition::new(x, y).into(),
            PhysicalSize::new(width, height).into(),
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
        Ok(()) => native.ime_enabled = true,
        Err(error) => tracing::debug!(%error, "IME request unavailable"),
    }
}

/// Matches the vendored softbuffer Wayland backend's non-blocking busy error
/// (third_party/softbuffer-0.4.8, COMPOSITOR_HOLDS_BUFFERS).
const COMPOSITOR_HOLDS_BUFFERS: &str = "Wayland compositor still holds every buffer";

impl Host {
    pub(super) fn write_clipboard(
        &self,
        event_loop: Option<&dyn ActiveEventLoop>,
        text: String,
    ) -> Result<(), RpcError> {
        let event_loop = event_loop
            .ok_or_else(|| RpcError::unsupported("Clipboard requires a native event loop"))?;
        let data = DataTransferSendBuilder::new(text)
            .with_type(TypeHint::Plaintext, |text: &String, _| Some(text.clone()))
            .build();
        event_loop
            .set_clipboard(data)
            .map_err(|e| RpcError::unavailable(format!("Clipboard write: {e}")))
    }

    pub(super) fn read_clipboard(
        &mut self,
        event_loop: Option<&dyn ActiveEventLoop>,
        target: TransferTarget,
    ) -> Result<(), RpcError> {
        let event_loop = event_loop
            .ok_or_else(|| RpcError::unsupported("Clipboard requires a native event loop"))?;
        if self.transfers.len() + self.jobs.len() >= protocol::MAX_PENDING {
            return Err(RpcError::busy("Too many pending native requests"));
        }
        let id = event_loop
            .clipboard()
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        match id {
            Some(id) => {
                let serial = event_loop
                    .fetch_data_transfer(id, &TypeHint::Plaintext)
                    .map_err(|e| RpcError::unavailable(format!("Clipboard read: {e}")))?;
                self.transfers.insert(
                    serial,
                    PendingTransfer {
                        target,
                        deadline: Instant::now() + Duration::from_secs(10),
                    },
                );
            }
            None => self.finish_transfer(target, Ok(String::new())),
        }
        Ok(())
    }

    pub(super) fn finish_transfer(
        &mut self,
        target: TransferTarget,
        result: Result<String, RpcError>,
    ) {
        match target {
            TransferTarget::Rpc(reply) => {
                self.respond(&reply, result.map(|text| json!({"text":text})))
            }
            TransferTarget::Paste(id) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    match result {
                        Ok(text) => {
                            if !window.probe.insert(&text) {
                                window.probe.status = "Paste exceeds probe size limit.".into();
                            }
                        }
                        Err(error) => window.probe.status = error.to_string(),
                    }
                }
                self.text_changed(&id);
            }
            TransferTarget::DropText(id) => match result {
                Ok(text) => {
                    if let Some(window) = self.windows.get_mut(&id) {
                        if !window.probe.insert(&text) {
                            window.probe.status = "Dropped text exceeds probe size limit.".into();
                        }
                    }
                    self.window_event_message(&id, "dropped_text", json!({"text":text}));
                    self.text_changed(&id);
                }
                Err(error) => {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.probe.status = error.to_string();
                    }
                    self.refresh(&id);
                }
            },
            TransferTarget::Drop(id) => match result {
                Ok(text) => {
                    let files: Vec<_> = text
                        .lines()
                        .filter(|line| !line.starts_with('#') && !line.is_empty())
                        .take(1024)
                        .filter_map(|line| url::Url::parse(line).ok())
                        .filter_map(|uri| uri.to_file_path().ok())
                        .filter_map(|path| path.into_os_string().into_string().ok())
                        .collect();
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.probe.status = format!("Dropped {} file(s).", files.len());
                    }
                    self.window_event_message(&id, "dropped_files", json!({"paths":files}));
                    self.refresh(&id);
                }
                Err(error) => {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.probe.status = error.to_string();
                    }
                    self.refresh(&id);
                }
            },
        }
    }

    fn text_changed(&mut self, id: &str) {
        if let Some(window) = self.windows.get(id) {
            let data = json!({"text":window.probe.text,"cursor":window.probe.cursor,"anchor":window.probe.anchor});
            self.window_event_message(id, "text", data);
        }
        self.refresh(id);
    }

    fn probe_action(
        &mut self,
        id: &str,
        action: ProbeAction,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) {
        let outcome = match action {
            ProbeAction::Copy => {
                let text = self
                    .windows
                    .get(id)
                    .map(|w| {
                        if w.probe.selection().is_empty() {
                            w.probe.text.clone()
                        } else {
                            w.probe.selected_text().to_owned()
                        }
                    })
                    .unwrap_or_default();
                self.write_clipboard(event_loop, text)
            }
            ProbeAction::Paste => self.read_clipboard(event_loop, TransferTarget::Paste(id.into())),
            ProbeAction::OpenFile => {
                self.start_dialog("dialog.open_files", json!({"window_id":id}), None)
            }
            ProbeAction::Quit => self.request_quit("native_probe"),
        };
        if let Err(error) = outcome {
            if let Some(window) = self.windows.get_mut(id) {
                window.probe.status = error.to_string();
            }
        }
        self.refresh(id);
    }

    pub(super) fn accessibility_action(
        &mut self,
        event: ActionEvent,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) {
        let id = event.window_id;
        let mut action = None;
        if let Some(window) = self.windows.get_mut(&id) {
            match (
                event.request.action,
                event.request.target_node,
                event.request.data,
            ) {
                (Action::Focus, node, _) if (2..=6).contains(&node.0) => {
                    window.probe.focus_index = (node.0 - 2) as usize;
                    if let Some(native) = &window.native {
                        native.window.focus_window();
                    }
                }
                (Action::Click, node, _) if (3..=6).contains(&node.0) => {
                    window.probe.focus_index = (node.0 - 2) as usize;
                    action = window.probe.activate();
                }
                (Action::SetValue, EDITOR, Some(ActionData::Value(text))) => {
                    if let Err(error) = window.probe.set_text(&text) {
                        window.probe.status = error.into();
                    }
                }
                (
                    Action::SetTextSelection,
                    EDITOR,
                    Some(ActionData::SetTextSelection(selection)),
                ) if selection.anchor.node == TEXT_RUN && selection.focus.node == TEXT_RUN => {
                    window.probe.select_characters(
                        selection.anchor.character_index,
                        selection.focus.character_index,
                    );
                }
                _ => {}
            }
        }
        if let Some(action) = action {
            self.probe_action(&id, action, event_loop);
        }
        self.text_changed(&id);
    }

    fn keyboard(&mut self, id: &str, event: KeyEvent, event_loop: &dyn ActiveEventLoop) {
        let key_name = match &event.logical_key {
            Key::Character(s) => s.to_string(),
            Key::Named(n) => format!("{n:?}"),
            _ => format!("{:?}", event.logical_key),
        };
        self.window_event_message(
            id,
            "key",
            json!({"key":key_name,"state":if event.state.is_pressed() {"pressed"} else {"released"},
            "repeat":event.repeat,"physical_key":format!("{:?}",event.physical_key)}),
        );
        if !event.state.is_pressed() {
            return;
        }
        let Some(record) = self.windows.get_mut(id) else {
            return;
        };
        let modifiers = record
            .native
            .as_ref()
            .map_or(ModifiersState::empty(), |n| n.modifiers);
        let shortcut = modifiers.control_key() && !modifiers.alt_key();
        let shift = modifiers.shift_key();
        let mut action = None;
        let mut force = false;
        match event.logical_key.as_ref() {
            Key::Character(text) if shortcut => match text.to_lowercase().as_str() {
                "a" if record.probe.focus_index == 0 => record.probe.select_all(),
                "c" => action = Some(ProbeAction::Copy),
                "v" => action = Some(ProbeAction::Paste),
                "q" if shift => force = true,
                "q" => action = Some(ProbeAction::Quit),
                _ => {}
            },
            Key::Named(NamedKey::Tab) => record.probe.tab(shift),
            Key::Named(NamedKey::Enter) | Key::Character(" ") if record.probe.focus_index != 0 => {
                action = record.probe.activate()
            }
            Key::Named(named)
                if record.probe.focus_index == 0 && record.probe.preedit.is_empty() =>
            {
                match named {
                    NamedKey::ArrowLeft => record.probe.move_cursor(-1, shift),
                    NamedKey::ArrowRight => record.probe.move_cursor(1, shift),
                    NamedKey::Home => record.probe.boundary(false, shift),
                    NamedKey::End => record.probe.boundary(true, shift),
                    NamedKey::Backspace => record.probe.delete(true),
                    NamedKey::Delete => record.probe.delete(false),
                    _ => {}
                }
            }
            _ if record.probe.focus_index == 0 && record.probe.preedit.is_empty() && !shortcut => {
                if let Some(text) = event.text {
                    record.probe.insert(&text);
                }
            }
            _ => {}
        }
        if force {
            self.force_shutdown();
        } else if let Some(action) = action {
            self.probe_action(id, action, Some(event_loop));
        }
        self.text_changed(id);
    }

    fn draw(&mut self, id: &str) {
        let result: Result<()> = (|| {
            let Some(record) = self.windows.get_mut(id) else {
                return Ok(());
            };
            let Some(native) = &mut record.native else {
                return Ok(());
            };
            let size = native.window.surface_size();
            let (Some(width), Some(height)) =
                (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            else {
                return Ok(());
            };
            if u64::from(size.width) * u64::from(size.height) > 64 * 1024 * 1024 {
                anyhow::bail!("Surface exceeds the 64 megapixel probe safety limit");
            }
            native
                .surface
                .resize(width, height)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut buffer = native
                .surface
                .buffer_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            record.probe.render(
                &mut buffer,
                size.width,
                size.height,
                native.window.scale_factor(),
                &record.title,
            );
            native.window.pre_present_notify();
            buffer.present().map_err(|e| anyhow::anyhow!("{e}"))?;
            if !native.presented {
                native.presented = true;
                tracing::info!(
                    window = id,
                    since_host_start_ms = self.started.elapsed().as_millis() as u64,
                    "first frame presented"
                );
            }
            native.accessibility.update(record.probe.tree(
                &record.title,
                size.width,
                size.height,
                native.window.scale_factor(),
            ));
            update_ime(&record.probe, native);
            Ok(())
        })();
        if let Err(error) = result {
            if error.to_string().contains(COMPOSITOR_HOLDS_BUFFERS) {
                // The compositor has not yet released any of this window's
                // bounded buffers (e.g. it is fully occluded). Never wait for
                // it on the GUI thread: skip this frame and retry on a later,
                // frame-callback-paced redraw.
                tracing::debug!(
                    window = id,
                    "frame deferred until the compositor releases a buffer"
                );
                if let Some(native) = self.windows.get(id).and_then(|w| w.native.as_ref()) {
                    native.window.request_redraw();
                }
            } else {
                tracing::error!(%error, "native presentation failed");
            }
        }
    }
}

impl ApplicationHandler for Host {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.standalone && self.windows.is_empty() {
            match self.create_window(
                json!({"title":self.config.app.name,"width":900,"height":560}),
                Some(event_loop),
            ) {
                Ok(value) => {
                    if let Some(id) = value["window_id"].as_str() {
                        if let Some(window) = self.windows.get_mut(id) {
                            let _ = window.probe.set_text("Type here. Try accents, selection, clipboard and your input method.");
                        }
                        self.refresh(id);
                    }
                }
                Err(error) => {
                    self.exit_code.store(1, Ordering::Relaxed);
                    tracing::error!(%error, "probe creation failed");
                    event_loop.exit();
                }
            }
        }
        self.pump(Some(event_loop));
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.pump(Some(event_loop));
    }
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.pump(Some(event_loop));
        // Workers wake the loop for messages, results and backend exit. The
        // timer only enforces deadlines, so poll quickly while time-bounded work
        // exists and back off when the application is simply idle.
        let deadlines_pending = self.lifecycle.phase() != Phase::Running
            || !self.transfers.is_empty()
            || !self.jobs.is_empty()
            || self.connected.keys().any(|id| Some(*id) != self.client);
        let interval = if deadlines_pending { 50 } else { 500 };
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(interval),
        ));
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        native_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(id) = self.native_ids.get(&native_id).cloned() else {
            return;
        };
        if let Some(native) = self.windows.get_mut(&id).and_then(|r| r.native.as_mut()) {
            native.accessibility.event(native.window.as_ref(), &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                if let Err(error) = self.request_close(&id, "native_close") {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.probe.status = format!("{error}. Ctrl+Shift+Q forces shutdown.");
                    }
                    self.refresh(&id);
                }
            }
            WindowEvent::RedrawRequested => self.draw(&id),
            WindowEvent::SurfaceResized(size) => {
                let scale = self
                    .windows
                    .get(&id)
                    .and_then(|w| w.native.as_ref())
                    .map_or(1.0, |n| n.window.scale_factor());
                if let Some(record) = self.windows.get_mut(&id) {
                    record.width = (f64::from(size.width) / scale) as u32;
                    record.height = (f64::from(size.height) / scale) as u32;
                }
                self.window_event_message(&id, "resized", json!({"physical_width":size.width,"physical_height":size.height,"scale_factor":scale}));
                self.refresh(&id);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_event_message(
                    &id,
                    "scale_changed",
                    json!({"scale_factor":scale_factor}),
                );
                self.refresh(&id);
            }
            WindowEvent::Focused(focused) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.probe.focused = focused;
                    if !focused {
                        window.probe.preedit.clear();
                    }
                }
                self.window_event_message(&id, "focused", json!({"focused":focused}));
                self.refresh(&id);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(native) = self.windows.get_mut(&id).and_then(|r| r.native.as_mut()) {
                    native.modifiers = modifiers.state();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => self.keyboard(&id, event, event_loop),
            WindowEvent::Ime(ime) => {
                if let Some(record) = self.windows.get_mut(&id) {
                    match ime {
                        Ime::Commit(text) => {
                            record.probe.insert(&text);
                            record.probe.preedit_cursor = None;
                        }
                        Ime::Preedit(text, cursor) => {
                            if text.len() <= crate::presentation::MAX_TEXT_BYTES {
                                record.probe.preedit_cursor = cursor.filter(|(a, b)| {
                                    text.is_char_boundary(*a) && text.is_char_boundary(*b)
                                });
                                record.probe.preedit = text;
                            }
                        }
                        Ime::DeleteSurrounding {
                            before_bytes,
                            after_bytes,
                        } => {
                            record.probe.delete_surrounding(before_bytes, after_bytes);
                        }
                        Ime::Disabled => {
                            record.probe.preedit.clear();
                            record.probe.preedit_cursor = None;
                        }
                        Ime::Enabled => {}
                    }
                }
                self.text_changed(&id);
            }
            WindowEvent::PointerButton {
                state,
                button: ButtonSource::Mouse(MouseButton::Left),
                position,
                ..
            } if state.is_pressed() => {
                let action = self
                    .windows
                    .get_mut(&id)
                    .and_then(|record| {
                        let native = record.native.as_mut()?;
                        native.drag_selecting =
                            (100.0..158.0).contains(&(position.y / native.window.scale_factor()));
                        Some(record.probe.click(
                            position.x,
                            position.y,
                            native.window.surface_size().width,
                            native.window.scale_factor(),
                            native.modifiers.shift_key(),
                        ))
                    })
                    .flatten();
                if let Some(action) = action {
                    self.probe_action(&id, action, Some(event_loop));
                }
                self.text_changed(&id);
            }
            WindowEvent::PointerButton {
                state,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } if !state.is_pressed() => {
                if let Some(native) = self.windows.get_mut(&id).and_then(|r| r.native.as_mut()) {
                    native.drag_selecting = false;
                }
            }
            WindowEvent::PointerMoved { position, .. } => {
                let mut changed = false;
                if let Some(record) = self.windows.get_mut(&id) {
                    if let Some(native) = &record.native {
                        if native.drag_selecting {
                            record.probe.click(
                                position.x,
                                120.0 * native.window.scale_factor(),
                                native.window.surface_size().width,
                                native.window.scale_factor(),
                                true,
                            );
                            changed = true;
                        }
                    }
                }
                if changed {
                    self.text_changed(&id);
                }
            }
            WindowEvent::DragEntered { id: transfer, .. }
            | WindowEvent::DragPosition { id: transfer, .. } => {
                if event_loop.data_transfer(transfer).is_ok_and(|data| {
                    data.has_type(&TypeHint::UriList) || data.has_type(&TypeHint::Plaintext)
                }) {
                    let _ = event_loop.set_valid_dnd_actions(transfer, &[DndAction::Copy]);
                }
            }
            WindowEvent::DragDropped { id: transfer, .. } => {
                if self.transfers.len() + self.jobs.len() < protocol::MAX_PENDING {
                    let file_drop = event_loop
                        .data_transfer(transfer)
                        .is_ok_and(|data| data.has_type(&TypeHint::UriList));
                    let hint = if file_drop {
                        TypeHint::UriList
                    } else {
                        TypeHint::Plaintext
                    };
                    if let Ok(serial) = event_loop.fetch_data_transfer(transfer, &hint) {
                        self.transfers.insert(
                            serial,
                            PendingTransfer {
                                target: if file_drop {
                                    TransferTarget::Drop(id.clone())
                                } else {
                                    TransferTarget::DropText(id.clone())
                                },
                                deadline: Instant::now() + Duration::from_secs(10),
                            },
                        );
                    }
                }
            }
            WindowEvent::DataTransferReceived { serial, value, .. } => {
                if self.transfers.contains_key(&serial) {
                    match bounded_transfer(value.as_ref()) {
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Deadlock
                            ) => {}
                        result => {
                            if let Some(pending) = self.transfers.remove(&serial) {
                                self.finish_transfer(
                                    pending.target,
                                    result.map_err(|e| {
                                        RpcError::unavailable(format!("Read native data: {e}"))
                                    }),
                                );
                            }
                        }
                    }
                }
            }
            WindowEvent::Destroyed => {
                self.native_ids.remove(&native_id);
                if let Some(record) = self.windows.get_mut(&id) {
                    record.native.take();
                }
                self.remove_window(&id);
                if self.windows.values().all(|window| window.closing) {
                    self.begin_shutdown();
                }
            }
            _ => {}
        }
    }
}

fn bounded_transfer(value: &dyn TypedData) -> std::io::Result<String> {
    const LIMIT: usize = 1024 * 1024;
    let mut bytes = Vec::new();
    if let Some(reader) = value.try_read() {
        reader.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > LIMIT {
            return Err(std::io::Error::other("Data transfer exceeds 1 MiB"));
        }
        return String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    }
    // Native Windows URI arrays may not expose a byte reader.
    let text = if value.type_().hint() == Some(TypeHint::UriList) {
        value.try_as_uris()?.join("\r\n")
    } else {
        value.try_as_string()?
    };
    if text.len() > LIMIT {
        return Err(std::io::Error::other("Data transfer exceeds 1 MiB"));
    }
    Ok(text)
}
