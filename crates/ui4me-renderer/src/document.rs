//! The application-facing document engine. Backend stages never cross this seam.
//! Every method is owner-thread only. Input records events; the host invokes
//! foreign callbacks only after this engine operation releases all its borrows.
use crate::interaction::dispatch::{self, DispatchHost, InteractionRuntime, Invocation};
use crate::interaction::frame::Modifiers;
use crate::interaction::input_queue::{InputItem, drain_input};
use crate::interaction::listeners::{ListenerSpec, Released};
use crate::interaction::scroll::{ScrollApplied, ScrollTargetInfo};
use crate::layout::coordinator::overflow::{ScrollError, ScrollTarget};
use crate::pipeline::resources::{PipelineResources, SharedResources};
use crate::pipeline::state::{DocumentPipeline, PipelineConfig, PipelineState};
use crate::raster::headless::PixelFormat;
use crate::scene::hit_index::{HitOptions, ScrollAxis, ViewportPoint};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use ui4me_assets::ResourceManager;
use ui4me_assets::decode::default_decoder;
use ui4me_assets::policy::{ResourcePolicy, ResourcePolicyPatch, RuntimeResourceModes};
use ui4me_document::control_semantics::InputType;
use ui4me_document::document::Document as Model;
use ui4me_document::limits::DomLimits;
use ui4me_engine_types::clipboard::ClipboardData;
use ui4me_engine_types::codes_generated::{Code, Family};
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{CallbackToken, DocumentId, NodeId, RawId};
use ui4me_engine_types::input::{
    InputKind, InputOrigin, KeyData, PointerData, utf8_to_utf16_offset,
};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const MAX_TEXT: usize = 16 * 1024;
pub const MAX_PIXELS: usize = 16_777_216;
const MAX_HTML: usize = 1024 * 1024;
const MAX_NODES: usize = 65_536;
const MAX_DEPTH: usize = 256;
const MAX_EVENTS: usize = 1024;
const BLINK: Duration = Duration::from_millis(500);
static NEXT_DOCUMENT: AtomicU32 = AtomicU32::new(1);
static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Error {
    pub code: i32,
    pub message: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<Status> for Error {
    fn from(value: Status) -> Self {
        let code = match value.code {
            Code::SizeLimit | Code::OutOfMemory => 5,
            Code::StaleHandle | Code::WrongDocument => 4,
            Code::DocumentClosed | Code::InvalidState => 2,
            _ => match value.family() {
                Family::Threading => 3,
                Family::Resource => 9,
                Family::Platform | Family::PaintBackend => 6,
                _ => 1,
            },
        };
        Error {
            code,
            message: format!("{}: {}", value.code.name(), value.message),
        }
    }
}
fn arg(message: impl Into<String>) -> Error {
    Error {
        code: 1,
        message: message.into(),
    }
}
fn invalid(message: impl Into<String>) -> Status {
    Status::new(Code::InvalidArgument, Stage::Host).with_message(message)
}
fn limit(message: impl Into<String>) -> Status {
    Status::new(Code::SizeLimit, Stage::Host).with_message(message)
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Synchronous clipboard seam for headless callers and engine tests. Native
/// hosts use `enable_deferred_clipboard` and its request/completion transport.
pub trait Clipboard {
    fn read_text(&mut self) -> std::result::Result<Option<String>, String>;
    fn write_text(&mut self, text: &str) -> std::result::Result<(), String>;
    /// HTML with its plain-text alternative (0.7 F0706). Default: the text only.
    fn write_html(&mut self, html: &str, alt_text: &str) -> std::result::Result<(), String> {
        let _ = html;
        self.write_text(alt_text)
    }
    /// The HTML flavour, if the clipboard holds one. Default: none.
    fn read_html(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(None)
    }
}
struct MemoryClipboard;
impl Clipboard for MemoryClipboard {
    fn read_text(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(lock(&CLIPBOARD).clone())
    }
    fn write_text(&mut self, text: &str) -> std::result::Result<(), String> {
        *lock(&CLIPBOARD) = Some(text.to_owned());
        *lock(&CLIPBOARD_HTML) = None;
        Ok(())
    }
    fn write_html(&mut self, html: &str, alt_text: &str) -> std::result::Result<(), String> {
        *lock(&CLIPBOARD) = Some(alt_text.to_owned());
        *lock(&CLIPBOARD_HTML) = Some(html.to_owned());
        Ok(())
    }
    fn read_html(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(lock(&CLIPBOARD_HTML).clone())
    }
}
/// The in-process clipboard's HTML flavour (headless sessions).
static CLIPBOARD_HTML: Mutex<Option<String>> = Mutex::new(None);
#[derive(Clone, Debug, PartialEq)]
pub struct ImeState {
    pub password: bool,
    pub cursor_area: (f64, f64, f64, f64),
}
/// Positions are CSS pixels; preedit cursor positions are UTF-8 byte offsets.
#[derive(Clone, Debug)]
pub enum Input {
    PointerMoved {
        x: f64,
        y: f64,
    },
    PointerButton {
        pressed: bool,
        button: i8,
        x: f64,
        y: f64,
    },
    PointerLeft,
    Wheel {
        dx: f64,
        dy: f64,
        x: f64,
        y: f64,
    },
    Key {
        key: String,
        code: String,
        pressed: bool,
        repeat: bool,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    },
    Modifiers {
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    },
    Text(String),
    Focus(bool),
    ImePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ImeCommit(String),
    ImeEnabled,
    ImeDisabled,
}
struct CaretBlink {
    node: NodeId,
    deadline: Instant,
    visible: bool,
    upstream: bool,
}
/// One canonical document and its owner-local renderer/interaction state.
pub struct Document {
    pipeline: DocumentPipeline,
    interaction: InteractionRuntime,
    resources: SharedResources,
    events: Vec<Value>,
    errors: VecDeque<Value>,
    clipboard: Box<dyn Clipboard>,
    cursor: String,
    ime: Option<ImeState>,
    /// Platform IME resets requested so far (the native host compares it, 0.5 F0501).
    ime_resets: u64,
    blink: Option<CaretBlink>,
    started: Instant,
    /// The one document clock: smooth scroll and CSS transitions and animations (0.9 F0901).
    clock: crate::pipeline::clock::SharedClock,
    modifiers: Modifiers,
    pointer_buttons: u16,
    pointer: (f64, f64),
    window_focused: bool,
    /// Media preferences: what the OS reported and what the app chose (0.5 F0506).
    env: Environment,
    /// File inputs whose picker the host must open, and the files chosen per input (0.6 F0606).
    file_requests: Vec<NodeId>,
    /// Element drag and drop (0.7 F0707).
    drag: drag::DragState,
    /// Plain-text editing hosts (0.8 F0801).
    editable: editable::EditableState,
    /// CSS `resize` drags (0.9 F0903).
    resize: resize::ResizeState,
    /// The last pagination (1.0 F1001).
    #[cfg(test)]
    paged: paged_facade::PagedState,
    input_files: HashMap<NodeId, Vec<String>>,
    repaint: bool,
    prevent: Vec<StoredRule>,
    follow: Vec<Follow>,
    /// Smooth scrolls in progress and the owner clock that drives them (0.8 F0803).
    scroll_anims: scroll_anim::ScrollAnims,
    selection: selection::DocSelection,
    /// Application-host profile; excludes document-authoring and page-output features.
    hosted: bool,
    native_clipboard: hosted::DeferredClipboard,
    /// Whether the immediately paired native text event belongs to a
    /// cancelled keydown. IME commits are independent input transactions.
    suppress_next_text: bool,
}
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("id", &self.id())
            .field("state", &self.pipeline.state())
            .finish_non_exhaustive()
    }
}
impl Document {
    /// Retained donor regression constructor. Applications use `parse_hosted`.
    #[cfg(test)]
    pub(crate) fn parse(html: &str) -> Result<Self> {
        Self::parse_with_source(html, None, 640, 480)
    }
    #[cfg(test)]
    pub(crate) fn parse_with_source(
        html: &str,
        base: Option<&Path>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::parse_profile(html, base, width, height, false)
    }
    /// The v0.2 hosted desktop profile, with explicit resource budgets and no
    /// rich contenteditable or page-output interface.
    pub fn parse_hosted(html: &str, base: Option<&Path>, width: u32, height: u32) -> Result<Self> {
        Self::parse_profile(html, base, width, height, true)
    }
    fn parse_profile(
        html: &str,
        base: Option<&Path>,
        width: u32,
        height: u32,
        hosted: bool,
    ) -> Result<Self> {
        check_html(html)?;
        let index = NEXT_DOCUMENT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| Error {
                code: 5,
                message: "Document identity space exhausted".into(),
            })?;
        let id = DocumentId::from_raw(RawId {
            index: 0,
            generation: NonZeroU32::new(index).expect("nonzero document generation"),
        });
        let base = base
            .map(std::fs::canonicalize)
            .transpose()
            .map_err(|e| Error {
                code: 9,
                message: format!("Cannot resolve asset base: {e}"),
            })?;
        if base.as_ref().is_some_and(|p| !p.is_dir()) {
            return Err(arg("Asset base must be a directory"));
        }
        let url = match &base {
            Some(path) => url::Url::from_directory_path(path)
                .map_err(|_| arg("Asset base must be an absolute directory"))?,
            None => url::Url::parse("about:blank").map_err(|e| arg(e.to_string()))?,
        };
        let limits = DomLimits {
            input_bytes: MAX_HTML as u64,
            nodes: MAX_NODES as u64,
            string_bytes: MAX_HTML as u64,
            serialization_bytes: 4 * MAX_HTML as u64,
            batch_commands: 1024,
            batch_bytes: MAX_HTML as u64,
        };
        let model = Model::create(id, html, url.as_str(), limits)?;
        validate_document(&model, None, None, hosted)?;
        let policy = ResourcePolicy {
            decoded_memory_budget: if hosted {
                hosted::DECODED_BYTES
            } else {
                128 * 1024 * 1024
            },
            cache_budget: if hosted {
                hosted::CACHE_BYTES
            } else {
                64 * 1024 * 1024
            },
            max_decoded_pixels: MAX_PIXELS as u64,
            ..ResourcePolicy::default()
        };
        let mut manager =
            ResourceManager::new(RuntimeResourceModes::default(), policy, default_decoder())?;
        if hosted {
            manager.set_svg_raster_budget_before_use(hosted::SVG_BYTES)?;
        }
        let patch = ResourcePolicyPatch {
            file_roots: base.map(|p| vec![p]),
            ..ResourcePolicyPatch::default()
        };
        manager.create_document(id, url.clone(), None, &patch)?;
        manager.set_local_family_lookup(Some(Arc::new(
            crate::pipeline::font_faces::system_family_exists,
        )));
        let fonts = manager.runtime_font_sources();
        let resources = Arc::new(Mutex::new(manager));
        let config = PipelineConfig {
            viewport_width: f64::from(width),
            viewport_height: f64::from(height),
            scale: 1.0,
            font_files: Vec::new(),
        };
        let mut pipeline = DocumentPipeline::from_document(model, config, fonts)?;
        pipeline.attach_resources(Arc::new(PipelineResources::new(resources.clone(), id, url)))?;
        let mut interaction = InteractionRuntime::with_event_log_capacity(64);
        pipeline.mutate(|d| interaction.attach_document(d))?;
        let clock = crate::pipeline::clock::SharedClock::default();
        pipeline.follow_clock(clock.clone());
        let root = pipeline.document().document_node();
        for event in [
            "click",
            "dblclick",
            "auxclick",
            "contextmenu",
            "beforeinput",
            "input",
            "select",
            "selectionchange",
            "copy",
            "cut",
            "paste",
            "change",
            "focus",
            "blur",
            "focusin",
            "focusout",
            "keydown",
            "keyup",
            "mousedown",
            "mouseup",
            "mousemove",
            "mouseover",
            "mouseout",
            "mouseenter",
            "mouseleave",
            "pointerdown",
            "pointerup",
            "pointermove",
            "pointercancel",
            "gotpointercapture",
            "lostpointercapture",
            "wheel",
            "scroll",
            "compositionstart",
            "compositionupdate",
            "compositionend",
            "submit",
            "reset",
            "invalid",
            "toggle",
            "beforetoggle",
            "cancel",
            "close",
            "transitionrun",
            "transitionstart",
            "transitionend",
            "transitioncancel",
            "animationstart",
            "animationiteration",
            "animationend",
            "animationcancel",
        ] {
            interaction.add_listener(
                pipeline.document(),
                root,
                ListenerSpec::new(event, CallbackToken::new(1).expect("nonzero")).capture(true),
            )?;
        }
        Ok(Self {
            pipeline,
            interaction,
            resources,
            events: Vec::new(),
            errors: VecDeque::new(),
            clipboard: Box::new(MemoryClipboard),
            cursor: "default".into(),
            ime: None,
            ime_resets: 0,
            blink: None,
            started: Instant::now(),
            clock,
            modifiers: Modifiers::default(),
            pointer_buttons: 0,
            pointer: (0.0, 0.0),
            window_focused: true,
            env: Environment::default(),
            file_requests: Vec::new(),
            drag: Default::default(),
            editable: Default::default(),
            resize: Default::default(),
            #[cfg(test)]
            paged: Default::default(),
            input_files: HashMap::new(),
            repaint: true,
            prevent: Vec::new(),
            follow: Vec::new(),
            scroll_anims: Default::default(),
            selection: selection::DocSelection::default(),
            hosted,
            native_clipboard: Default::default(),
            suppress_next_text: false,
        })
    }
    /// Read-only access for in-crate regression tests (update records).
    #[cfg(test)]
    pub(crate) fn pipeline_for_tests(&self) -> &DocumentPipeline {
        &self.pipeline
    }
    pub(crate) fn id(&self) -> DocumentId {
        self.pipeline.document().id()
    }
    fn node(&self, element: &str) -> Result<NodeId> {
        self.pipeline
            .element_by_id(element)
            .ok_or_else(|| arg(format!("Unknown element ID: {element}")))
    }
    pub fn set_waker(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        lock(&self.resources).set_wake(Some(wake));
    }
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }
    pub fn cursor(&self) -> &str {
        &self.cursor
    }
    pub fn ime_request(&self) -> Option<&ImeState> {
        self.ime.as_ref().filter(|_| self.window_focused)
    }
    /// How many times the document asked the platform to drop its preedit; the native host resets
    /// the input method whenever this changes.
    pub fn ime_reset_count(&self) -> u64 {
        self.ime_resets
    }
    pub fn needs_repaint(&self) -> bool {
        self.repaint || self.pipeline.state() == PipelineState::Dirty
    }
    pub fn drain_events(&mut self) -> Vec<Value> {
        // Queued element tasks (dialog `close`, `toggle`) run at the end of each host operation.
        let id = self.id();
        crate::interaction::dialog::dispatch_element_tasks(self, id);
        self.dispatch_animation_events();
        std::mem::take(&mut self.events)
    }
    /// Recheck before each foreign callback: an earlier callback may have removed
    /// this target and created an element with the same public ID.
    pub fn event_is_current(&self, event: &Value) -> bool {
        let decode = |name: &str| -> Option<NodeId> {
            let token = event.get(name)?.as_array()?;
            let index = u32::try_from(token.first()?.as_u64()?).ok()?;
            let generation = NonZeroU32::new(u32::try_from(token.get(1)?.as_u64()?).ok()?)?;
            Some(NodeId::from_raw(RawId { index, generation }))
        };
        // Document-level notifications (`selectionchange`) carry no target and are always current.
        if event.get("_target").is_none() && event.get("_element").is_none() {
            return true;
        }
        let Some(target) = decode("_target") else {
            return false;
        };
        let Some(element) = decode("_element") else {
            return false;
        };
        let d = self.pipeline.document();
        d.arena().contains(target)
            && d.is_connected(target).unwrap_or(false)
            && d.arena().contains(element)
            && d.is_connected(element).unwrap_or(false)
            && d.get_attribute(element, "id").ok().flatten().unwrap_or("")
                == event.get("element").and_then(Value::as_str).unwrap_or("")
    }
    pub fn get_value(&self, element: &str) -> Result<String> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        if d.local_name(n)? == "select" {
            return Ok(d.select_value(n));
        }
        // A range's value is the sanitised one (clamped, step-snapped, default midpoint).
        if let Some(v) = d.range_value(n) {
            return Ok(ui4me_document::control_values::format_float(v));
        }
        match d.control_value(n) {
            Some(value) => Ok(value),
            None => Ok(self.pipeline.text_content(n)?),
        }
    }
    pub fn set_text(&mut self, element: &str, text: &str) -> Result<()> {
        check_text(text)?;
        let n = self.node(element)?;
        if crate::interaction::editing::text_kind(self.pipeline.document(), n).is_some() {
            return self.set_value(element, text);
        }
        self.pipeline.set_text_content(n, text)?;
        self.after_mutation(true);
        Ok(())
    }
    pub fn set_value(&mut self, element: &str, value: &str) -> Result<()> {
        check_text(value)?;
        let n = self.node(element)?;
        if !matches!(
            self.pipeline.document().local_name(n)?,
            "input" | "textarea" | "select"
        ) {
            return Err(arg("set_value requires a form control"));
        }
        self.pipeline
            .mutate(|d| crate::interaction::controls::host_set_value(d, n, value))?;
        let value = self
            .pipeline
            .document()
            .control_value(n)
            .unwrap_or_default();
        if self.interaction.host_value_written(self.id(), n, &value) {
            self.ime_resets += 1;
        }
        self.after_mutation(false);
        Ok(())
    }
    pub fn get_checked(&self, element: &str) -> Result<bool> {
        let n = self.node(element)?;
        self.pipeline
            .document()
            .control_state(n)?
            .and_then(|s| s.checkedness)
            .ok_or_else(|| arg("get_checked requires a checkbox or radio"))
    }
    pub fn set_checked(&mut self, element: &str, checked: bool) -> Result<()> {
        let n = self.node(element)?;
        self.get_checked(element)?;
        self.pipeline
            .mutate(|d| crate::interaction::controls::host_set_checked(d, n, checked))?;
        self.after_mutation(false);
        Ok(())
    }
    pub fn set_attribute(&mut self, element: &str, name: &str, value: Option<&str>) -> Result<()> {
        self.run_ops(
            &[DomOp::SetAttribute {
                element: element.into(),
                name: name.into(),
                value: value.map(Into::into),
            }],
            false,
        )
    }
    pub fn add_class(&mut self, element: &str, class: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::AddClass {
                element: element.into(),
                name: class.into(),
            }],
            false,
        )
    }
    pub fn remove_class(&mut self, element: &str, class: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::RemoveClass {
                element: element.into(),
                name: class.into(),
            }],
            false,
        )
    }
    pub fn replace_children_html(&mut self, element: &str, html: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::ReplaceChildrenHtml {
                element: element.into(),
                html: html.into(),
            }],
            false,
        )
    }
    /// Parse `html` as a fragment and insert it relative to `element` (`insertAdjacentHTML`
    /// positions). The fragment is validated (active content, IDs unique against the whole
    /// document, limits) before anything is inserted; a rejection leaves the document unchanged.
    pub fn insert_html(&mut self, element: &str, position: &str, html: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::InsertHtml {
                element: element.into(),
                position: position.into(),
                html: html.into(),
            }],
            false,
        )
    }
    /// Remove `element` and its subtree. Focus inside it moves to the viewport with no events;
    /// its resources, queued events and ID are retired.
    pub fn remove_element(&mut self, element: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::RemoveElement {
                element: element.into(),
            }],
            false,
        )
    }
    /// Move `element` (with its subtree) relative to `target`. Identity, control values,
    /// selection and focus within the moved subtree are preserved; no focus events fire.
    pub fn move_element(&mut self, element: &str, target: &str, position: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::MoveElement {
                element: element.into(),
                target: target.into(),
                position: position.into(),
            }],
            false,
        )
    }
    /// Apply `ops` atomically: all succeed with one document revision (one style/layout
    /// invalidation), or the first failure rolls every earlier operation back and the error
    /// message names the failing operation index. Value/checked/focus changes are not batch
    /// operations because they carry interaction state outside the document tree.
    pub fn apply_batch(&mut self, ops: &[DomOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        if ops.len() > MAX_BATCH_OPS {
            return Err(limit_error(format!(
                "A batch holds at most {MAX_BATCH_OPS} operations"
            )));
        }
        let bytes: usize = ops.iter().map(DomOp::payload_len).sum();
        if bytes > MAX_HTML * 4 {
            return Err(limit_error("Batch payload exceeds 4 MiB"));
        }
        self.run_ops(ops, true)
    }
    fn run_ops(&mut self, ops: &[DomOp], atomic: bool) -> Result<()> {
        self.run_ops_resolved(ops, atomic, false)
    }
    fn run_ops_resolved(&mut self, ops: &[DomOp], atomic: bool, nodes: bool) -> Result<()> {
        self.invalidate_clipboard_context();
        let id = self.id();
        let d = self.pipeline.document();
        let focused = self.interaction.focused_element(id);
        let visible = focused.is_some_and(|f| {
            d.ui_flags(f)
                .is_ok_and(|g| g.contains(ui4me_document::ui_state::UiFlags::FOCUS_VISIBLE))
        });
        let hosted = self.hosted;
        let outcome: Result<()> = self.pipeline.mutate(|d| {
            Ok(if atomic {
                d.transaction(|d| {
                    for (i, op) in ops.iter().enumerate() {
                        apply_op(d, op, hosted, nodes).map_err(|e| Error {
                            code: e.code,
                            message: format!("batch operation {i}: {}", e.message),
                        })?;
                    }
                    Ok(())
                })
            } else {
                ops.iter().try_for_each(|op| apply_op(d, op, hosted, nodes))
            })
        })?;
        // A move (or a rolled-back removal) passes through the DOM removing steps, which reset
        // focus with no events. Restore it when the element is still a connected focusable area.
        if let Some(f) = focused
            && self.interaction.focused_element(id).is_none()
        {
            let interaction = &mut self.interaction;
            self.pipeline.mutate(|d| {
                if crate::interaction::focus::is_focusable(d, f) {
                    interaction.set_focus(d, Some(f), visible);
                }
                Ok(())
            })?;
        }
        outcome?;
        self.after_mutation(ops.iter().any(DomOp::is_structural));
        Ok(())
    }
    /// Register (`prevent = true`) or remove a host rule that cancels the default action of
    /// `event` dispatched at `element` or inside it, optionally only for one `key` value and
    /// exact modifier states. Bindings cannot cancel synchronously, because their callbacks run
    /// after dispatch; the delivered event reports `default_prevented`. Rules are retired with
    /// their element.
    pub fn set_default_action(&mut self, rule: DefaultActionRule, prevent: bool) -> Result<()> {
        let n = self.node(&rule.element)?;
        if !PREVENTABLE.contains(&rule.event.as_str()) {
            return Err(arg(format!(
                "Default action of {} cannot be prevented; use one of: {}",
                rule.event,
                PREVENTABLE.join(", ")
            )));
        }
        if rule
            .key
            .as_ref()
            .is_some_and(|k| k.is_empty() || k.len() > 64)
        {
            return Err(arg("key must be 1..64 bytes"));
        }
        let d = self.pipeline.document();
        self.prevent
            .retain(|r| d.arena().contains(r.node) && d.is_connected(r.node).unwrap_or(false));
        let stored = StoredRule { node: n, rule };
        self.prevent.retain(|r| !r.same(&stored));
        if prevent {
            if self.prevent.len() >= MAX_PREVENT_RULES {
                return Err(limit_error(format!(
                    "At most {MAX_PREVENT_RULES} default-action rules per window"
                )));
            }
            self.prevent.push(stored);
        }
        Ok(())
    }
    fn apply_prevent_rules(&mut self, ev: ui4me_engine_types::ids::DispatchId, event_type: &str) {
        if self.prevent.is_empty() {
            return;
        }
        let Ok(event) = self.interaction.event(ev) else {
            return;
        };
        if !event.init.cancelable {
            return;
        }
        let Ok(path) = self.interaction.event_path(ev) else {
            return;
        };
        let data = &event.init.data;
        let m = event.init.modifiers;
        let d = self.pipeline.document();
        let hit = self.prevent.iter().any(|r| {
            let rule = &r.rule;
            rule.event == event_type
                && d.arena().contains(r.node)
                && path.contains(&r.node)
                && rule
                    .key
                    .as_ref()
                    .is_none_or(|k| data.key.as_ref() == Some(k))
                && rule.shift.is_none_or(|v| v == m.shift)
                && rule.ctrl.is_none_or(|v| v == m.ctrl)
                && rule.alt.is_none_or(|v| v == m.alt)
                && rule.meta.is_none_or(|v| v == m.meta)
        });
        if hit {
            let _ = self.interaction.prevent_default(ev);
        }
    }
    /// Apply a new device size/scale now, so geometry queries before the next frame are current.
    /// The OS colour scheme (`Some(true)` = dark; `None` = the platform does not say). An app
    /// override wins. Returns whether the scheme in force changed (one restyle; DOM, values, focus
    /// and selection are untouched).
    pub fn set_system_color_scheme(&mut self, dark: Option<bool>) -> Result<bool> {
        self.env.system_dark = dark;
        self.apply_environment()
    }
    /// The OS high-contrast mode (0.7 F0701): reported in the environment; Core does not apply
    /// `forced-colors` (apps may restyle on `environment_changed`).
    pub fn set_system_high_contrast(&mut self, on: Option<bool>) {
        self.env.system_high_contrast = on;
    }
    /// Replace the observed system preferences atomically. `None` means unknown, not false;
    /// application overrides continue to win. A source/unknown-state change is observable too.
    pub fn set_system_preferences(
        &mut self,
        dark: Option<bool>,
        reduced_motion: Option<bool>,
        high_contrast: Option<bool>,
    ) -> Result<bool> {
        let before = self.environment();
        self.env.system_dark = dark;
        self.env.system_reduced_motion = reduced_motion;
        self.env.system_high_contrast = high_contrast;
        let changed = self.apply_environment()?;
        Ok(changed || before != self.environment())
    }
    /// The app's media preferences: `Some(Some(v))` overrides, `Some(None)` returns to the system
    /// value, `None` leaves it. Returns whether anything in force changed.
    pub fn set_app_environment(
        &mut self,
        dark: Option<Option<bool>>,
        reduced_motion: Option<Option<bool>>,
    ) -> Result<bool> {
        if let Some(v) = dark {
            self.env.app_dark = v;
        }
        if let Some(v) = reduced_motion {
            self.env.app_reduced_motion = v;
        }
        self.apply_environment()
    }
    fn apply_environment(&mut self) -> Result<bool> {
        let dark = self.env.app_dark.or(self.env.system_dark).unwrap_or(false);
        let reduce = self.effective_reduced_motion();
        let changed = self.pipeline.set_dark(dark)? | self.pipeline.set_reduced_motion(reduce)?;
        if changed {
            self.repaint = true;
        }
        Ok(changed)
    }
    fn effective_reduced_motion(&self) -> bool {
        self.env
            .app_reduced_motion
            .or(self.env.system_reduced_motion)
            .unwrap_or(false)
    }
    /// The media preferences in force and where each comes from.
    pub fn environment(&self) -> Value {
        let scheme = |dark: bool| if dark { "dark" } else { "light" };
        let dark = self.env.app_dark.or(self.env.system_dark).unwrap_or(false);
        json!({
            "color_scheme": scheme(dark),
            "color_scheme_source": if self.env.app_dark.is_some() { "app" } else if self.env.system_dark.is_some() { "system" } else { "default" },
            "system_color_scheme": self.env.system_dark.map_or("unknown", scheme),
            "reduced_motion": if self.effective_reduced_motion() { "reduce" } else { "no-preference" },
            "reduced_motion_source": if self.env.app_reduced_motion.is_some() { "app" } else if self.env.system_reduced_motion.is_some() { "system" } else { "default" },
            "system_reduced_motion": self.env.system_reduced_motion,
            "scale": self.pipeline.config.scale,
            "high_contrast": self.env.system_high_contrast,
            "high_contrast_source": if self.env.system_high_contrast.is_some() { "system" } else { "unknown" },
        })
    }
    pub fn set_viewport(&mut self, width: u32, height: u32, scale: f64) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.pipeline.set_device_viewport(width, height, scale)?;
        self.repaint = true;
        Ok(())
    }
    fn scroll_node(&self, element: Option<&str>) -> Result<Option<NodeId>> {
        element.map(|e| self.node(e)).transpose()
    }
    /// Offset, reachable range and scrollport (CSS px) of an element's scroll container, or of the
    /// viewport when `element` is `None`. A non-scroll-container reports `scrollable: false`.
    pub fn scroll_state(&mut self, element: Option<&str>) -> Result<ScrollState> {
        let target = self.scroll_node(element)?;
        self.tick_scroll()?;
        self.apply_follow()?;
        self.pipeline.flush()?;
        Ok(self.state_of(target))
    }
    fn state_of(&self, target: Option<NodeId>) -> ScrollState {
        match self.pipeline.scroll_entry(scroll_target(target)) {
            Some(e) => ScrollState {
                x: e.offset.0,
                y: e.offset.1,
                min_x: e.range.min.0,
                min_y: e.range.min.1,
                max_x: e.range.max.0,
                max_y: e.range.max.1,
                width: e.scrollport.0,
                height: e.scrollport.1,
                scrollable: true,
                animating: self.scroll_anims.running(target),
            },
            None => ScrollState::default(),
        }
    }
    /// `scrollTo` with the element's `scroll-behavior` (`auto`); see [`Self::scroll_to_behavior`].
    pub fn scroll_to(
        &mut self,
        element: Option<&str>,
        x: Option<f64>,
        y: Option<f64>,
    ) -> Result<ScrollState> {
        self.scroll_to_behavior(element, x, y, "auto")
    }
    pub fn scroll_by(&mut self, element: Option<&str>, dx: f64, dy: f64) -> Result<ScrollState> {
        self.scroll_by_behavior(element, dx, dy, "auto")
    }
    pub fn scroll_into_view(&mut self, element: &str, block: &str, inline: &str) -> Result<()> {
        self.scroll_into_view_behavior(element, block, inline, "auto")
    }
    /// The accessibility tree of the committed document state (roles, names, states, values and
    /// viewport bounds); flushes the pipeline first.
    pub fn accessibility_tree(&mut self) -> Result<crate::a11y::A11yTree> {
        self.accessibility_tree_titled("")
    }
    /// The tree with `window_title` as the name of its root (the platform window).
    pub fn accessibility_tree_titled(
        &mut self,
        window_title: &str,
    ) -> Result<crate::a11y::A11yTree> {
        self.apply_follow()?;
        self.pipeline.flush()?;
        let id = self.id();
        let focused = self.interaction.focused_element(id);
        let info = A11yInfo {
            doc: self,
            id,
            committed: self.pipeline.committed.as_deref(),
        };
        let model = self.pipeline.document();
        let cfg = &self.pipeline.config;
        Ok(crate::a11y::build(&crate::a11y::BuildInput {
            document: model,
            info: &info,
            focused,
            title: window_title,
            viewport: (cfg.viewport_width, cfg.viewport_height),
            scale: cfg.scale,
        }))
    }
    /// A static-text accessibility object is a DOM text node, which cannot be scrolled
    /// itself: scroll its parent element instead.
    fn scroll_element_of(&self, el: NodeId) -> NodeId {
        let document = self.pipeline.document();
        if document
            .arena()
            .get(el)
            .is_some_and(|n| n.as_element().is_some())
        {
            el
        } else {
            document.parent(el).ok().flatten().unwrap_or(el)
        }
    }

    /// Execute an assistive-technology request as the equivalent trusted user input.
    pub fn accessibility_request(&mut self, request: &crate::a11y::A11yRequest) -> Result<()> {
        self.invalidate_clipboard_context();
        use crate::a11y::A11yRequestKind as K;
        use crate::interaction::assistive;
        let id = self.id();
        let Some(el) = request.target else {
            return Ok(());
        };
        if !self.pipeline.document().arena().is_connected(el) {
            return Err(Error {
                code: 4,
                message: "Accessibility target is no longer connected".into(),
            });
        }
        let centre = |d: &mut Self| {
            d.viewport_rect(el)
                .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
        };
        let expanded = |d: &Self| {
            d.pipeline
                .document()
                .arena()
                .get(el)
                .and_then(|n| n.as_element())
                .and_then(|e| e.attr("aria-expanded"))
                .map(|v| v == "true")
        };
        match &request.kind {
            K::Focus => assistive::focus(self, id, el)?,
            K::Blur => assistive::blur(self, id, el)?,
            K::Click => {
                self.pipeline.flush()?;
                let p = centre(self);
                assistive::activate(self, id, el, p)?
            }
            K::SetValue(v) => {
                assistive::set_text_value(self, id, el, v)?;
            }
            K::SetTextSelection { anchor, focus } => {
                let tree = self.accessibility_tree()?;
                if let (Some(a), Some(f)) = (tree.run_offset(*anchor), tree.run_offset(*focus)) {
                    assistive::set_selection(self, id, el, a, f)?;
                }
            }
            K::ReplaceSelectedText(text) => {
                assistive::replace_selection(self, id, el, text)?;
            }
            K::SetNumericValue(v) => {
                assistive::set_number(self, id, el, *v)?;
            }
            K::Increment => {
                assistive::step(self, id, el, 1)?;
            }
            K::Decrement => {
                assistive::step(self, id, el, -1)?;
            }
            K::ScrollIntoView => {
                let target = self.scroll_element_of(el);
                self.scroll_node_into_view(target, "nearest", "nearest", "instant")?
            }
            K::ScrollIntoViewWithHint {
                target_id,
                block,
                inline,
            } => {
                // Adapters send every ScrollIntoView here (AT-SPI always with a hint).
                let target = self.scroll_element_of(el);
                self.scroll_node_into_view(target, block, inline, "instant")?;
                let tree = self.accessibility_tree()?;
                if let Some(run) = tree.get(*target_id).filter(|n| n.run.is_some())
                    && let Some(bounds) = run.bounds
                    && let Some(g) = DispatchHost::control_geometry(self, id, el)?
                {
                    // Text-run bounds are viewport coordinates. Scroll the control's internal
                    // content without moving its caret, selection, focus or undo history.
                    let width = g.inner.viewport.size.width.to_f64();
                    let height = g.inner.viewport.size.height.to_f64();
                    let dx = align(inline, bounds.x - g.origin.0, bounds.w, 0.0, width);
                    let dy = align(block, bounds.y - g.origin.1, bounds.h, 0.0, height);
                    if dx != 0.0 || dy != 0.0 {
                        DispatchHost::scroll_set(
                            self,
                            id,
                            Some(el),
                            g.scroll.0 + dx,
                            g.scroll.1 + dy,
                        )?;
                    }
                }
            }
            K::Expand | K::Collapse => {
                let want = matches!(request.kind, K::Expand);
                if expanded(self) != Some(want) {
                    self.pipeline.flush()?;
                    let p = centre(self);
                    assistive::activate(self, id, el, p)?;
                }
            }
        }
        Ok(())
    }
    /// Compact JSON dump of the accessibility tree (diagnostics and tests).
    pub fn accessibility_json(&mut self) -> Result<Value> {
        Ok(self.accessibility_tree()?.to_json())
    }
    /// Border-box bounds of `element` in viewport CSS px (`getBoundingClientRect`), or `None`
    /// when it generates no box (for example `display: none`).
    pub fn element_rect(&mut self, element: &str) -> Result<Option<Rect>> {
        let n = self.node(element)?;
        self.apply_follow()?;
        self.pipeline.flush()?;
        Ok(self.viewport_rect(n).map(|(x, y, width, height)| Rect {
            x,
            y,
            width,
            height,
        }))
    }
    /// Keep a scroll container (or the viewport) at its end while content grows, as long as it
    /// was at the end. A user or programmatic scroll away stops following until it is back at
    /// the end. Follow adjustments do not emit `scroll` events.
    pub fn set_scroll_follow(&mut self, element: Option<&str>, follow: bool) -> Result<()> {
        let target = self.scroll_node(element)?;
        self.follow.retain(|f| f.target != target);
        if follow {
            if self.follow.len() >= 64 {
                return Err(limit_error("At most 64 follow targets per window"));
            }
            self.follow.push(Follow {
                target,
                pinned: true,
            });
            self.apply_follow()?;
        }
        Ok(())
    }
    fn apply_follow(&mut self) -> Result<()> {
        if self.follow.is_empty() {
            return Ok(());
        }
        let d = self.pipeline.document();
        self.follow.retain(|f| {
            f.target
                .is_none_or(|n| d.arena().contains(n) && d.is_connected(n).unwrap_or(false))
        });
        if !self.follow.iter().any(|f| f.pinned) {
            return Ok(());
        }
        self.pipeline.flush()?;
        let targets: Vec<Option<NodeId>> = self
            .follow
            .iter()
            .filter(|f| f.pinned && !self.scroll_anims.running(f.target))
            .map(|f| f.target)
            .collect();
        for t in targets {
            if let Some(e) = self.pipeline.scroll_entry(scroll_target(t))
                && e.offset.1 < e.range.max.1
                && let Ok(a) = self
                    .pipeline
                    .scroll_to(scroll_target(t), e.offset.0, e.range.max.1)
            {
                self.repaint |= a.changed;
            }
        }
        Ok(())
    }
    fn update_follow(&mut self) {
        if self.follow.is_empty() || self.pipeline.flush().is_err() {
            return;
        }
        for i in 0..self.follow.len() {
            let t = self.follow[i].target;
            self.follow[i].pinned = self
                .pipeline
                .scroll_entry(scroll_target(t))
                .is_none_or(|e| e.offset.1 >= e.range.max.1 - 1.0);
        }
    }
    fn viewport_rect(&self, n: NodeId) -> Option<(f64, f64, f64, f64)> {
        let c = self.pipeline.committed.as_deref()?;
        let tree = &c.layout.fragments;
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        let offsets = self.pipeline.scroll_offsets();
        let own = tree.fragments_of(n);
        // A control's inner content (a textarea's lines) also belongs to it and overflows its
        // scrollport: when the node has boxes, its rect is theirs.
        let is_text = |f: &crate::layout::fragment::FragmentId| {
            matches!(
                tree.get(*f).kind,
                crate::layout::fragment::FragmentKind::Text(_)
            )
        };
        let boxed = own.iter().any(|f| !is_text(f));
        for &f in own {
            if boxed && is_text(&f) {
                continue;
            }
            let r = tree.document_rect(f).rect;
            // `position: sticky` boxes (the fragment or an ancestor) are drawn shifted.
            let (mut dx, mut dy) = (0.0, 0.0);
            let mut at = Some(f);
            while let Some(a) = at {
                if let Ok((sx, sy)) =
                    crate::layout::coordinator::sticky::sticky_shift(tree, offsets, a)
                {
                    dx += sx;
                    dy += sy;
                }
                at = tree.get(a).parent;
            }
            let (ax, ay) =
                crate::layout::coordinator::sticky::anchor_scroll_shift(tree, offsets, f);
            dx += ax;
            dy += ay;
            let (x0, y0) = (r.origin.x.to_f64() + dx, r.origin.y.to_f64() + dy);
            let (x1, y1) = (x0 + r.size.width.to_f64(), y0 + r.size.height.to_f64());
            bounds = Some(match bounds {
                None => (x0, y0, x1, y1),
                Some(b) => (b.0.min(x0), b.1.min(y0), b.2.max(x1), b.3.max(y1)),
            });
        }
        let (x0, y0, x1, y1) = bounds?;
        let (mut sx, mut sy) = (0.0, 0.0);
        // A `fixed` box does not scroll with its containers (an anchored one follows its anchor, above).
        let fixed = tree.fragments_of(n).first().is_some_and(|&f| {
            matches!(&tree.get(f).kind, crate::layout::fragment::FragmentKind::Box(b)
                if b.extras.position == crate::layout::box_tree::PositionValue::Fixed)
        });
        for e in self.pipeline.scroll_chain(n).into_iter().filter(|_| !fixed) {
            if e.target != ScrollTarget::Node(n) {
                sx += e.offset.0;
                sy += e.offset.1;
            }
        }
        Some((x0 - sx, y0 - sy, x1 - x0, y1 - y0))
    }
    fn scrollport_rect(&self, target: ScrollTarget) -> Option<(f64, f64, f64, f64)> {
        let c = self.pipeline.committed.as_deref()?;
        let g = crate::layout::coordinator::overflow::scroll_geometry(&c.layout.fragments, target)?;
        let size = (
            g.scrollport.size.width.to_f64(),
            g.scrollport.size.height.to_f64(),
        );
        match target {
            ScrollTarget::Viewport => Some((0.0, 0.0, size.0, size.1)),
            ScrollTarget::Node(n) => {
                let (bx, by, _, _) = self.viewport_rect(n)?;
                Some((
                    bx + g.scrollport.origin.x.to_f64(),
                    by + g.scrollport.origin.y.to_f64(),
                    size.0,
                    size.1,
                ))
            }
        }
    }
    pub fn append_text(&mut self, element: &str, text: &str) -> Result<()> {
        self.run_ops(
            &[DomOp::AppendText {
                element: element.into(),
                text: text.into(),
            }],
            false,
        )
    }
    pub fn trim_children(&mut self, element: &str, keep: usize, from_start: bool) -> Result<()> {
        self.run_ops(
            &[DomOp::TrimChildren {
                element: element.into(),
                keep,
                from_start,
            }],
            false,
        )
    }
    /// `dialog.show()` / `dialog.showModal()`: a modal dialog enters the top layer, makes the rest
    /// of the document inert, and focuses its autofocus or first focusable element; Escape
    /// requests a close (`cancel`, then `close`) and focus returns to the previous element.
    pub fn show_dialog(&mut self, element: &str, modal: bool) -> Result<()> {
        let n = self.node(element)?;
        if !crate::interaction::dialog::is_dialog(self.pipeline.document(), n) {
            return Err(arg("show_dialog requires a dialog element"));
        }
        let id = self.id();
        crate::interaction::dialog::show(self, id, n, modal)?;
        self.repaint = true;
        Ok(())
    }
    /// `dialog.close(returnValue)`; nothing when the dialog is not open.
    pub fn close_dialog(&mut self, element: &str, return_value: Option<&str>) -> Result<()> {
        if let Some(v) = return_value {
            check_text(v)?;
        }
        let n = self.node(element)?;
        if !crate::interaction::dialog::is_dialog(self.pipeline.document(), n) {
            return Err(arg("close_dialog requires a dialog element"));
        }
        let id = self.id();
        crate::interaction::dialog::close(self, id, n, return_value)?;
        self.repaint = true;
        Ok(())
    }
    /// `dialog.returnValue` and whether the dialog is open and modal.
    pub fn dialog_state(&self, element: &str) -> Result<(bool, bool, String)> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        if !crate::interaction::dialog::is_dialog(d, n) {
            return Err(arg("dialog_state requires a dialog element"));
        }
        let open = d.has_attribute(n, "open")?;
        let modal = d
            .ui_flags(n)
            .is_ok_and(|f| f.contains(ui4me_document::ui_state::UiFlags::MODAL));
        Ok((
            open,
            modal,
            self.interaction.dialog_return_value(self.id(), n),
        ))
    }
    /// `showPopover()` on an element with a `popover` attribute; no-op when it is already showing.
    pub fn show_popover(&mut self, element: &str) -> Result<()> {
        let n = self.node(element)?;
        let id = self.id();
        crate::interaction::popover::show(self, id, n, None)?;
        self.repaint = true;
        Ok(())
    }
    /// `hidePopover()`; nothing when it is not showing. Hiding an auto popover hides those above it.
    pub fn hide_popover(&mut self, element: &str) -> Result<()> {
        let n = self.node(element)?;
        let id = self.id();
        crate::interaction::popover::hide(self, id, n)?;
        self.repaint = true;
        Ok(())
    }
    /// `togglePopover()`: whether the popover is showing afterwards.
    pub fn toggle_popover(&mut self, element: &str) -> Result<bool> {
        let n = self.node(element)?;
        let id = self.id();
        let open = crate::interaction::popover::toggle(self, id, n, None)?;
        self.repaint = true;
        Ok(open)
    }
    /// `(open, kind)` of a popover; `kind` is `"auto"` or `"manual"`.
    pub fn popover_state(&self, element: &str) -> Result<(bool, &'static str)> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        let kind = crate::interaction::popover::popover_kind(d, n)
            .ok_or_else(|| arg("popover_state requires an element with a popover attribute"))?;
        Ok((crate::interaction::popover::is_open(d, n), kind.name()))
    }
    /// Options of a `select` in order: (value, label, selected, disabled).
    pub fn select_options(&self, element: &str) -> Result<Vec<(String, String, bool, bool)>> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        if d.local_name(n)? != "select" {
            return Err(arg("select_options requires a select element"));
        }
        Ok(d.select_options(n)
            .into_iter()
            .map(|o| {
                (
                    d.option_value(o),
                    d.option_label(o),
                    d.option_selected(o),
                    d.option_disabled(o),
                )
            })
            .collect())
    }
    /// Select exactly the options whose values are in `values` (one at most for a single select).
    pub fn set_selected_values(&mut self, element: &str, values: &[String]) -> Result<()> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        if d.local_name(n)? != "select" {
            return Err(arg("set_selected_values requires a select element"));
        }
        if !d.select_is_multiple(n) && values.len() > 1 {
            return Err(arg("A single select takes at most one value"));
        }
        let options = d.select_options(n);
        for v in values {
            if !options.iter().any(|&o| &d.option_value(o) == v) {
                return Err(arg(format!("No option with value {v:?}")));
            }
        }
        self.pipeline.mutate(|d| {
            if values.is_empty() {
                return d.select_set_selected_index(n, -1);
            }
            for &o in &options {
                d.option_set_selected(o, values.contains(&d.option_value(o)))?;
            }
            Ok(())
        })?;
        self.after_mutation(false);
        Ok(())
    }
    /// Literal text content of `element` (not a control's value).
    pub fn get_text(&self, element: &str) -> Result<String> {
        let n = self.node(element)?;
        Ok(self.pipeline.text_content(n)?)
    }
    pub fn get_attribute(&self, element: &str, name: &str) -> Result<Option<String>> {
        let n = self.node(element)?;
        Ok(self
            .pipeline
            .document()
            .get_attribute(n, &name.to_ascii_lowercase())?
            .map(str::to_owned))
    }
    /// All attributes in source order as `(name, value)`.
    pub fn get_attributes(&self, element: &str) -> Result<Vec<(String, String)>> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        Ok(d.attributes(n)?
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect())
    }
    pub fn has_element(&self, element: &str) -> bool {
        self.pipeline.element_by_id(element).is_some()
    }
    /// IDs of identified elements matching a CSS selector list, in tree order (at most `limit`),
    /// plus the total number of matches including unidentified elements.
    pub fn query_ids(&self, selector: &str, limit: usize) -> Result<(Vec<String>, usize)> {
        if selector.len() > 4096 {
            return Err(arg("Selector is too long"));
        }
        let service = crate::style::query::SelectorService::new();
        let list = service
            .parse(selector)
            .map_err(|e| arg(format!("Invalid selector: {selector} ({:?})", e.sub_reason)))?;
        let d = self.pipeline.document();
        let matches = service.query_all(d, &list, d.document_node())?;
        let ids = matches
            .iter()
            .filter_map(|&m| d.get_attribute(m, "id").ok().flatten())
            .filter(|id| !id.is_empty())
            .take(limit)
            .map(str::to_owned)
            .collect();
        Ok((ids, matches.len()))
    }
    fn after_mutation(&mut self, subtree: bool) {
        self.invalidate_clipboard_context();
        if !self.input_files.is_empty() {
            self.prune_input_files();
        }
        let id = self.id();
        self.interaction.note_doc(id);
        dispatch::operation(self, |_| {});
        {
            let interaction = &mut self.interaction;
            let _ = self.pipeline.mutate(|document| {
                interaction.drop_dead_pointer_targets(document);
                Ok(())
            });
        }
        if subtree {
            let d = self.pipeline.document();
            let live = d
                .arena()
                .iter()
                .map(|n| n.id())
                .filter(|&n| d.arena().is_connected(n))
                .collect();
            lock(&self.resources).retain_nodes(id, &live);
        }
        if self
            .blink
            .as_ref()
            .is_some_and(|b| Some(b.node) != self.interaction.focused_element(id))
        {
            self.blink = None;
            self.ime = None;
        }
        if self.hosted {
            self.refresh_ime_eligibility();
        }
        self.repaint = true;
    }
    /// Constraint validity of the control `element` (F0605): `{valid, flags, message}`, or
    /// `{will_validate:false, valid:true}` for a control barred from validation. No events.
    pub fn get_validity(&mut self, element: &str) -> Result<Value> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        Ok(match d.constraint_validity(n) {
            Some(cv) => {
                let mut v = validity_json(&cv);
                v["will_validate"] = json!(true);
                v
            }
            None => json!({"valid": true, "will_validate": false, "flags": {}, "message": ""}),
        })
    }
    /// `checkValidity()` on a form or a control (F0605): fires a non-bubbling `invalid` at every
    /// invalid control, returns `{valid, invalid:[ids], count}` (controls without an `id` are
    /// counted but not listed). Never submits and never moves focus.
    pub fn check_validity(&mut self, element: &str) -> Result<Value> {
        let n = self.node(element)?;
        let id = self.id();
        let d = self.pipeline.document();
        let ok_kind = d.local_name(n).is_ok_and(|t| t == "form") || d.control_facts(n).is_some();
        if !ok_kind {
            return Err(arg("check_validity needs a form or a form control"));
        }
        let (fired, _) = crate::interaction::activation::fire_invalid_events(self, id, n);
        let d = self.pipeline.document();
        let ids: Vec<String> = fired
            .iter()
            .filter_map(|&c| d.get_attribute(c, "id").ok().flatten())
            .filter(|i| !i.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(json!({"valid": fired.is_empty(), "invalid": ids, "count": fired.len()}))
    }
    /// `setPointerCapture`/`releasePointerCapture` for the mouse pointer (`pointer_id` 1; 0.8
    /// F0807). Capture needs a pressed button and takes effect at the pointer's next event; it
    /// ends at the release, on `release`, or when the element is removed. Returns whether the
    /// request was accepted (false: no button is pressed).
    pub fn set_pointer_capture(
        &mut self,
        element: &str,
        pointer_id: u32,
        capture: bool,
    ) -> Result<bool> {
        let n = self.node(element)?;
        if pointer_id != 1 {
            return Err(Error {
                code: 3,
                message: format!("Unknown pointer {pointer_id} (the mouse is pointer 1)"),
            });
        }
        Ok(self
            .interaction
            .request_pointer_capture(self.id(), n, capture))
    }
    /// The id of the element holding the mouse pointer's capture.
    pub fn pointer_capture(&self) -> Option<String> {
        let n = self.interaction.pointer_capture(self.id())?;
        self.pipeline
            .document()
            .get_attribute(n, "id")
            .ok()
            .flatten()
            .map(str::to_owned)
    }
    pub fn focus(&mut self, element: &str) -> Result<()> {
        self.invalidate_clipboard_context();
        let n = self.node(element)?;
        let id = self.id();
        crate::interaction::focus::focus(self, id, n)?;
        Ok(())
    }
    pub fn render(&mut self, width: u32, height: u32, scale: f64) -> Result<Vec<u32>> {
        let count = framebuffer_length(width, height, scale)?;
        let mut pixels = vec![0; count];
        self.render_into(width, height, scale, &mut pixels)?;
        Ok(pixels)
    }
    /// Render directly into a caller-owned native surface (`0x00RRGGBB`). The destination must
    /// have exactly width * height pixels. Invalid dimensions/length leave it untouched, and
    /// the frame is completed before this call returns; no surface reference is retained.
    pub fn render_into(
        &mut self,
        width: u32,
        height: u32,
        scale: f64,
        pixels: &mut [u32],
    ) -> Result<()> {
        let count = framebuffer_length(width, height, scale)?;
        if pixels.len() != count {
            return Err(arg("Framebuffer slice length must equal width * height"));
        }
        self.poll_resources()?;
        self.pipeline.set_device_viewport(width, height, scale)?;
        self.apply_follow()?;
        let frame = self.pipeline.render_headless()?;
        if frame.pixels.width != width || frame.pixels.height != height {
            return Err(Error {
                code: 6,
                message: format!(
                    "Renderer surface mismatch: {}x{} versus {width}x{height}",
                    frame.pixels.width, frame.pixels.height
                ),
            });
        }
        if frame.pixels.data.len() != count * 4 {
            return Err(Error {
                code: 6,
                message: "Renderer pixel data length does not match surface dimensions".into(),
            });
        }
        for (pixel, rgba) in pixels.iter_mut().zip(frame.pixels.data.as_chunks::<4>().0) {
            let a = u32::from(rgba[3]);
            let channel = |v: u8| match frame.pixels.format {
                PixelFormat::Rgba8Premultiplied => (u32::from(v) + 255 - a).min(255),
                PixelFormat::Rgba8Unpremultiplied => {
                    (u32::from(v) * a + 255 * (255 - a) + 127) / 255
                }
            };
            *pixel = (channel(rgba[0]) << 16) | (channel(rgba[1]) << 8) | channel(rgba[2]);
        }
        frame.recycle();
        self.paint_selection(pixels, width, height, scale)?;
        self.paint_editing_caret(pixels, width, height, scale)?;
        if self.hosted {
            // Geometry refresh belongs to the frame boundary. Doing it after
            // each DOM write forces layout for every streamed log/chat token.
            let id = self.id();
            crate::interaction::editing::ime_enabled(self, id);
        }
        self.repaint = false;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn resources_weak_for_testing(&self) -> std::sync::Weak<Mutex<ResourceManager>> {
        Arc::downgrade(&self.resources)
    }
    #[cfg(test)]
    pub(crate) fn pipeline_mut_for_testing(&mut self) -> &mut DocumentPipeline {
        &mut self.pipeline
    }
    pub fn has_pending_resources(&self) -> bool {
        lock(&self.resources).pending(self.id()).unwrap_or(0) != 0
    }
    pub fn next_wake_in(&self) -> Option<Duration> {
        let resources = self
            .has_pending_resources()
            .then_some(Duration::from_millis(50));
        let blink = self
            .blink
            .as_ref()
            .filter(|_| self.window_focused)
            .map(|b| b.deadline.saturating_duration_since(Instant::now()));
        let anim = self
            .scroll_anims
            .active()
            .then_some(Duration::from_millis(16));
        let css = self
            .pipeline
            .next_animation_wake_ms()
            .map(|ms| Duration::from_secs_f64(ms.max(1.0) / 1000.0));
        [resources, blink, anim, css].into_iter().flatten().min()
    }
    pub fn poll_resources(&mut self) -> Result<bool> {
        let id = self.id();
        let batches = lock(&self.resources).process_completions();
        let mut changed = false;
        for batch in batches.into_iter().filter(|b| b.document == id) {
            changed |= self.pipeline.apply_resource_batch(&batch)?;
            if batch.font_epoch_changed {
                let faces = lock(&self.resources).document_faces(id)?;
                changed |= self.pipeline.apply_font_faces(faces)?;
            }
        }
        if self.blink.as_ref().is_some_and(|b| {
            !self
                .pipeline
                .document()
                .is_connected(b.node)
                .unwrap_or(false)
        }) {
            self.blink = None;
        }
        if let Some(b) = self.blink.as_mut().filter(|_| self.window_focused)
            && b.deadline <= Instant::now()
        {
            b.deadline = Instant::now() + BLINK;
            b.visible = !b.visible;
            self.pipeline
                .set_caret_state(b.visible, b.upstream, b.node)?;
            changed = true;
        }
        changed |= self.tick_scroll()?;
        changed |= self.tick_animations(false);
        self.repaint |= changed;
        Ok(changed)
    }
    pub fn diagnostics(&self) -> Vec<Value> {
        let mut values: Vec<Value> = self.errors.iter().rev().take(32).cloned().collect();
        let stores = self.pipeline.stores();
        for entry in stores.errors.iter().take(32) {
            let status = &entry.record.status;
            values.push(json!({"code":status.code.name(),"stage":status.stage.name(),"message":status.message}));
        }
        for entry in stores.unsupported.iter().take(32) {
            values.push(json!({"code":entry.code.name(),"stage":entry.key.stage.name(),"feature":entry.key.feature.as_str(),"reason":entry.reason.name()}));
        }
        for e in lock(&self.resources)
            .document_records(self.id())
            .into_iter()
            .rev()
            .take(32)
        {
            if e.outcome != Code::Ok {
                values.push(
                    json!({"code":e.outcome.name(),"stage":"resource","request":e.request.serial}),
                );
            }
        }
        values.truncate(128);
        values
    }
    pub fn handle_input(&mut self, input: Input) -> Result<Vec<Value>> {
        if matches!(input, Input::Text(_)) {
            if std::mem::take(&mut self.suppress_next_text) {
                return Ok(self.drain_events());
            }
        } else if !matches!(input, Input::Modifiers { .. }) {
            self.suppress_next_text = false;
        }
        if hosted::changes_clipboard_context(&input, self.pointer_buttons) {
            self.invalidate_clipboard_context();
        }
        if matches!(input, Input::Modifiers { .. }) {
            return self.handle_input_inner(input);
        }
        let follow = !self.follow.is_empty();
        if follow {
            self.apply_follow()?;
        }
        let probe = input.clone();
        if self.resize_input(&probe)? {
            return Ok(self.drain_events());
        }
        if self.drag_input(&probe) {
            // Pointer moves of a running drag (and the Escape that cancels it) stay out of the page.
            if follow {
                self.update_follow();
            }
            return Ok(self.drain_events());
        }
        let dragging = self.drag_owns_pointer();
        let result = self.handle_input_inner(input);
        if follow {
            self.update_follow();
        }
        let mut events = result?;
        if !dragging {
            self.selection_input(&probe, &events)?;
        }
        self.editable_pointer(&probe)?;
        self.editable_input(&probe, &events)?;
        events.extend(self.drain_events());
        Ok(events)
    }
    fn handle_input_inner(&mut self, input: Input) -> Result<Vec<Value>> {
        if let Input::Modifiers {
            shift,
            ctrl,
            alt,
            meta,
        } = input
        {
            self.modifiers = Modifiers {
                shift,
                ctrl,
                alt,
                meta,
            };
            return Ok(Vec::new());
        }
        let window_focus = match &input {
            Input::Focus(focused) => Some(*focused),
            _ => None,
        };
        if let Some(focused) = window_focus {
            self.window_focused = focused;
        }
        if window_focus == Some(false) {
            self.modifiers = Modifiers::default();
            self.pointer_buttons = 0;
        }
        let id = self.id();
        self.pipeline.flush()?;
        if let Some(node) = self.interaction.focused_element(id)
            && !self.focus_target_visible(id, node)?
        {
            crate::interaction::focus::blur(self, id, node)?;
        }
        if let Input::Text(text) = &input {
            check_text(text)?;
            if let Some(n) = self.interaction.focused_element(id) {
                let composing = crate::interaction::editing::ime::is_composing(self, id);
                let key = KeyData {
                    text: Some(text.clone()),
                    is_composing: composing,
                    ..KeyData::new("Unidentified", "Unidentified")
                };
                // The platform has already separated committed text from
                // shortcuts. AltGr often arrives with Ctrl+Alt set.
                let m = Modifiers {
                    shift: self.modifiers.shift,
                    ..Modifiers::default()
                };
                dispatch::operation(self, |h| {
                    crate::interaction::editing::keydown(
                        h,
                        id,
                        n,
                        &key,
                        m,
                        DispatchOrigin::Native,
                        None,
                    )
                })?;
            }
            return Ok(self.drain_events());
        }
        let kind = match input {
            Input::PointerMoved { x, y } => {
                finite_point(x, y)?;
                self.pointer = (x, y);
                let mut p = PointerData::mouse(x, y, -1);
                p.buttons = self.pointer_buttons;
                p.pressure = if p.buttons == 0 { 0.0 } else { 0.5 };
                InputKind::PointerMove(p)
            }
            Input::PointerButton {
                pressed,
                button,
                x,
                y,
            } => {
                finite_point(x, y)?;
                self.pointer = (x, y);
                let bit = ui4me_engine_types::input::button_bit(button);
                if pressed {
                    self.pointer_buttons |= bit;
                } else {
                    self.pointer_buttons &= !bit;
                }
                let mut p = PointerData::mouse(x, y, button);
                p.buttons = self.pointer_buttons;
                p.pressure = if p.buttons == 0 { 0.0 } else { 0.5 };
                if pressed {
                    InputKind::PointerDown(p)
                } else {
                    InputKind::PointerUp(p)
                }
            }
            Input::PointerLeft => {
                let mut p = PointerData::mouse(self.pointer.0, self.pointer.1, -1);
                p.buttons = self.pointer_buttons;
                InputKind::PointerLeave(p)
            }
            Input::Wheel { dx, dy, x, y } => {
                finite_point(dx, dy)?;
                finite_point(x, y)?;
                InputKind::Wheel {
                    x,
                    y,
                    delta_x: dx,
                    delta_y: dy,
                }
            }
            Input::Key {
                key,
                code,
                pressed,
                repeat,
                shift,
                ctrl,
                alt,
                meta,
            } => {
                self.modifiers = Modifiers {
                    shift,
                    ctrl,
                    alt,
                    meta,
                };
                let composing = crate::interaction::editing::ime::is_composing(self, id);
                let key = KeyData {
                    repeat,
                    is_composing: composing,
                    ..KeyData::new(key, code)
                };
                if pressed {
                    InputKind::KeyDown(key)
                } else {
                    InputKind::KeyUp(key)
                }
            }
            Input::Focus(focused) => InputKind::WindowFocus { focused },
            Input::ImePreedit { text, cursor } => {
                check_text(&text)?;
                let cursor = cursor.map(|(a, b)| {
                    (
                        utf8_to_utf16_offset(&text, a),
                        utf8_to_utf16_offset(&text, b),
                    )
                });
                InputKind::ImePreedit { text, cursor }
            }
            Input::ImeCommit(text) => {
                check_text(&text)?;
                InputKind::ImeCommit { text }
            }
            Input::ImeEnabled => InputKind::ImeEnabled,
            Input::ImeDisabled => InputKind::ImeDisabled,
            Input::Text(_) | Input::Modifiers { .. } => unreachable!(),
        };
        let mut item = InputItem::new(id, InputOrigin::Native, kind);
        item.modifiers = self.modifiers;
        self.interaction.queue_input(item);
        drain_input(self);
        if let Some(focused) = window_focus
            && let Some(blink) = &self.blink
        {
            self.set_caret_state(id, blink.node, focused, blink.upstream);
        }
        Ok(self.drain_events())
    }
}
impl DispatchHost for Document {
    fn with_runtime(&mut self, f: &mut dyn FnMut(&mut InteractionRuntime)) {
        f(&mut self.interaction);
    }
    fn with_engine(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut InteractionRuntime, &mut Model),
    ) -> std::result::Result<(), Status> {
        if id != self.id() {
            return Err(Status::new(Code::WrongDocument, Stage::Host));
        }
        let interaction = &mut self.interaction;
        self.pipeline.mutate(|document| {
            f(interaction, document);
            Ok(())
        })
    }
    fn invoke(&mut self, inv: &Invocation) -> std::result::Result<(), dispatch::CallbackError> {
        self.apply_prevent_rules(inv.event, &inv.event_type);
        let event = self
            .interaction
            .event(inv.event)
            .map_err(|e| dispatch::CallbackError::new(e.message))?;
        if inv.event_type == "keydown" && event.flags.canceled {
            self.suppress_next_text = true;
        }
        let node = event.target;
        let d = self.pipeline.document();
        let raw = node.raw();
        let path = self
            .interaction
            .event_path(inv.event)
            .map_err(|e| dispatch::CallbackError::new(e.message))?;
        let identified: Vec<(NodeId, String)> = path
            .into_iter()
            .filter_map(|n| {
                d.get_attribute(n, "id")
                    .ok()
                    .flatten()
                    .filter(|id| !id.is_empty())
                    .map(|id| (n, id.to_owned()))
            })
            .collect();
        let (element, element_name) = identified.first().cloned().unwrap_or((node, String::new()));
        let eraw = element.raw();
        let route: Vec<&str> = identified
            .iter()
            .take(if event.init.bubbles { usize::MAX } else { 1 })
            .map(|(_, id)| id.as_str())
            .collect();
        let event_type = if inv.event_type == "change" && !self.hosted {
            "commit"
        } else {
            &inv.event_type
        };
        let mut value = json!({"type":event_type,"element":element_name,"path":route,
            "_target":[raw.index,raw.generation.get()],"_element":[eraw.index,eraw.generation.get()]});
        value["node"] = json!(public_dom::node_ref(self.id(), node));
        value["detail"] = json!(event.init.detail);
        if let Some(related) = event.init.related_target {
            value["related_node"] = json!(public_dom::node_ref(self.id(), related));
            value["related_element"] = json!(d.get_attribute(related, "id").ok().flatten());
        }
        if let Some(control) = d.control_state(node).ok().flatten() {
            if let Some(text) = d.control_value(node) {
                value["value"] = json!(text);
            }
            if let Some(checked) = control.checkedness {
                value["checked"] = json!(checked);
            }
            if let Some(selection) = control.selection {
                value["selection"] = json!({"start":selection.start,"end":selection.end,"direction":match selection.direction {
                    ui4me_document::control_state::SelectionDirection::Forward=>"forward",
                    ui4me_document::control_state::SelectionDirection::Backward=>"backward",
                    ui4me_document::control_state::SelectionDirection::None=>"none"
                }});
            }
        }
        if d.local_name(node).is_ok_and(|t| t == "select") {
            value["value"] = json!(d.select_value(node));
        }
        if let Some(files) = self.input_files.get(&node) {
            value["files"] = json!(files);
        }
        let data = &event.init.data;
        macro_rules! field {
            ($name:literal,$field:ident) => {
                if let Some(v) = &data.$field {
                    value[$name] = json!(v);
                }
            };
        }
        field!("key", key);
        field!("code", code);
        field!("data", data);
        field!("button", button);
        field!("x", client_x);
        field!("y", client_y);
        field!("delta_x", delta_x);
        field!("delta_y", delta_y);
        field!("repeat", repeat);
        field!("input_type", input_type);
        field!("is_composing", is_composing);
        field!("old_state", old_state);
        field!("new_state", new_state);
        field!("submitter", submitter);
        field!("pointer_id", pointer_id);
        field!("pointer_type", pointer_type);
        field!("is_primary", is_primary);
        field!("pressure", pressure);
        field!("property_name", property_name);
        field!("animation_name", animation_name);
        field!("elapsed_time", elapsed_time);
        field!("pseudo_element", pseudo_element);
        if data.client_x.is_some() {
            value["buttons"] = json!(event.init.buttons);
        }
        value["cancelable"] = json!(event.init.cancelable);
        if inv.event_type == "invalid"
            && let Some(cv) = d.constraint_validity(node)
        {
            value["validity"] = validity_json(&cv);
        }
        if inv.event_type == "close" && crate::interaction::dialog::is_dialog(d, node) {
            value["return_value"] = json!(self.interaction.dialog_return_value(self.id(), node));
        }
        let m = event.init.modifiers;
        if event.init.cancelable {
            value["default_prevented"] = json!(event.flags.canceled);
        }
        value["shift"] = json!(m.shift);
        value["ctrl"] = json!(m.ctrl);
        value["alt"] = json!(m.alt);
        value["meta"] = json!(m.meta);
        let needed = if inv.event_type == "input" && !self.hosted {
            2
        } else {
            1
        };
        if self.events.len() + needed > MAX_EVENTS {
            return Err(dispatch::CallbackError::new("Document event queue is full"));
        }
        self.events.push(value.clone());
        // The hosted profile uses canonical HTML events. Only the internal
        // donor regression profile retains its historical aliases.
        if inv.event_type == "input" && !self.hosted {
            value["type"] = json!("change");
            self.events.push(value);
        }
        Ok(())
    }
    fn record_error(&mut self, _id: DocumentId, status: Status) -> Status {
        if self.errors.len() == 64 {
            self.errors.pop_front();
        }
        self.errors
            .push_back(json!({"code":status.code.name(),"message":status.message.clone()}));
        status
    }
    fn hit_test(
        &mut self,
        _id: DocumentId,
        x: f64,
        y: f64,
    ) -> std::result::Result<Option<NodeId>, Status> {
        let read = self.pipeline.flush()?;
        let hit = read.committed.scene.hit_test(
            ViewportPoint { x, y },
            HitOptions {
                all: true,
                include_scrollbars: false,
            },
        );
        let candidates: Vec<NodeId> = hit
            .hits
            .iter()
            .map(|h| h.element)
            .filter(|&n| !self.pipeline.document().is_inert(n))
            .collect();
        Ok(candidates
            .into_iter()
            .find_map(|n| self.inline_svg_target(n, x, y)))
    }
    fn scrollbar_hit(
        &mut self,
        _id: DocumentId,
        x: f64,
        y: f64,
    ) -> std::result::Result<Option<crate::interaction::scroll::ScrollbarHit>, Status> {
        let read = self.pipeline.flush()?;
        let Some(hit) = read.committed.scene.scrollbar_hit(ViewportPoint { x, y }) else {
            return Ok(None);
        };
        let d = self.pipeline.document();
        if d.is_inert(hit.container) {
            return Ok(None);
        }
        Ok(Some(crate::interaction::scroll::ScrollbarHit {
            node: hit.container,
            viewport: d.document_element() == Some(hit.container),
            vertical: hit.axis == ScrollAxis::Vertical,
            local: hit.local,
            track_length: hit.track_length,
            thumb_start: hit.thumb_start,
            thumb_length: hit.thumb_length,
        }))
    }
    fn scrollbar_local(
        &mut self,
        _id: DocumentId,
        node: NodeId,
        vertical: bool,
        x: f64,
        y: f64,
    ) -> std::result::Result<Option<f64>, Status> {
        let read = self.pipeline.flush()?;
        let axis = if vertical {
            ScrollAxis::Vertical
        } else {
            ScrollAxis::Horizontal
        };
        Ok(read
            .committed
            .scene
            .scrollbar_local(node, axis, ViewportPoint { x, y }))
    }
    fn now_ms(&mut self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
    fn run_deferred_closes(&mut self) {}
    fn outermost_commit(&mut self, _id: DocumentId) {
        self.pipeline.outermost_commit();
    }
    fn release_callbacks(&mut self, _released: &[Released]) {}
    fn request_frame(&mut self, _id: DocumentId) {
        self.repaint = true;
    }
    fn computed_cursor(
        &mut self,
        _id: DocumentId,
        node: NodeId,
    ) -> std::result::Result<String, Status> {
        self.pipeline.flush()?;
        self.pipeline
            .style_bridge()
            .computed_value(self.pipeline.document(), node, "cursor")
    }
    fn set_cursor(&mut self, _id: DocumentId, keyword: &str) {
        self.cursor = keyword.to_owned();
    }
    fn focus_target_visible(
        &mut self,
        _id: DocumentId,
        node: NodeId,
    ) -> std::result::Result<bool, Status> {
        let read = self.pipeline.flush()?;
        let Some(style) = read.committed.style.view(node) else {
            return Ok(false);
        };
        let flow = style.display_flow();
        Ok(!flow.display.is_none()
            && flow.visibility == crate::style::view::groups::Visibility::Visible
            && read
                .committed
                .layout
                .fragments
                .element_border_box(node)
                .is_some())
    }
    fn control_geometry(
        &mut self,
        _id: DocumentId,
        node: NodeId,
    ) -> std::result::Result<Option<crate::interaction::editing::EditGeometry>, Status> {
        let read = self.pipeline.flush()?;
        let tree = &read.committed.layout.fragments;
        let Some((frag, bf)) = tree.box_fragment(node) else {
            return Ok(None);
        };
        let Some(inner) = bf.extras.control_inner.clone() else {
            return Ok(None);
        };
        let origin = tree.document_rect(frag.id).rect.origin;
        let (mut sx, mut sy) = (0.0, 0.0);
        for e in self.pipeline.scroll_chain(node) {
            if e.target != ScrollTarget::Node(node) {
                sx += e.offset.0;
                sy += e.offset.1;
            }
        }
        let scroll = self
            .pipeline
            .scroll_offsets()
            .offset(ScrollTarget::Node(node));
        Ok(Some(crate::interaction::editing::EditGeometry {
            origin: (
                origin.x.to_f64() + inner.viewport.origin.x.to_f64() - sx,
                origin.y.to_f64() + inner.viewport.origin.y.to_f64() - sy,
            ),
            inner: *inner,
            scroll,
        }))
    }
    fn element_content_rect(
        &mut self,
        _id: DocumentId,
        node: NodeId,
    ) -> std::result::Result<Option<(f64, f64, f64, f64)>, Status> {
        let read = self.pipeline.flush()?;
        let tree = &read.committed.layout.fragments;
        let Some((frag, bf)) = tree.box_fragment(node) else {
            return Ok(None);
        };
        let rect = tree.document_rect(frag.id).rect;
        let (mut sx, mut sy) = (0.0, 0.0);
        for e in self.pipeline.scroll_chain(node) {
            if e.target != ScrollTarget::Node(node) {
                sx += e.offset.0;
                sy += e.offset.1;
            }
        }
        let c = bf.content_rect;
        Ok(Some((
            rect.origin.x.to_f64() + c.origin.x.to_f64() - sx,
            rect.origin.y.to_f64() + c.origin.y.to_f64() - sy,
            c.size.width.to_f64(),
            c.size.height.to_f64(),
        )))
    }
    fn set_caret_state(&mut self, _id: DocumentId, node: NodeId, visible: bool, upstream: bool) {
        let visible = visible && self.window_focused;
        if self
            .pipeline
            .set_caret_state(visible, upstream, node)
            .is_ok()
        {
            self.repaint = true;
        }
        if let Some(b) = self.blink.as_mut().filter(|b| b.node == node) {
            b.visible = visible;
            b.upstream = upstream;
            if visible {
                b.deadline = Instant::now() + BLINK;
            }
        }
    }
    fn caret_blink(&mut self, _id: DocumentId, node: Option<NodeId>) {
        self.blink = node.map(|node| CaretBlink {
            node,
            deadline: Instant::now() + BLINK,
            visible: self.window_focused,
            upstream: false,
        });
    }
    fn clipboard_read(&mut self) -> std::result::Result<ClipboardData, Status> {
        let source = self
            .native_clipboard
            .completion
            .take()
            .unwrap_or_else(|| self.clipboard.read_text());
        let plain = source.map_err(|message| {
            Status::new(Code::ResourceDenied, Stage::Platform).with_message(message)
        })?;
        let data = ClipboardData { plain, html: None };
        data.check_size()?;
        Ok(data)
    }
    fn clipboard_write(&mut self, data: &ClipboardData) -> std::result::Result<(), Status> {
        data.check_size()?;
        self.clipboard
            .write_text(data.plain.as_deref().unwrap_or(""))
            .map_err(|message| {
                Status::new(Code::ResourceDenied, Stage::Platform).with_message(message)
            })
    }
    fn defer_clipboard_paste(&mut self, doc: DocumentId, node: NodeId) -> bool {
        self.defer_native_paste(doc, node)
    }
    fn defer_clipboard_write(
        &mut self,
        doc: DocumentId,
        node: Option<NodeId>,
        data: &ClipboardData,
        cut: bool,
    ) -> bool {
        self.defer_native_write(doc, node, data, cut)
    }
    fn ime_update(
        &mut self,
        _id: DocumentId,
        request: Option<ui4me_engine_types::ime::ImeRequest>,
    ) {
        self.ime = request.map(|r| ImeState {
            password: r.purpose == ui4me_engine_types::ime::ImePurposeKind::Password,
            cursor_area: r.cursor_area,
        });
    }
    fn form_reset(&mut self, _id: DocumentId, form: NodeId) {
        // A reset empties the file inputs of the form (HTML reset algorithm).
        let controls = self
            .pipeline
            .document()
            .form_elements(form)
            .unwrap_or_default();
        let id = self.id();
        for n in controls {
            self.input_files.remove(&n);
            let _ = self.pipeline.set_file_input_names(n, &[]);
            if let Some(value) = self.pipeline.document().control_value(n)
                && self.interaction.host_value_written(id, n, &value)
            {
                self.ime_resets += 1;
            }
        }
    }
    fn file_input_activated(&mut self, _id: DocumentId, input: NodeId) {
        // One picker per input at a time; the host opens it after the dispatch (0.6 F0606).
        if !self.file_requests.contains(&input) && self.file_requests.len() < 8 {
            self.file_requests.push(input);
        }
    }
    fn link_activated(&mut self, _id: DocumentId, link: NodeId) {
        // Reported after the dispatch as `link_activated` (0.6 F0608); the host decides.
        let d = self.pipeline.document();
        let href = d
            .get_attribute(link, "href")
            .ok()
            .flatten()
            .unwrap_or("")
            .to_owned();
        let element = d
            .get_attribute(link, "id")
            .ok()
            .flatten()
            .unwrap_or("")
            .to_owned();
        let base = url::Url::parse(d.url()).ok();
        let resolved = url::Url::options()
            .base_url(base.as_ref().filter(|b| b.scheme() != "about"))
            .parse(href.trim())
            .ok()
            .map(|u| u.to_string());
        if self.events.len() < MAX_EVENTS {
            self.events.push(json!({"type":"link_activated","element":element,"href":href,"url":resolved,"node":crate::document::public_dom::node_ref(self.id(), link)}));
        }
    }
    fn ime_reset(&mut self, _id: DocumentId) {
        // Clearing the request alone is not enough: a following `ime_update` for the next focused
        // control would restore an equal request and the platform would never hear of the reset.
        self.ime = None;
        self.ime_resets += 1;
    }
    fn scroll_chain(
        &mut self,
        _id: DocumentId,
        from: NodeId,
    ) -> std::result::Result<Vec<ScrollTargetInfo>, Status> {
        self.pipeline.flush()?;
        Ok(self
            .pipeline
            .scroll_chain(from)
            .iter()
            .map(|e| self.scroll_info_of(e))
            .collect())
    }
    fn scroll_info(
        &mut self,
        _id: DocumentId,
        target: Option<NodeId>,
    ) -> std::result::Result<Option<ScrollTargetInfo>, Status> {
        self.pipeline.flush()?;
        Ok(self
            .pipeline
            .scroll_entry(scroll_target(target))
            .as_ref()
            .map(|e| self.scroll_info_of(e)))
    }
    fn scroll_user_by(
        &mut self,
        _id: DocumentId,
        target: Option<NodeId>,
        dx: f64,
        dy: f64,
    ) -> std::result::Result<ScrollApplied, Status> {
        self.scroll_user_move(target, dx, dy)
    }
    fn scroll_cancel(
        &mut self,
        _id: DocumentId,
        target: Option<NodeId>,
    ) -> std::result::Result<(), Status> {
        self.scroll_anims.cancel(target);
        Ok(())
    }
    fn scroll_user_settle(
        &mut self,
        _id: DocumentId,
        target: Option<NodeId>,
    ) -> std::result::Result<bool, Status> {
        self.scroll_user_settle_at(target)
    }
    fn scroll_set(
        &mut self,
        _id: DocumentId,
        target: Option<NodeId>,
        x: f64,
        y: f64,
    ) -> std::result::Result<ScrollApplied, Status> {
        self.pipeline.flush()?;
        match self.pipeline.scroll_to(scroll_target(target), x, y) {
            Ok(a) => {
                self.repaint |= a.changed;
                Ok(ScrollApplied {
                    offset: a.offset,
                    changed: a.changed,
                    clamped: a.clamped,
                })
            }
            Err(ScrollError::NotScrollContainer) => Ok(ScrollApplied {
                offset: (0.0, 0.0),
                changed: false,
                clamped: false,
            }),
            Err(ScrollError::NonFinite) => Err(invalid("Non-finite scroll offset")),
        }
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        let id = self.id();
        self.interaction.detach_document(id);
        let mut r = lock(&self.resources);
        r.set_wake(None);
        r.close_document(id);
        r.release_document(id);
        r.shutdown();
    }
}
fn finite_point(x: f64, y: f64) -> Result<()> {
    if x.is_finite() && y.is_finite() {
        Ok(())
    } else {
        Err(arg("Input coordinates must be finite"))
    }
}
fn framebuffer_length(width: u32, height: u32, scale: f64) -> Result<usize> {
    let count = (width as usize).checked_mul(height as usize);
    if width == 0 || height == 0 || count.is_none_or(|n| n > MAX_PIXELS) {
        return Err(limit_error("Framebuffer dimensions exceed pixel budget"));
    }
    if !scale.is_finite() || scale <= 0.0 {
        return Err(arg("Scale must be finite and positive"));
    }
    Ok(count.expect("dimensions were validated"))
}
fn check_text(text: &str) -> Result<()> {
    if text.len() > MAX_TEXT {
        Err(Error {
            code: 5,
            message: "Text exceeds 16 KiB".into(),
        })
    } else if text.contains('\0') {
        Err(arg("NUL is unsupported"))
    } else {
        Ok(())
    }
}
fn check_html(html: &str) -> Result<()> {
    if html.len() > MAX_HTML {
        Err(Error {
            code: 5,
            message: "HTML exceeds 1 MiB".into(),
        })
    } else if html.contains('\0') {
        Err(arg("NUL in HTML"))
    } else {
        Ok(())
    }
}
fn validate_id(id: &str) -> Result<()> {
    if id.len() > 256 || id.chars().any(char::is_whitespace) {
        Err(arg("Invalid public element ID"))
    } else {
        Ok(())
    }
}
fn validate_input_type(kind: InputType) -> std::result::Result<(), Status> {
    if matches!(
        kind,
        // `file` has the native picker since 0.6 (F0606).
        InputType::Color
            | InputType::Date
            | InputType::Time
            | InputType::Month
            | InputType::Week
            | InputType::DateTimeLocal
    ) {
        return Err(invalid(format!(
            "Input type '{}' requires a native picker and is unavailable in UI4ME Core",
            kind.as_str()
        )));
    }
    Ok(())
}
/// Validate the proposed connected tree before any fragment hooks or revision.
fn validate_document(
    d: &Model,
    replacing: Option<NodeId>,
    fragment: Option<NodeId>,
    hosted: bool,
) -> std::result::Result<(), Status> {
    let mut ids = BTreeSet::new();
    let mut count = 0;
    let mut replacement_depth = 0;
    let mut stack = vec![(d.document_node(), 0usize)];
    let mut phase = 0;
    loop {
        while let Some((n, depth)) = stack.pop() {
            count += 1;
            if count > MAX_NODES || depth > MAX_DEPTH {
                return Err(limit("Document node/depth limit exceeded"));
            }
            if let Ok(tag) = d.local_name(n) {
                if matches!(tag, "script" | "iframe" | "object" | "embed") {
                    return Err(invalid(format!("Unsupported active element: {tag}")));
                }
                if let Some(kind) = d.input_type(n) {
                    validate_input_type(kind)?;
                }
                for attr in d.attributes(n)? {
                    let name: &str = &attr.name.local;
                    if name.starts_with("on") || name == "srcdoc" {
                        return Err(invalid("Script-bearing attributes are unsupported"));
                    }
                    if hosted
                        && name == "contenteditable"
                        && !attr.value.trim().eq_ignore_ascii_case("false")
                    {
                        return Err(Status::new(Code::UnsupportedFeature, Stage::Host)
                            .with_message("contenteditable is outside the hosted desktop profile; use input or textarea"));
                    }
                }
                if let Some(id) = d.get_attribute(n, "id")?.filter(|id| !id.is_empty()) {
                    validate_id(id).map_err(|e| invalid(e.message))?;
                    if !ids.insert(id.to_owned()) {
                        return Err(invalid(format!("Duplicate element ID: {id}")));
                    }
                }
            }
            if phase == 0 && Some(n) == replacing {
                replacement_depth = depth + 1;
                continue;
            }
            for &child in d.children(n)? {
                stack.push((child, depth + 1));
            }
            if let Some(contents) = d.template_contents(n).ok().flatten() {
                stack.push((contents, depth + 1));
            }
        }
        if phase == 0 {
            phase = 1;
            if let Some(f) = fragment {
                stack.push((f, replacement_depth));
                continue;
            }
        }
        break;
    }
    Ok(())
}
/// Scroll state reported by [`Document::scroll_state`] (CSS px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    pub x: f64,
    pub y: f64,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub width: f64,
    pub height: f64,
    pub scrollable: bool,
    /// A smooth scroll of the container is in progress.
    pub animating: bool,
}
/// A rectangle in viewport CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
struct Follow {
    target: Option<NodeId>,
    pinned: bool,
}
/// CSSOM View scroll-into-view alignment: the scroll delta that aligns `[start, start+size)`
/// within the port `[port, port+port_size)`.
fn align(mode: &str, start: f64, size: f64, port: f64, port_size: f64) -> f64 {
    let end = start + size;
    let port_end = port + port_size;
    match mode {
        "start" => start - port,
        "end" => end - port_end,
        "center" => (start + size / 2.0) - (port + port_size / 2.0),
        _ => {
            if start < port {
                start - port
            } else if end > port_end {
                if size > port_size {
                    start - port
                } else {
                    end - port_end
                }
            } else {
                0.0
            }
        }
    }
}
/// Operations accepted by [`Document::apply_batch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomOp {
    SetText {
        element: String,
        text: String,
    },
    SetAttribute {
        element: String,
        name: String,
        value: Option<String>,
    },
    AddClass {
        element: String,
        name: String,
    },
    RemoveClass {
        element: String,
        name: String,
    },
    InsertHtml {
        element: String,
        position: String,
        html: String,
    },
    ReplaceChildrenHtml {
        element: String,
        html: String,
    },
    RemoveElement {
        element: String,
    },
    MoveElement {
        element: String,
        target: String,
        position: String,
    },
    /// Append literal text (a streamed chunk) at the end of an element.
    AppendText {
        element: String,
        text: String,
    },
    /// Remove element children beyond `keep`, oldest first (`from_start`) or newest first.
    TrimChildren {
        element: String,
        keep: usize,
        from_start: bool,
    },
}
impl DomOp {
    fn payload_len(&self) -> usize {
        match self {
            Self::SetText { text, .. } | Self::AppendText { text, .. } => text.len(),
            Self::SetAttribute { value, .. } => value.as_ref().map_or(0, String::len),
            Self::InsertHtml { html, .. } | Self::ReplaceChildrenHtml { html, .. } => html.len(),
            _ => 0,
        }
    }
    fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::SetText { .. }
                | Self::InsertHtml { .. }
                | Self::ReplaceChildrenHtml { .. }
                | Self::RemoveElement { .. }
                | Self::AppendText { .. }
                | Self::TrimChildren { .. }
        )
    }
}
/// A host default-action rule (see [`Document::set_default_action`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultActionRule {
    pub element: String,
    pub event: String,
    pub key: Option<String>,
    pub shift: Option<bool>,
    pub ctrl: Option<bool>,
    pub alt: Option<bool>,
    pub meta: Option<bool>,
}
struct StoredRule {
    node: NodeId,
    rule: DefaultActionRule,
}
impl StoredRule {
    fn same(&self, other: &StoredRule) -> bool {
        self.node == other.node
            && DefaultActionRule {
                element: String::new(),
                ..self.rule.clone()
            } == DefaultActionRule {
                element: String::new(),
                ..other.rule.clone()
            }
    }
}
const PREVENTABLE: &[&str] = &[
    "keydown",
    "click",
    "dblclick",
    "auxclick",
    "contextmenu",
    "beforeinput",
    "copy",
    "cut",
    "paste",
    "mousedown",
    "pointerdown",
    "wheel",
    "submit",
    "reset",
    "cancel",
    "beforetoggle",
];
const MAX_PREVENT_RULES: usize = 256;
const MAX_BATCH_OPS: usize = 1024;
const STREAM_NODE_BYTES: usize = 4096;
fn limit_error(message: impl Into<String>) -> Error {
    Error {
        code: 5,
        message: message.into(),
    }
}
fn model_node(d: &Model, element: &str) -> Result<NodeId> {
    d.element_by_id(element)
        .ok_or_else(|| arg(format!("Unknown element ID: {element}")))
}
fn check_movable(d: &Model, n: NodeId, what: &str) -> Result<()> {
    if Some(n) == d.document_element() || Some(n) == d.body() || Some(n) == d.head() {
        return Err(arg(format!("Cannot {what} the html, head or body element")));
    }
    Ok(())
}
fn apply_op(d: &mut Model, op: &DomOp, hosted: bool, nodes: bool) -> Result<()> {
    let resolve = |d: &Model, target: &str| {
        if nodes {
            public_dom::model_live_ref(d, target)
        } else {
            model_node(d, target)
        }
    };
    match op {
        DomOp::SetText { element, text } => {
            check_text(text)?;
            let n = resolve(d, element)?;
            if d.local_name(n)
                .is_ok_and(|tag| matches!(tag, "input" | "textarea" | "select"))
            {
                return Err(arg(
                    "Use set_value for form controls; it is not a batch operation",
                ));
            }
            d.set_text_content(n, text)?;
        }
        DomOp::AppendText { element, text } => {
            check_text(text)?;
            let n = resolve(d, element)?;
            if matches!(d.local_name(n)?, "input" | "textarea" | "select") {
                return Err(arg("append_text requires a non-control element"));
            }
            if text.is_empty() {
                return Ok(());
            }
            // Extend the last text node while it is small, so a long stream stays O(chunk).
            let last = d.children(n)?.last().copied();
            match last.filter(|&l| d.text_data(l).is_ok_and(|t| t.len() < STREAM_NODE_BYTES)) {
                Some(l) => d.append_data(l, text)?,
                None => {
                    let t = d.create_text_node(text)?;
                    d.append_child(n, t)?;
                }
            }
        }
        DomOp::TrimChildren {
            element,
            keep,
            from_start,
        } => {
            let n = resolve(d, element)?;
            let kids: Vec<NodeId> = d.children(n)?.to_vec();
            let elements: Vec<usize> = kids
                .iter()
                .enumerate()
                .filter(|&(_, &k)| {
                    d.node_kind(k)
                        .is_ok_and(|k| k == ui4me_document::node::NodeKind::Element)
                })
                .map(|(i, _)| i)
                .collect();
            if elements.len() > *keep {
                let excess = elements.len() - keep;
                let doomed: Vec<NodeId> = if *from_start {
                    kids[..=elements[excess - 1]].to_vec()
                } else {
                    kids[elements[elements.len() - excess]..].to_vec()
                };
                for k in doomed {
                    d.remove(k)?;
                }
            }
        }
        DomOp::SetAttribute {
            element,
            name,
            value,
        } => model_set_attribute(d, resolve(d, element)?, name, value.as_deref(), hosted)?,
        DomOp::AddClass { element, name } | DomOp::RemoveClass { element, name } => {
            if name.is_empty() || name.chars().any(char::is_whitespace) {
                return Err(arg("A class must be one nonempty token"));
            }
            let n = resolve(d, element)?;
            let mut classes: Vec<String> = d
                .get_attribute(n, "class")?
                .unwrap_or("")
                .split_whitespace()
                .filter(|s| s != name)
                .map(str::to_owned)
                .collect();
            if matches!(op, DomOp::AddClass { .. }) {
                classes.push(name.clone());
            }
            model_set_attribute(d, n, "class", Some(&classes.join(" ")), hosted)?;
        }
        DomOp::InsertHtml {
            element,
            position,
            html,
        } => {
            check_html(html)?;
            let n = resolve(d, element)?;
            let position = parse_position(position)?;
            if matches!(position, "beforebegin" | "afterend") {
                check_movable(d, n, "insert_html beside")?;
            }
            d.insert_adjacent_html_checked(n, position, html, |d, f| {
                validate_document(d, None, Some(f), hosted)
            })?;
        }
        DomOp::ReplaceChildrenHtml { element, html } => {
            check_html(html)?;
            let n = resolve(d, element)?;
            d.set_inner_html_checked(n, html, |d, f| {
                validate_document(d, Some(n), Some(f), hosted)
            })?;
        }
        DomOp::RemoveElement { element } => {
            let n = resolve(d, element)?;
            check_movable(d, n, "remove")?;
            d.remove(n)?;
        }
        DomOp::MoveElement {
            element,
            target,
            position,
        } => {
            let n = resolve(d, element)?;
            let t = resolve(d, target)?;
            let position = parse_position(position)?;
            check_movable(d, n, "move")?;
            if matches!(position, "beforebegin" | "afterend") {
                check_movable(d, t, "move beside")?;
            }
            if d.arena().is_inclusive_ancestor(n, t) {
                return Err(arg("Cannot move an element into itself or its descendant"));
            }
            let (parent, before) = match position {
                "beforebegin" => (
                    d.parent(t)?.ok_or_else(|| arg("Target has no parent"))?,
                    Some(t),
                ),
                "afterend" => {
                    let parent = d.parent(t)?.ok_or_else(|| arg("Target has no parent"))?;
                    let kids = d.children(parent)?;
                    let at = kids.iter().position(|&c| c == t).unwrap_or(kids.len());
                    (parent, kids.get(at + 1).copied())
                }
                "afterbegin" => (t, d.children(t)?.first().copied()),
                _ => (t, None),
            };
            if before != Some(n) {
                d.insert_before(parent, n, before)?;
            }
        }
    }
    Ok(())
}
fn model_set_attribute(
    d: &mut Model,
    n: NodeId,
    name: &str,
    value: Option<&str>,
    hosted: bool,
) -> Result<()> {
    let name = name.to_ascii_lowercase();
    if name.is_empty() || name.len() > 256 {
        return Err(arg("Attribute name must be 1..256 bytes"));
    }
    if name.starts_with("on") || name == "srcdoc" {
        return Err(arg("Script-bearing attributes are unsupported"));
    }
    if hosted
        && name == "contenteditable"
        && value.is_some_and(|v| !v.trim().eq_ignore_ascii_case("false"))
    {
        return Err(Status::new(Code::UnsupportedFeature, Stage::Host)
            .with_message(
                "contenteditable is outside the hosted desktop profile; use input or textarea",
            )
            .into());
    }
    if name == "type" && d.input_type(n).is_some() {
        validate_input_type(InputType::parse(value))?;
    }
    if let Some(value) = value {
        check_text(value)?;
        if name == "id" && !value.is_empty() {
            validate_id(value)?;
            if d.element_by_id(value).is_some_and(|other| other != n) {
                return Err(arg("Duplicate public element ID"));
            }
        }
        d.set_attribute(n, &name, value)?;
    } else {
        d.remove_attribute(n, &name)?;
    }
    Ok(())
}
fn parse_position(position: &str) -> Result<&'static str> {
    match position {
        "beforebegin" => Ok("beforebegin"),
        "afterbegin" => Ok("afterbegin"),
        "beforeend" => Ok("beforeend"),
        "afterend" => Ok("afterend"),
        _ => Err(arg(
            "position must be beforebegin, afterbegin, beforeend or afterend",
        )),
    }
}
fn scroll_target(n: Option<NodeId>) -> ScrollTarget {
    n.map_or(ScrollTarget::Viewport, ScrollTarget::Node)
}
fn scroll_info(e: &crate::pipeline::scroll::ScrollEntry) -> ScrollTargetInfo {
    ScrollTargetInfo {
        target: match e.target {
            ScrollTarget::Viewport => None,
            ScrollTarget::Node(n) => Some(n),
        },
        offset: e.offset,
        min: e.range.min,
        max: e.range.max,
        scrollport: e.scrollport,
        user_x: e.user_x,
        user_y: e.user_y,
        contain_x: false,
        contain_y: false,
    }
}

