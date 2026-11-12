//! Hosted application operations. Targets are HTML IDs or generation-scoped node references;
//! callbacks remain asynchronous, while native default actions finish in one dispatch operation.

use super::*;
use crate::interaction::editing;
use crate::interaction::frame::EventInit;
use ui4me_document::control_state::{Selection, SelectionDirection};

/// Atomic mutations addressed by stable DOM node references, including anonymous elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeOp {
    SetText {
        node: String,
        text: String,
    },
    SetAttribute {
        node: String,
        name: String,
        value: Option<String>,
    },
    AddClass {
        node: String,
        name: String,
    },
    RemoveClass {
        node: String,
        name: String,
    },
    InsertHtml {
        node: String,
        position: String,
        html: String,
    },
    ReplaceChildrenHtml {
        node: String,
        html: String,
    },
    RemoveElement {
        node: String,
    },
    MoveElement {
        node: String,
        target_node: String,
        position: String,
    },
    AppendText {
        node: String,
        text: String,
    },
    TrimChildren {
        node: String,
        keep: usize,
        from_start: bool,
    },
}

impl NodeOp {
    fn into_dom(self) -> DomOp {
        match self {
            Self::SetText { node, text } => DomOp::SetText {
                element: node,
                text,
            },
            Self::SetAttribute { node, name, value } => DomOp::SetAttribute {
                element: node,
                name,
                value,
            },
            Self::AddClass { node, name } => DomOp::AddClass {
                element: node,
                name,
            },
            Self::RemoveClass { node, name } => DomOp::RemoveClass {
                element: node,
                name,
            },
            Self::InsertHtml {
                node,
                position,
                html,
            } => DomOp::InsertHtml {
                element: node,
                position,
                html,
            },
            Self::ReplaceChildrenHtml { node, html } => DomOp::ReplaceChildrenHtml {
                element: node,
                html,
            },
            Self::RemoveElement { node } => DomOp::RemoveElement { element: node },
            Self::MoveElement {
                node,
                target_node,
                position,
            } => DomOp::MoveElement {
                element: node,
                target: target_node,
                position,
            },
            Self::AppendText { node, text } => DomOp::AppendText {
                element: node,
                text,
            },
            Self::TrimChildren {
                node,
                keep,
                from_start,
            } => DomOp::TrimChildren {
                element: node,
                keep,
                from_start,
            },
        }
    }
}

impl Document {
    pub fn register_resource_bundle(
        &mut self,
        name: &str,
        entries: Vec<ui4me_assets::bundles::BundleEntryInput>,
    ) -> Result<Value> {
        let resources = self
            .pipeline
            .resources
            .as_ref()
            .ok_or_else(|| arg("Document resources are unavailable"))?;
        let change = resources.register_bundle(name, entries)?;
        if change.affected.contains(&self.id()) {
            self.pipeline.apply_bundle_change()?;
            self.repaint = true;
        }
        Ok(json!({"name":name,"replaced":change.existed}))
    }
    pub fn unregister_resource_bundle(&mut self, name: &str) -> Result<Value> {
        let resources = self
            .pipeline
            .resources
            .as_ref()
            .ok_or_else(|| arg("Document resources are unavailable"))?;
        let change = resources.unregister_bundle(name)?;
        if change.affected.contains(&self.id()) {
            self.pipeline.apply_bundle_change()?;
            self.repaint = true;
        }
        Ok(json!({"name":name,"removed":change.existed}))
    }
    pub fn resource_stats(&self) -> Result<Value> {
        let stats = self
            .pipeline
            .resources
            .as_ref()
            .ok_or_else(|| arg("Document resources are unavailable"))?
            .resource_stats();
        Ok(
            json!({"cache":{"entries":stats.cache.entries,"bytes":stats.cache.bytes,"referenced_bytes":stats.cache.referenced_bytes,"hits":stats.cache.hits,"misses":stats.cache.misses,"evictions":stats.cache.evictions,"reserved":stats.cache.reserved},"bundles":{"count":stats.bundles.bundles,"entries":stats.bundles.entries,"bytes":stats.bundles.bytes},"svg_rasters":{"rasterisations":stats.svg_rasters.rasterisations,"hits":stats.svg_rasters.hits,"clamped":stats.svg_rasters.clamped,"bytes":stats.svg_rasters.bytes}}),
        )
    }
    pub fn purge_unused_resources(&mut self, url: Option<&str>) -> Result<Value> {
        let purged = self
            .pipeline
            .resources
            .as_ref()
            .ok_or_else(|| arg("Document resources are unavailable"))?
            .purge_unused_resources(url)?;
        Ok(json!({"purged":purged,"stats":self.resource_stats()?}))
    }

