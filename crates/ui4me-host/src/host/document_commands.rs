//! Validated document operations. Native lifetime and OS services remain host methods.
use super::*;
use serde::Deserialize;
use ui4me_renderer::{Document, DomOp, NodeOp, ScrollState};

fn yes() -> bool {
    true
}
fn start() -> String {
    "start".into()
}
fn nearest() -> String {
    "nearest".into()
}
fn children_limit() -> usize {
    256
}
fn query_limit() -> usize {
    1024
}
fn mouse_pointer() -> u32 {
    1
}
fn no_direction() -> String {
    "none".into()
}
fn preserve_selection() -> String {
    "preserve".into()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DocumentCommand {
    RegisterResourceBundle {
        name: String,
        entries: Vec<ResourceEntry>,
    },
    UnregisterResourceBundle {
        name: String,
    },
    GetResourceStats,
    PurgeUnusedResources {
        #[serde(default)]
        url: Option<String>,
    },
    ElementAtPoint {
        x: f64,
        y: f64,
    },
    GetFocusedElement,
    Blur {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
    },
    GetControlSelection {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
    },
    SetControlSelection {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        start: u32,
        end: u32,
        #[serde(default = "no_direction")]
        direction: String,
    },
    ReplaceControlRange {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        replacement: String,
        #[serde(default)]
        start: Option<u32>,
        #[serde(default)]
        end: Option<u32>,
        #[serde(default = "preserve_selection")]
        selection_mode: String,
    },
    EditCommand {
        command: String,
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
    },
    ResetForm {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
    },
    RequestSubmit {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        submitter: Option<String>,
        #[serde(default)]
        submitter_node: Option<String>,
    },
    GetFormData {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        submitter: Option<String>,
        #[serde(default)]
        submitter_node: Option<String>,
    },
    SetCustomValidity {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        message: String,
    },
    QueryNodes {
        selector: String,
        #[serde(default)]
        offset: usize,
        #[serde(default = "query_limit")]
        limit: usize,
    },
    NodeBatch {
        ops: Vec<NodeBatchOp>,
    },
    SetText {
        element: String,
        text: String,
    },
    GetValue {
        element: String,
    },
    SetValue {
        element: String,
        value: String,
    },
    GetChecked {
        element: String,
    },
    SetChecked {
        element: String,
        checked: bool,
    },
    SetAttribute {
        element: String,
        name: String,
        value: String,
    },
    RemoveAttribute {
        element: String,
        name: String,
    },
    AddClass {
        element: String,
        name: String,
    },
    RemoveClass {
        element: String,
        name: String,
    },
    Focus {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
    },
    ReplaceChildrenHtml {
        element: String,
        html: String,
    },
    InsertHtml {
        element: String,
        position: String,
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
    GetHtml {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        outer: bool,
        /// UTF-8 byte offset. Must be a character boundary.
        #[serde(default)]
        offset: usize,
        /// Optional byte window (4..=262144). Omit to request the full remainder.
        #[serde(default)]
        limit: Option<usize>,
    },
    TextRects {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        start: Option<u32>,
        #[serde(default)]
        end: Option<u32>,
    },
    TextAtPoint {
        x: f64,
        y: f64,
    },
    GetFiles {
        element: String,
    },
    GetComputedStyle {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        properties: Vec<String>,
    },
    AddStylesheet {
        css: String,
    },
    ReplaceStylesheet {
        sheet: u64,
        css: String,
    },
    RemoveStylesheet {
        sheet: u64,
    },
    ListStylesheets,
    NodeOf {
        #[serde(default)]
        element: Option<String>,
    },
    GetNode {
        node: String,
    },
    NodeChildren {
        node: String,
        #[serde(default)]
        offset: usize,
        #[serde(default = "children_limit")]
        limit: usize,
    },
    GetText {
        element: String,
    },
    GetAttribute {
        element: String,
        name: String,
    },
    GetAttributes {
        element: String,
    },
    HasElement {
        element: String,
    },
    Query {
        selector: String,
        #[serde(default = "query_limit")]
        limit: usize,
        #[serde(default)]
        offset: usize,
    },
    Batch {
        ops: Vec<BatchOp>,
    },
    SetDefaultAction {
        element: String,
        event: String,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        shift: Option<bool>,
        #[serde(default)]
        ctrl: Option<bool>,
        #[serde(default)]
        alt: Option<bool>,
        #[serde(default)]
        meta: Option<bool>,
        #[serde(default = "yes")]
        prevent: bool,
    },
    GetValidity {
        element: String,
    },
    CheckValidity {
        element: String,
    },
    GetScroll {
        #[serde(default)]
        element: Option<String>,
    },
    ScrollTo {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        /// `auto` (the element's `scroll-behavior`), `instant` or `smooth` (0.8 F0803).
        #[serde(default)]
        behavior: Option<String>,
    },
    ScrollBy {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
        #[serde(default)]
        behavior: Option<String>,
    },
    ScrollIntoView {
        element: String,
        #[serde(default = "nearest")]
        block: String,
        #[serde(default = "nearest")]
        inline: String,
        #[serde(default)]
        behavior: Option<String>,
    },
    GetRect {
        element: String,
    },
    SetEnvironment {
        #[serde(default)]
        color_scheme: Option<String>,
        #[serde(default)]
        reduced_motion: Option<String>,
    },
    GetEnvironment,
    SetScrollFollow {
        #[serde(default)]
        element: Option<String>,
        #[serde(default = "yes")]
        follow: bool,
    },
    GetSelection,
    ClearSelection,
    SelectText {
        #[serde(default)]
        element: Option<String>,
    },
    CopySelection,
    AppendText {
        element: String,
        text: String,
    },
    TrimChildren {
        element: String,
        keep: usize,
        #[serde(default = "start")]
        from: String,
    },
    ShowDialog {
        element: String,
        #[serde(default = "yes")]
        modal: bool,
    },
    CloseDialog {
        element: String,
        #[serde(default)]
        return_value: Option<String>,
    },
    GetDialog {
        element: String,
    },
    ShowPopover {
        element: String,
    },
    HidePopover {
        element: String,
    },
    TogglePopover {
        element: String,
    },
    GetPopover {
        element: String,
    },
    SetPointerCapture {
        element: String,
        #[serde(default = "mouse_pointer")]
        pointer_id: u32,
        #[serde(default = "yes")]
        capture: bool,
    },
    GetPointerCapture,
    GetOptions {
        element: String,
    },
    SetSelectedValues {
        element: String,
        values: Vec<String>,
    },
    GetAccessibilityTree,
    /// Bounded snapshot of the engine's current diagnostics.
    GetDiagnostics,
    CssProfile,
    Explain {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        property: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResourceEntry {
    path: String,
    data_url: String,
}

/// Decode the whole request before registering anything. A failed entry cannot
/// replace a working bundle or consume part of its aggregate source budget.
fn decode_resource_entries(
    entries: Vec<ResourceEntry>,
) -> Result<Vec<ui4me_assets::bundles::BundleEntryInput>, RpcError> {
    if entries.len() > 256 {
        return Err(RpcError::busy(
            "A resource registration permits at most 256 entries",
        ));
    }
    let mut bytes = 0usize;
    entries
        .into_iter()
        .map(|entry| {
            let url = data_url::DataUrl::process(&entry.data_url)
                .map_err(|_| RpcError::invalid_params("Resource entry must be a valid data URL"))?;
            let mime = url.mime_type().to_string();
            let (data, _) = url.decode_to_vec().map_err(|_| {
                RpcError::invalid_params("Resource entry data URL cannot be decoded")
            })?;
            bytes = bytes
                .checked_add(data.len())
                .ok_or_else(|| RpcError::busy("Resource registration size overflow"))?;
            if bytes > 1024 * 1024 {
                return Err(RpcError::busy(
                    "A resource registration permits at most 1 MiB decoded bytes",
                ));
            }
            Ok(ui4me_assets::bundles::BundleEntryInput::new(
                entry.path,
                data,
                Some(mime),
            ))
        })
        .collect()
}

/// Explicit, generation-checked node mutations. Anonymous HTML nodes need no
/// synthetic id attribute, and a stale node must never target a replacement.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum NodeBatchOp {
    SetText {
        node: String,
        text: String,
    },
    SetAttribute {
        node: String,
        name: String,
        value: String,
    },
    RemoveAttribute {
        node: String,
        name: String,
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
        #[serde(default = "start")]
        from: String,
    },
}

impl NodeBatchOp {
    fn into_engine(self) -> Result<NodeOp, RpcError> {
        Ok(match self {
            Self::SetText { node, text } => NodeOp::SetText { node, text },
            Self::SetAttribute { node, name, value } => NodeOp::SetAttribute {
                node,
                name,
                value: Some(value),
            },
            Self::RemoveAttribute { node, name } => NodeOp::SetAttribute {
                node,
                name,
                value: None,
            },
            Self::AddClass { node, name } => NodeOp::AddClass { node, name },
            Self::RemoveClass { node, name } => NodeOp::RemoveClass { node, name },
            Self::InsertHtml {
                node,
                position,
                html,
            } => NodeOp::InsertHtml {
                node,
                position,
                html,
            },
            Self::ReplaceChildrenHtml { node, html } => NodeOp::ReplaceChildrenHtml { node, html },
            Self::RemoveElement { node } => NodeOp::RemoveElement { node },
            Self::MoveElement {
                node,
                target_node,
                position,
            } => NodeOp::MoveElement {
                node,
                target_node,
                position,
            },
            Self::AppendText { node, text } => NodeOp::AppendText { node, text },
            Self::TrimChildren { node, keep, from } => NodeOp::TrimChildren {
                node,
                keep,
                from_start: trim_from_start(&from)?,
            },
        })
    }
}

/// One operation of a `batch` command; the same fields as the single command.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum BatchOp {
    SetText {
        element: String,
        text: String,
    },
    SetAttribute {
        element: String,
        name: String,
        value: String,
    },
    RemoveAttribute {
        element: String,
        name: String,
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
    AppendText {
        element: String,
        text: String,
    },
    TrimChildren {
        element: String,
        keep: usize,
        #[serde(default = "start")]
        from: String,
    },
}

impl BatchOp {
    fn into_engine(self) -> Result<DomOp, RpcError> {
        Ok(match self {
            Self::SetText { element, text } => DomOp::SetText { element, text },
            Self::SetAttribute {
                element,
                name,
                value,
            } => DomOp::SetAttribute {
                element,
                name,
                value: Some(value),
            },
            Self::RemoveAttribute { element, name } => DomOp::SetAttribute {
                element,
                name,
                value: None,
            },
            Self::AddClass { element, name } => DomOp::AddClass { element, name },
            Self::RemoveClass { element, name } => DomOp::RemoveClass { element, name },
            Self::InsertHtml {
                element,
                position,
                html,
            } => DomOp::InsertHtml {
                element,
                position,
                html,
            },
            Self::ReplaceChildrenHtml { element, html } => {
                DomOp::ReplaceChildrenHtml { element, html }
            }
            Self::RemoveElement { element } => DomOp::RemoveElement { element },
            Self::MoveElement {
                element,
                target,
                position,
            } => DomOp::MoveElement {
                element,
                target,
                position,
            },
            Self::AppendText { element, text } => DomOp::AppendText { element, text },
            Self::TrimChildren {
                element,
                keep,
                from,
            } => DomOp::TrimChildren {
                element,
                keep,
                from_start: trim_from_start(&from)?,
            },
        })
    }
}

fn trim_from_start(from: &str) -> Result<bool, RpcError> {
    match from {
        "start" => Ok(true),
        "end" => Ok(false),
        _ => Err(RpcError::invalid_params("from must be start or end")),
    }
}

fn scroll_json(s: ScrollState) -> Value {
    json!({"x":s.x,"y":s.y,"min_x":s.min_x,"min_y":s.min_y,
        "max_x":s.max_x,"max_y":s.max_y,"width":s.width,"height":s.height,
        "scrollable":s.scrollable,"animating":s.animating})
}

fn changed(result: ui4me_renderer::Result<()>) -> Result<Value, RpcError> {
    result?;
    Ok(json!({"updated":true}))
}

impl DocumentCommand {
    pub(super) fn execute(self, document: &mut Document) -> Result<Value, RpcError> {
        let value = match self {
            Self::RegisterResourceBundle { name, entries } => {
                document.register_resource_bundle(&name, decode_resource_entries(entries)?)?
            }
            Self::UnregisterResourceBundle { name } => {
                document.unregister_resource_bundle(&name)?
            }
            Self::GetResourceStats => document.resource_stats()?,
            Self::PurgeUnusedResources { url } => {
                document.purge_unused_resources(url.as_deref())?
            }
            Self::ElementAtPoint { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(RpcError::invalid_params("Coordinates must be finite"));
                }
                document.element_at_point(x, y)?
            }
            Self::GetFocusedElement => document.focused_element(),
            Self::Blur { element, node } => {
                changed(document.blur(element.as_deref(), node.as_deref()))?
            }
            Self::GetControlSelection { element, node } => {
                document.control_selection(element.as_deref(), node.as_deref())?
            }
            Self::SetControlSelection {
                element,
                node,
                start,
                end,
                direction,
            } => document.set_control_selection(
                element.as_deref(),
                node.as_deref(),
                start,
                end,
                &direction,
            )?,
            Self::ReplaceControlRange {
                element,
                node,
                replacement,
                start,
                end,
                selection_mode,
            } => document.replace_control_range(
                element.as_deref(),
                node.as_deref(),
                &replacement,
                start,
                end,
                &selection_mode,
            )?,
            Self::EditCommand {
                command,
                element,
                node,
            } => document.edit_command(&command, element.as_deref(), node.as_deref())?,
            Self::ResetForm { element, node } => {
                document.reset_form(element.as_deref(), node.as_deref())?
            }
            Self::RequestSubmit {
                element,
                node,
                submitter,
                submitter_node,
            } => document.request_submit(
                element.as_deref(),
                node.as_deref(),
                submitter.as_deref(),
                submitter_node.as_deref(),
            )?,
            Self::GetFormData {
                element,
                node,
                submitter,
                submitter_node,
            } => document.form_data(
                element.as_deref(),
                node.as_deref(),
                submitter.as_deref(),
                submitter_node.as_deref(),
            )?,
            Self::SetCustomValidity {
                element,
                node,
                message,
            } => document.set_custom_validity(element.as_deref(), node.as_deref(), &message)?,
            Self::QueryNodes {
                selector,
                offset,
                limit,
            } => {
                if limit == 0 || limit > 4096 {
                    return Err(RpcError::invalid_params("query limit must be 1..4096"));
                }
                document.query_nodes(&selector, offset, limit)?
            }
            Self::NodeBatch { ops } => {
                if ops.len() > 1024 {
                    return Err(RpcError::busy(
                        "A node batch permits at most 1024 operations",
                    ));
                }
                let ops = ops
                    .into_iter()
                    .map(NodeBatchOp::into_engine)
                    .collect::<Result<Vec<_>, _>>()?;
                changed(document.apply_node_batch(&ops))?
            }
            Self::SetText { element, text } => changed(document.set_text(&element, &text))?,
            Self::GetText { element } => json!({"text":document.get_text(&element)?}),
            Self::GetValue { element } => json!({"value":document.get_value(&element)?}),
            Self::SetValue { element, value } => changed(document.set_value(&element, &value))?,
            Self::GetChecked { element } => json!({"checked":document.get_checked(&element)?}),
            Self::SetChecked { element, checked } => {
                changed(document.set_checked(&element, checked))?
            }
            Self::SetAttribute {
                element,
                name,
                value,
            } => changed(document.set_attribute(&element, &name, Some(&value)))?,
            Self::RemoveAttribute { element, name } => {
                changed(document.set_attribute(&element, &name, None))?
            }
            Self::AddClass { element, name } => changed(document.add_class(&element, &name))?,
            Self::RemoveClass { element, name } => changed(document.remove_class(&element, &name))?,
            Self::GetAttribute { element, name } => {
                json!({"value":document.get_attribute(&element, &name)?})
            }
            Self::GetAttributes { element } => {
                let attributes: serde_json::Map<String, Value> = document
                    .get_attributes(&element)?
                    .into_iter()
                    .map(|(name, value)| (name, Value::String(value)))
                    .collect();
                json!({"attributes":attributes})
            }
            Self::HasElement { element } => json!({"exists":document.has_element(&element)}),
            Self::Query {
                selector,
                offset,
                limit,
            } => {
                if limit == 0 || limit > 4096 {
                    return Err(RpcError::invalid_params("query limit must be 1..4096"));
                }
                // The model itself caps all nodes at 65,536. Query all identified
                // matches once so pagination counts do not confuse anonymous nodes
                // with truncated identified matches.
                let (ids, matched_nodes) = document.query_ids(&selector, 65_536)?;
                let total = ids.len();
                let selected: Vec<_> = ids.into_iter().skip(offset).take(limit).collect();
                let next = offset.saturating_add(selected.len());
                json!({"ids":selected,"total":total,"matched_nodes":matched_nodes,
                    "offset":offset,"limit":limit,"truncated":next < total})
            }
            Self::Focus { element, node } => {
                changed(document.focus_target(element.as_deref(), node.as_deref()))?
            }
            Self::ReplaceChildrenHtml { element, html } => {
                changed(document.replace_children_html(&element, &html))?
            }
            Self::InsertHtml {
                element,
                position,
                html,
            } => changed(document.insert_html(&element, &position, &html))?,
            Self::RemoveElement { element } => changed(document.remove_element(&element))?,
            Self::MoveElement {
                element,
                target,
                position,
            } => changed(document.move_element(&element, &target, &position))?,
            Self::AppendText { element, text } => changed(document.append_text(&element, &text))?,
            Self::TrimChildren {
                element,
                keep,
                from,
            } => changed(document.trim_children(&element, keep, trim_from_start(&from)?))?,
            Self::Batch { ops } => {
                if ops.len() > 1024 {
                    return Err(RpcError::busy(
                        "A document batch permits at most 1024 operations",
                    ));
                }
                let operations = ops
                    .into_iter()
                    .map(BatchOp::into_engine)
                    .collect::<Result<Vec<_>, _>>()?;
                changed(document.apply_batch(&operations))?
            }
            Self::GetHtml {
                element,
                node,
                outer,
                offset,
                limit,
            } => {
                if limit.is_some_and(|limit| !(4..=262_144).contains(&limit)) {
                    return Err(RpcError::invalid_params(
                        "HTML byte limit must be 4..262144",
                    ));
                }
                let html = document.get_html(element.as_deref(), node.as_deref(), outer)?;
                if !html.is_char_boundary(offset) {
                    return Err(RpcError::invalid_params("HTML offset must be a UTF-8 character boundary within the serialized document"));
                }
                let mut end = limit.map_or(html.len(), |limit| {
                    offset.saturating_add(limit).min(html.len())
                });
                while !html.is_char_boundary(end) {
                    end -= 1;
                }
                json!({"html":&html[offset..end],"total_bytes":html.len(),"offset":offset,
                    "next_offset":(end < html.len()).then_some(end),"truncated":end < html.len()})
            }
            Self::NodeOf { element } => json!({"node":document.node_of(element.as_deref())?}),
            Self::GetNode { node } => document.get_node(&node)?,
            Self::NodeChildren {
                node,
                offset,
                limit,
            } => {
                if limit == 0 || limit > 4096 {
                    return Err(RpcError::invalid_params("children limit must be 1..4096"));
                }
                document.node_children(&node, offset, limit)?
            }
            Self::AddStylesheet { css } => document.add_stylesheet(&css)?,
            Self::ReplaceStylesheet { sheet, css } => document.replace_stylesheet(sheet, &css)?,
            Self::RemoveStylesheet { sheet } => changed(document.remove_stylesheet(sheet))?,
            Self::ListStylesheets => json!({"sheets":document.list_stylesheets()}),
            Self::GetComputedStyle {
                element,
                node,
                properties,
            } => document.computed_style(element.as_deref(), node.as_deref(), &properties)?,
            Self::GetRect { element } => json!({"rect":document.element_rect(&element)?.map(|r|
                json!({"x":r.x,"y":r.y,"width":r.width,"height":r.height}))}),
            Self::TextRects {
                element,
                node,
                start,
                end,
            } => document.text_rects(element.as_deref(), node.as_deref(), start, end)?,
            Self::TextAtPoint { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(RpcError::invalid_params("Text coordinates must be finite"));
                }
                json!({"hit":document.text_at_point(x, y)?})
            }
            Self::GetScroll { element } => scroll_json(document.scroll_state(element.as_deref())?),
            Self::ScrollTo {
                element,
                x,
                y,
                behavior,
            } => scroll_json(document.scroll_to_behavior(
                element.as_deref(),
                x,
                y,
                behavior.as_deref().unwrap_or("auto"),
            )?),
            Self::ScrollBy {
                element,
                dx,
                dy,
                behavior,
            } => scroll_json(document.scroll_by_behavior(
                element.as_deref(),
                dx,
                dy,
                behavior.as_deref().unwrap_or("auto"),
            )?),
            Self::ScrollIntoView {
                element,
                block,
                inline,
                behavior,
            } => changed(document.scroll_into_view_behavior(
                &element,
                &block,
                &inline,
                behavior.as_deref().unwrap_or("auto"),
            ))?,
            Self::SetScrollFollow { element, follow } => {
                changed(document.set_scroll_follow(element.as_deref(), follow))?
            }
            Self::GetSelection => json!({"text":document.selection_text()?}),
            Self::ClearSelection => {
                document.clear_selection();
                json!({"updated":true})
            }
            Self::SelectText { element } => changed(document.select_text(element.as_deref()))?,
            Self::CopySelection => {
                let text = document.selection_text()?;
                if text.len() > 1024 * 1024 {
                    return Err(RpcError::busy(
                        "Copied selection exceeds the 1 MiB native clipboard limit",
                    ));
                }
                // Admit the encoded reply before queuing the side effect. A
                // failed response-size check must not still write the clipboard.
                let result = bounded_result(json!({"text": &text}))?;
                if !text.is_empty() {
                    document.set_clipboard_text(&text)?;
                }
                result
            }
            Self::GetValidity { element } => document.get_validity(&element)?,
            Self::CheckValidity { element } => document.check_validity(&element)?,
            Self::GetFiles { element } => json!({"files":document.get_files(&element)?}),
            Self::GetOptions { element } => {
                let options: Vec<_> = document.select_options(&element)?.into_iter()
                    .map(|(value, label, selected, disabled)| json!({"value":value,"label":label,"selected":selected,"disabled":disabled})).collect();
                json!({"options":options})
            }
            Self::SetSelectedValues { element, values } => {
                changed(document.set_selected_values(&element, &values))?
            }
            Self::ShowDialog { element, modal } => changed(document.show_dialog(&element, modal))?,
            Self::CloseDialog {
                element,
                return_value,
            } => changed(document.close_dialog(&element, return_value.as_deref()))?,
            Self::GetDialog { element } => {
                let (open, modal, return_value) = document.dialog_state(&element)?;
                json!({"open":open,"modal":modal,"return_value":return_value})
            }
            Self::ShowPopover { element } => changed(document.show_popover(&element))?,
            Self::HidePopover { element } => changed(document.hide_popover(&element))?,
            Self::TogglePopover { element } => json!({"open":document.toggle_popover(&element)?}),
            Self::GetPopover { element } => {
                let (open, kind) = document.popover_state(&element)?;
                json!({"open":open,"kind":kind})
            }
            Self::SetDefaultAction {
                element,
                event,
                key,
                shift,
                ctrl,
                alt,
                meta,
                prevent,
            } => changed(document.set_default_action(
                ui4me_renderer::DefaultActionRule {
                    element,
                    event,
                    key,
                    shift,
                    ctrl,
                    alt,
                    meta,
                },
                prevent,
            ))?,
            Self::SetPointerCapture {
                element,
                pointer_id,
                capture,
            } => json!({"accepted":document.set_pointer_capture(&element, pointer_id, capture)?}),
            Self::GetPointerCapture => json!({"element":document.pointer_capture()}),
            Self::GetEnvironment => document.environment(),
            Self::SetEnvironment {
                color_scheme,
                reduced_motion,
            } => {
                let dark = match color_scheme.as_deref() {
                    None => None,
                    Some("light") => Some(Some(false)),
                    Some("dark") => Some(Some(true)),
                    Some("system") => Some(None),
                    Some(_) => {
                        return Err(RpcError::invalid_params(
                            "color_scheme must be light, dark or system",
                        ))
                    }
                };
                let reduce = match reduced_motion.as_deref() {
                    None => None,
                    Some("reduce") => Some(Some(true)),
                    Some("no-preference") => Some(Some(false)),
                    Some("system") => Some(None),
                    Some(_) => {
                        return Err(RpcError::invalid_params(
                            "reduced_motion must be reduce, no-preference or system",
                        ))
                    }
                };
                document.set_app_environment(dark, reduce)?;
                document.environment()
            }
            Self::GetAccessibilityTree => json!({"tree":document.accessibility_json()?}),
            Self::GetDiagnostics => {
                document.refresh_for_diagnostics();
                json!({"diagnostics":document.diagnostics(),"css_profile_version":ui4me_renderer::css_profile().0})
            }
            Self::CssProfile => {
                let (version, features) = ui4me_renderer::css_profile();
                let features: Vec<_> = features
                    .into_iter()
                    .map(|(id, state)| json!({"id":id,"state":state}))
                    .collect();
                json!({"version":version,"features":features})
            }
            Self::Explain {
                element,
                node,
                property,
            } => document.explain(element.as_deref(), node.as_deref(), property.as_deref())?,
        };
        Ok(value)
    }
}