#[cfg(test)]
mod a11y_text_tests;
#[cfg(test)]
mod animation_tests;
mod application_api;
#[cfg(test)]
mod bidi_tests;
#[cfg(test)]
mod controls_tests;
pub use application_api::NodeOp;
#[cfg(test)]
mod application_api_tests;
mod css_animation;
mod hosted;
pub use hosted::{ClipboardRequest, ImeSurrounding};
mod drag;
pub(crate) mod editable;
pub mod file_input;
#[cfg(test)]
mod focus_pointer_tests;
#[cfg(test)]
mod framework_fixtures_tests;
#[cfg(test)]
mod layout_conformance_tests;
#[cfg(test)]
pub(crate) mod paged_facade;
#[cfg(test)]
mod paged_tests;
#[cfg(test)]
mod popover_tests;
pub mod public_dom;
mod resize;
pub mod rich_clipboard;
pub(crate) mod rich_edit;
#[cfg(test)]
mod rich_editing_tests;
mod scroll_anim;
#[cfg(test)]
mod scroll_tests;
mod selection;
mod svg_hit;
#[cfg(test)]
mod svg_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unicode_corpus_tests;
#[cfg(test)]
mod visual_tests;

/// Media preferences of a document (0.5 F0506): the OS signal and the app override are kept
/// apart, so a later OS change never undoes an app choice.
#[derive(Clone, Copy, Debug, Default)]
struct Environment {
    system_dark: Option<bool>,
    app_dark: Option<bool>,
    app_reduced_motion: Option<bool>,
    system_reduced_motion: Option<bool>,
    /// The OS high-contrast mode (reported; `forced-colors` is not evaluated).
    system_high_contrast: Option<bool>,
}