    pub fn apply_node_batch(&mut self, ops: &[NodeOp]) -> Result<()> {
        if ops.len() > MAX_BATCH_OPS {
            return Err(limit_error(format!(
                "A batch holds at most {MAX_BATCH_OPS} operations"
            )));
        }
        let ops: Vec<_> = ops.iter().cloned().map(NodeOp::into_dom).collect();
        if ops.iter().map(DomOp::payload_len).sum::<usize>() > MAX_HTML * 4 {
            return Err(limit_error("Batch payload exceeds 4 MiB"));
        }
        self.run_ops_resolved(&ops, true, true)
    }

    /// Matching elements in DOM order, including elements without HTML IDs. Paging is bounded.
    pub fn query_nodes(&self, selector: &str, offset: usize, limit: usize) -> Result<Value> {
        if selector.len() > 4096 || limit > public_dom::MAX_CHILDREN {
            return Err(arg(
                "Selector must be at most 4096 bytes; limit at most 4096",
            ));
        }
        let service = crate::style::query::SelectorService::new();
        let list = service
            .parse(selector)
            .map_err(|e| arg(format!("Invalid selector: {:?}", e.sub_reason)))?;
        let d = self.pipeline.document();
        let found = service.query_all(d, &list, d.document_node())?;
        let nodes: Vec<_> = found
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&n| self.node_identity(n))
            .collect();
        Ok(json!({"nodes":nodes,"total":found.len(),"offset":offset}))
    }

    fn node_identity(&self, n: NodeId) -> Value {
        json!({"node":public_dom::node_ref(n),"element":self.pipeline.document().get_attribute(n,"id").ok().flatten().filter(|id| !id.is_empty())})
    }

    /// Coordinates are viewport CSS pixels, with native hit-test ordering and pointer-events rules.
    pub fn element_at_point(&mut self, x: f64, y: f64) -> Result<Value> {
        if !x.is_finite() || !y.is_finite() {
            return Err(arg("Coordinates must be finite"));
        }
        if x < 0.0
            || y < 0.0
            || x >= self.pipeline.config.viewport_width
            || y >= self.pipeline.config.viewport_height
        {
            return Ok(Value::Null);
        }
        let id = self.id();
        Ok(DispatchHost::hit_test(self, id, x, y)?.map_or(Value::Null, |n| self.node_identity(n)))
    }

    pub fn focused_element(&self) -> Value {
        self.interaction
            .focused_element(self.id())
            .map_or(Value::Null, |n| self.node_identity(n))
    }

    pub fn focus_target(&mut self, element: Option<&str>, node: Option<&str>) -> Result<()> {
        let n = self.target_of(element, node)?;
        self.invalidate_clipboard_context();
        let id = self.id();
        crate::interaction::focus::focus(self, id, n)?;
        Ok(())
    }

    /// Resolve the declared native-drop receiver now. The returned opaque document/node token
    /// must be retained until native transfer completion; hit-testing again would change ownership.
    pub fn native_drop_target(&mut self, x: f64, y: f64, files: bool) -> Option<Value> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let id = self.id();
        let hit = DispatchHost::hit_test(self, id, x, y).ok().flatten()?;
        let target = self.drop_receiver(hit, files)?;
        let mut result = self.node_identity(target);
        let raw = id.raw();
        result["document"] = json!([raw.index, raw.generation.get()]);
        Some(result)
    }

    /// Revalidate the original receiver and its current policy without acquiring a new target.
    pub fn native_drop_accepts(&self, target: &Value, files: bool) -> bool {
        let raw = self.id().raw();
        if target["document"] != json!([raw.index, raw.generation.get()]) {
            return false;
        }
        let Some(reference) = target["node"].as_str() else {
            return false;
        };
        let Ok(node) = self.live_ref(reference) else {
            return false;
        };
        self.drop_receiver(node, files) == Some(node)
    }

    fn drop_receiver(&self, hit: NodeId, files: bool) -> Option<NodeId> {
        let d = self.pipeline.document();
        if !d.arena().is_connected(hit) || d.is_inert(hit) {
            return None;
        }
        let wanted = if files { "files" } else { "text" };
        let mut at = Some(hit);
        let mut receiver = None;
        let mut saw_policy = false;
        while let Some(node) = at {
            if d.control_facts(node).is_some_and(|f| f.disabled) || d.is_inert(node) {
                return None;
            }
            if let Some(policy) = d.get_attribute(node, "data-ui4me-drop").ok().flatten() {
                let tokens: Vec<_> = policy.split_ascii_whitespace().collect();
                if tokens.contains(&"none") {
                    return None;
                }
                if !saw_policy {
                    saw_policy = true;
                    if tokens.contains(&wanted) {
                        receiver = Some(node);
                    }
                }
            }
            at = d.parent(node).ok().flatten();
        }
        receiver
    }

    /// With no target, blur the current control. Blurring another live node is a no-op.
    pub fn blur(&mut self, element: Option<&str>, node: Option<&str>) -> Result<()> {
        let n = if element.is_none() && node.is_none() {
            let Some(n) = self.interaction.focused_element(self.id()) else {
                return Ok(());
            };
            n
        } else {
            self.target_of(element, node)?
        };
        self.invalidate_clipboard_context();
        let id = self.id();
        crate::interaction::focus::blur(self, id, n)?;
        Ok(())
    }

    fn text_target(&self, element: Option<&str>, node: Option<&str>) -> Result<NodeId> {
        let n = self.target_of(element, node)?;
        if editing::text_kind(self.pipeline.document(), n).is_none() {
            return Err(arg("Operation requires a connected text input or textarea"));
        }
        Ok(n)
    }

    fn selection_of(&self, n: NodeId) -> Selection {
        let d = self.pipeline.document();
        let len = d
            .control_value(n)
            .unwrap_or_default()
            .encode_utf16()
            .count() as u32;
        d.control_state(n)
            .ok()
            .flatten()
            .and_then(|c| c.selection)
            .unwrap_or(Selection {
                start: len,
                end: len,
                direction: SelectionDirection::None,
            })
    }

    fn selection_json(&self, n: NodeId) -> Value {
        let s = self.selection_of(n);
        let text = self
            .pipeline
            .document()
            .control_value(n)
            .unwrap_or_default();
        json!({"start":s.start,"end":s.end,"direction":match s.direction { SelectionDirection::Forward=>"forward",SelectionDirection::Backward=>"backward",SelectionDirection::None=>"none" },"length":text.encode_utf16().count(),"node":public_dom::node_ref(n)})
    }

    pub fn control_selection(&self, element: Option<&str>, node: Option<&str>) -> Result<Value> {
        Ok(self.selection_json(self.text_target(element, node)?))
    }

    /// UTF-16 offsets are clamped to the value length, but a split surrogate is rejected.
    /// The API does not steal keyboard focus. `end < start` collapses to end, as setSelectionRange.
    pub fn set_control_selection(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        start: u32,
        end: u32,
        direction: &str,
    ) -> Result<Value> {
        let n = self.text_target(element, node)?;
        let direction = parse_direction(direction)?;
        let value = self
            .pipeline
            .document()
            .control_value(n)
            .unwrap_or_default();
        let len = value.encode_utf16().count() as u32;
        let end = end.min(len);
        let start = start.min(end);
        check_offset(&value, start)?;
        check_offset(&value, end)?;
        self.invalidate_clipboard_context();
        let id = self.id();
        dispatch::operation(self, |host| {
            editing::set_application_selection(host, id, n, start, end, direction)
        })?;
        self.repaint = true;
        Ok(self.selection_json(n))
    }

    /// One native editing transaction: beforeinput, the replacement, input, selectionchange.
    /// Undo restores the original value and selection. Read-only/disabled controls are unchanged.
    /// `start/end` both absent use the selection; modes are preserve/select/start/end.
    pub fn replace_control_range(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        replacement: &str,
        start: Option<u32>,
        end: Option<u32>,
        selection_mode: &str,
    ) -> Result<Value> {
        check_text(replacement)?;
        if !matches!(selection_mode, "preserve" | "select" | "start" | "end") {
            return Err(arg("selection_mode must be preserve, select, start or end"));
        }
        let n = self.text_target(element, node)?;
        let old = self.selection_of(n);
        let (start, end) = match (start, end) {
            (Some(a), Some(b)) => (a, b),
            (None, None) => (old.start, old.end),
            _ => return Err(arg("Give both start and end, or neither")),
        };
        let value = self
            .pipeline
            .document()
            .control_value(n)
            .unwrap_or_default();
        if start > end || end as usize > value.encode_utf16().count() {
            return Err(arg("Range must satisfy 0 <= start <= end <= UTF-16 length"));
        }
        check_offset(&value, start)?;
        check_offset(&value, end)?;
        self.invalidate_clipboard_context();
        let id = self.id();
        let changed = dispatch::operation(self, |host| {
            editing::replace_application_range(host, id, n, replacement, start, end, selection_mode)
        })?;
        self.repaint |= changed;
        let mut result = self.selection_json(n);
        result["changed"] = json!(changed);
        Ok(result)
    }

    /// Execute a native edit/menu command. Clipboard work is queued to the existing host service;
    /// acceptance does not claim that an asynchronous read/write has already completed.
    pub fn edit_command(
        &mut self,
        command: &str,
        element: Option<&str>,
        node: Option<&str>,
    ) -> Result<Value> {
        let key = match command {
            "undo" | "redo" => "z",
            "cut" => "x",
            "copy" => "c",
            "paste" => "v",
            "select_all" => "a",
            "delete" => "Delete",
            _ => {
                return Err(arg(
                    "Command must be undo, redo, cut, copy, paste, select_all or delete",
                ));
            }
        };
        let n = if element.is_none() && node.is_none() {
            self.interaction
                .focused_element(self.id())
                .unwrap_or(self.pipeline.document().document_node())
        } else {
            self.target_of(element, node)?
        };
        let text = editing::text_kind(self.pipeline.document(), n).is_some();
        if !text && !matches!(command, "copy" | "cut" | "paste") {
            return Err(arg(
                "This command requires a focused or specified text control",
            ));
        }
        self.invalidate_clipboard_context();
        let id = self.id();
        let transfer_serial = self.native_clipboard.request_serial();
        let modifiers = Modifiers {
            ctrl: command != "delete",
            shift: command == "redo",
            ..Modifiers::default()
        };
        let consumed = dispatch::operation(self, |host| {
            if text {
                editing::finish_application_composition(host, id, n)?;
                editing::keydown(
                    host,
                    id,
                    n,
                    &KeyData::new(key, "Unidentified"),
                    modifiers,
                    DispatchOrigin::ElementTask,
                    None,
                )
            } else {
                let c = editing::clipboard_command(key, modifiers)
                    .expect("validated clipboard command");
                editing::document_clipboard(
                    host,
                    id,
                    n,
                    c,
                    modifiers,
                    DispatchOrigin::ElementTask,
                    None,
                )
                .map(|()| true)
            }
        })?;
        self.repaint = true;
        Ok(
            json!({"accepted":consumed,"clipboard_pending":self.native_clipboard.request_serial()!=transfer_serial}),
        )
    }

    fn form_target(&self, element: Option<&str>, node: Option<&str>) -> Result<NodeId> {
        let n = self.target_of(element, node)?;
        if self.pipeline.document().local_name(n)? != "form" {
            return Err(arg("Operation requires a form"));
        }
        Ok(n)
    }

    fn submitter_target(
        &self,
        form: NodeId,
        element: Option<&str>,
        node: Option<&str>,
    ) -> Result<Option<NodeId>> {
        if element.is_none() && node.is_none() {
            return Ok(None);
        }
        let n = self.target_of(element, node)?;
        let d = self.pipeline.document();
        let submit = matches!(d.input_type(n), Some(InputType::Submit | InputType::Image))
            || (d.local_name(n)? == "button"
                && !matches!(
                    d.get_attribute(n, "type")?
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "reset" | "button"
                ));
        if !submit || d.form_owner(n)? != Some(form) {
            return Err(arg(
                "Submitter must be a submit button belonging to this form",
            ));
        }
        Ok(Some(n))
    }

    pub fn reset_form(&mut self, element: Option<&str>, node: Option<&str>) -> Result<Value> {
        let n = self.form_target(element, node)?;
        self.invalidate_clipboard_context();
        let id = self.id();
        let reset = dispatch::operation(self, |host| -> std::result::Result<bool, Status> {
            let out = dispatch::run_dispatch(
                host,
                id,
                n,
                EventInit::new("reset").bubbles(true).cancelable(true),
                true,
                DispatchOrigin::ElementTask,
                None,
            )?;
            if out.canceled {
                return Ok(false);
            }
            dispatch::eng(host, id, |_, d| d.form_reset_controls(n))??;
            host.form_reset(id, n);
            Ok(true)
        })?;
        self.after_mutation(false);
        Ok(json!({"reset":reset,"default_prevented":!reset}))
    }

    pub fn request_submit(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        submitter: Option<&str>,
        submitter_node: Option<&str>,
    ) -> Result<Value> {
        let form = self.form_target(element, node)?;
        let submitter = self.submitter_target(form, submitter, submitter_node)?;
        let before = self.events.len();
        let id = self.id();
        dispatch::operation(self, |host| {
            crate::interaction::activation::submit_form(host, id, form, submitter)
        });
        let event = self.events[before..].iter().find(|e| e["type"] == "submit");
        Ok(
            json!({"submitted":event.is_some(),"default_prevented":event.and_then(|e|e["default_prevented"].as_bool()).unwrap_or(false)}),
        )
    }

    /// Ordered successful controls, preserving duplicate names and selected-option/file order.
    /// Values remain local data: no URL encoding, upload, path reads or navigation occurs here.
    pub fn form_data(
        &self,
        element: Option<&str>,
        node: Option<&str>,
        submitter: Option<&str>,
        submitter_node: Option<&str>,
    ) -> Result<Value> {
        let form = self.form_target(element, node)?;
        let submitter = self.submitter_target(form, submitter, submitter_node)?;
        let d = self.pipeline.document();
        let mut entries = Vec::new();
        for n in d.form_elements(form)? {
            if d.control_facts(n).is_none_or(|f| f.disabled) {
                continue;
            }
            // Controls inside datalist are not successful controls.
            let mut parent = d.parent(n)?;
            let mut in_datalist = false;
            while let Some(p) = parent {
                if d.local_name(p).ok() == Some("datalist") {
                    in_datalist = true;
                    break;
                }
                parent = d.parent(p)?;
            }
            if in_datalist {
                continue;
            }
            let name = d.get_attribute(n, "name")?.unwrap_or("");
            let kind = d.input_type(n);
            let tag = d.local_name(n)?;
            if kind == Some(InputType::Image) {
                if Some(n) == submitter {
                    let prefix = if name.is_empty() {
                        String::new()
                    } else {
                        format!("{name}.")
                    };
                    entries.push(json!({"name":format!("{prefix}x"),"kind":"text","value":"0"}));
                    entries.push(json!({"name":format!("{prefix}y"),"kind":"text","value":"0"}));
                }
                continue;
            }
            if name.is_empty() {
                continue;
            }
            if (tag == "button"
                || matches!(
                    kind,
                    Some(InputType::Submit | InputType::Reset | InputType::Button)
                ))
                && Some(n) != submitter
            {
                continue;
            }
            if matches!(kind, Some(InputType::Checkbox | InputType::Radio))
                && !d.control_facts(n).is_some_and(|f| f.checked == Some(true))
            {
                continue;
            }
            if kind == Some(InputType::File) {
                let paths = self.input_files.get(&n).map(Vec::as_slice).unwrap_or(&[]);
                if paths.is_empty() {
                    entries.push(json!({"name":name,"kind":"file","path":null,"filename":""}));
                }
                for path in paths {
                    entries.push(json!({"name":name,"kind":"file","path":path,"filename":file_basename(path)}));
                }
                continue;
            }
            if tag == "select" {
                for option in d.select_options(n) {
                    if d.option_selected(option) && !d.option_disabled(option) {
                        entries.push(
                            json!({"name":name,"kind":"text","value":d.option_value(option)}),
                        );
                    }
                }
                continue;
            }
            if !matches!(tag, "input" | "textarea" | "button") {
                continue;
            }
            let value = if kind == Some(InputType::Hidden) && name == "_charset_" {
                "UTF-8".to_owned()
            } else if matches!(kind, Some(InputType::Checkbox | InputType::Radio)) {
                d.get_attribute(n, "value")?.unwrap_or("on").to_owned()
            } else if tag == "button" {
                d.get_attribute(n, "value")?.unwrap_or("").to_owned()
            } else {
                d.control_value(n).unwrap_or_default()
            };
            entries.push(json!({"name":name,"kind":"text","value":value}));
            if (tag == "textarea"
                || kind.is_some_and(|t| matches!(t, InputType::Text | InputType::Search)))
                && let Some(dirname) = d.get_attribute(n, "dirname")?.filter(|s| !s.is_empty())
            {
                let mut at = Some(n);
                let mut direction = "ltr";
                while let Some(p) = at {
                    if let Some(dir) = d.get_attribute(p, "dir").ok().flatten()
                        && matches!(dir, "rtl" | "ltr")
                    {
                        direction = dir;
                        break;
                    }
                    at = d.parent(p)?;
                }
                entries.push(json!({"name":dirname,"kind":"text","value":direction}));
            }
        }
        Ok(json!({"entries":entries,"submitter":submitter.map(|n|self.node_identity(n))}))
    }

    pub fn set_custom_validity(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        message: &str,
    ) -> Result<Value> {
        check_text(message)?;
        let n = self.target_of(element, node)?;
        if self.pipeline.document().control_facts(n).is_none() {
            return Err(arg("Custom validity requires a form control"));
        }
        self.pipeline.mutate(|d| {
            d.control_state_writer()
                .set_custom_validity_message(n, message)?;
            // Publish the selector-relevant custom-error bit as well as the message. Keep
            // normal value-dependent validity derived by the style adapter after clearing it.
            d.control_state_writer().set_validity_flags(
                n,
                if message.is_empty() {
                    0
                } else {
                    ui4me_document::control_state::validity::CUSTOM_ERROR
                },
            )
        })?;
        self.after_mutation(false);
        Ok(self
            .pipeline
            .document()
            .constraint_validity(n)
            .map_or(json!({"valid":true,"will_validate":false}), |v| {
                validity_json(&v)
            }))
    }

    /// Optional input hints for native adapters. Unknown/unspecified metadata remains null.
    pub fn ime_hints(&self) -> Value {
        let Some(n) = self.interaction.focused_element(self.id()) else {
            return Value::Null;
        };
        let d = self.pipeline.document();
        let Some(kind) = editing::text_kind(d, n) else {
            return Value::Null;
        };
        let attr = |key| d.get_attribute(n, key).ok().flatten();
        json!({"input_type":d.input_type(n).map(InputType::as_str).unwrap_or("textarea"),"inputmode":attr("inputmode"),"autocomplete":attr("autocomplete"),"spellcheck":attr("spellcheck").and_then(|v|match v {"true"|""=>Some(true),"false"=>Some(false),_=>None}),"password":kind.password,"multiline":kind.multiline})
    }
}

pub(super) fn file_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn parse_direction(direction: &str) -> Result<SelectionDirection> {
    match direction {
        "forward" => Ok(SelectionDirection::Forward),
        "backward" => Ok(SelectionDirection::Backward),
        "none" => Ok(SelectionDirection::None),
        _ => Err(arg("direction must be forward, backward or none")),
    }
}

fn check_offset(value: &str, offset: u32) -> Result<()> {
    let mut at = 0;
    for ch in value.chars() {
        if at == offset {
            return Ok(());
        }
        at += ch.len_utf16() as u32;
        if at > offset {
            return Err(arg("UTF-16 offset splits a surrogate pair"));
        }
    }
    if at == offset {
        Ok(())
    } else {
        Err(arg("UTF-16 offset exceeds value length"))
    }
}