/// Style and layout answers for the accessibility builder, read from the committed state.
struct A11yInfo<'a> {
    doc: &'a Document,
    id: DocumentId,
    committed: Option<&'a crate::pipeline::publish::CommittedSet>,
}

impl A11yInfo<'_> {
    fn flow(&self, n: NodeId) -> Option<crate::style::view::groups::DisplayFlow> {
        Some(self.committed?.style.view(n)?.display_flow())
    }
}

impl crate::a11y::RenderInfo for A11yInfo<'_> {
    fn rendering(&self, element: NodeId) -> crate::a11y::Rendering {
        use crate::a11y::Rendering;
        use crate::style::view::groups::Visibility;
        let arena = self.doc.pipeline.document().arena();
        let mut at = Some(element);
        while let Some(x) = at {
            match self.flow(x) {
                Some(f) if f.display.is_none() => return Rendering::NotRendered,
                None if x == element => return Rendering::NotRendered,
                _ => {}
            }
            // The contents of a `content-visibility: hidden` element are skipped (0.9 F0903).
            if x != element
                && self
                    .committed
                    .and_then(|c| c.style.view(x))
                    .is_some_and(|v| v.containment().skips_contents)
            {
                return Rendering::NotRendered;
            }
            at = arena.get(x).and_then(|n| n.parent());
        }
        match self.flow(element) {
            Some(f) if f.visibility == Visibility::Visible => Rendering::Rendered,
            _ => Rendering::Invisible,
        }
    }
    fn bounds(&self, node: NodeId) -> Option<crate::a11y::Bounds> {
        self.doc
            .viewport_rect(node)
            .map(|(x, y, w, h)| crate::a11y::Bounds { x, y, w, h })
    }
    fn is_block(&self, element: NodeId) -> bool {
        use crate::style::view::groups::DisplayOutside;
        self.flow(element)
            .is_some_and(|f| f.display.outside == DisplayOutside::Block)
    }
    fn has_listeners(&self, element: NodeId) -> bool {
        self.doc.interaction.listener_count(self.id, element) > 0
    }
    fn text_lines(&self, field: NodeId) -> Option<Vec<crate::a11y::TextLine>> {
        let tree = &self.committed?.layout.fragments;
        let (frag, bf) = tree.box_fragment(field)?;
        let inner = bf.extras.control_inner.as_ref()?;
        if inner.select.is_some() {
            return None;
        }
        // Viewport position of the content box, as `control_geometry` computes it.
        let origin = tree.document_rect(frag.id).rect.origin;
        let (mut sx, mut sy) = (0.0, 0.0);
        for e in self.doc.pipeline.scroll_chain(field) {
            if e.target != ScrollTarget::Node(field) {
                sx += e.offset.0;
                sy += e.offset.1;
            }
        }
        let own = self
            .doc
            .pipeline
            .scroll_offsets()
            .offset(ScrollTarget::Node(field));
        let ox = origin.x.to_f64() + inner.viewport.origin.x.to_f64() - sx - own.0;
        let oy = origin.y.to_f64() + inner.viewport.origin.y.to_f64() - sy - own.1;
        let width = inner.viewport.size.width.to_f64();
        Some(
            inner
                .lines
                .iter()
                .map(|l| {
                    let clusters = if l.clusters.is_empty() {
                        // A left-to-right line: each pair of neighbouring caret stops is a character.
                        l.stops
                            .windows(2)
                            .filter(|w| w[1].0 > w[0].0)
                            .map(|w| {
                                let (a, b) = (f64::from(w[0].1), f64::from(w[1].1));
                                (w[0].0, w[1].0, ox + a.min(b), ox + a.max(b), false)
                            })
                            .collect()
                    } else {
                        l.clusters
                            .iter()
                            .map(|c| (c.0, c.1, ox + f64::from(c.2), ox + f64::from(c.3), c.4))
                            .collect()
                    };
                    crate::a11y::TextLine {
                        start: if inner.placeholder_shown { 0 } else { l.start },
                        end: if inner.placeholder_shown { 0 } else { l.end },
                        bounds: crate::a11y::Bounds {
                            x: ox,
                            y: oy + f64::from(l.top),
                            w: width,
                            h: f64::from(l.height),
                        },
                        clusters: if inner.placeholder_shown {
                            Vec::new()
                        } else {
                            clusters
                        },
                    }
                })
                .collect(),
        )
    }
}

fn validity_json(cv: &ui4me_document::constraint::ConstraintValidity) -> Value {
    let mut flags = serde_json::Map::new();
    for (name, bit) in ui4me_document::constraint::FLAG_NAMES {
        flags.insert(name.to_owned(), json!(cv.flags & bit != 0));
    }
    json!({"valid": cv.valid(), "flags": flags, "message": cv.message})
}
