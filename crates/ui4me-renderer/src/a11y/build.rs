//! owner: 013-accessibility — lane L-A11Y-b1 (beta contract B3)
//!
//! DOM → accessibility tree (013 role-mapping.md, beta subset): roles from HTML-AAM and the v1
//! ARIA set, a simplified accname 1.2 (`aria-labelledby`, `aria-label`, native labels, content,
//! `title`, `placeholder`), states from control state and ARIA, exclusion of `aria-hidden`,
//! `hidden`, `display: none` and `visibility: hidden`. Generic containers (`div`, `span`, `label`,
//! row groups, unnamed `section`/`form`, ...) are flattened: their children join the parent.

use super::{
    A11yAction, A11yNode, A11yTree, Bounds, Checked, LivePoliteness, Numeric, ROOT_ID, RunText,
    States, TextSel, node_key,
};
use accesskit::Role;
use std::collections::{BTreeMap, HashMap};
use ui4me_document::document::Document;
use ui4me_document::node::{ElementData, NodeKind};
use ui4me_engine_types::ids::NodeId;
use unicode_segmentation::UnicodeSegmentation;

/// Tag bit of the text-run children of a text field (013 adapter-threading §5: a derived id space).
pub(crate) const TEXT_RUN_TAG: u64 = 1 << 63;
/// A text run's line number sits in bits 20..31 of its id (arena indices stay below 2^20: the
/// document caps nodes at 65,536); a field has at most [`MAX_RUNS`] runs.
pub(crate) const RUN_LINE_SHIFT: u32 = 20;
pub(crate) const RUN_LINE_MASK: u64 = 0xfff << RUN_LINE_SHIFT;
const MAX_RUNS: usize = 0xfff;

/// The id of line `line` of text field `field`.
fn run_key(field: u64, line: usize) -> u64 {
    field | TEXT_RUN_TAG | ((line.min(MAX_RUNS) as u64) << RUN_LINE_SHIFT)
}

/// A character of a run being built: UTF-16 start and end, viewport x extent, right-to-left.
type RunChar = (u32, u32, Option<(f64, f64)>, bool);

/// One line of a text control's editing area as laid out (0.5 F0504).
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// The value's UTF-16 range shown on the line (without a trailing line break).
    pub start: u32,
    pub end: u32,
    /// The line box in viewport CSS px (scroll applied).
    pub bounds: Bounds,
    /// Each caret-stop character (grapheme cluster) in logical order: UTF-16 start and end, and
    /// its viewport x extent (x0 < x1), right-to-left.
    pub clusters: Vec<(u32, u32, f64, f64, bool)>,
}

/// How an element renders, from the committed computed style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rendering {
    Rendered,
    /// `visibility: hidden | collapse`: the element is excluded, its children may still show.
    Invisible,
    /// `display: none` (itself or an ancestor), or not styled at all.
    NotRendered,
}

/// The style and layout answers the builder needs (the runtime reads them from the committed set).
pub trait RenderInfo {
    fn rendering(&self, element: NodeId) -> Rendering;
    /// Border-box union in viewport CSS px (scroll-adjusted); for a text node, the text's box.
    fn bounds(&self, node: NodeId) -> Option<Bounds>;
    /// Block-level (name-from-content joins block children with a space).
    fn is_block(&self, element: NodeId) -> bool {
        let _ = element;
        false
    }
    /// The element has event listeners (an included node then offers Click).
    fn has_listeners(&self, element: NodeId) -> bool {
        let _ = element;
        false
    }
    /// The laid-out lines of a text control (`input`, `textarea`); `None` without a layout (the
    /// field then has one run without geometry).
    fn text_lines(&self, field: NodeId) -> Option<Vec<TextLine>> {
        let _ = field;
        None
    }
}

/// Everything [`build`] reads.
pub struct BuildInput<'a> {
    pub document: &'a Document,
    pub info: &'a dyn RenderInfo,
    /// 012's focused element (`None` = the viewport).
    pub focused: Option<NodeId>,
    /// The window title (the root's name; the document title when empty).
    pub title: &'a str,
    /// Viewport, CSS px.
    pub viewport: (f64, f64),
    pub scale: f64,
}

impl std::fmt::Debug for BuildInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildInput")
            .field("focused", &self.focused)
            .field("title", &self.title)
            .field("viewport", &self.viewport)
            .finish_non_exhaustive()
    }
}

/// Derive the accessibility tree (pure).
pub fn build(input: &BuildInput<'_>) -> A11yTree {
    let mut b = Builder::new(input);
    let d = input.document;
    let doc = d.document_node();
    let doc_key = node_key(doc);
    let doc_title = d.title();
    // While a modal dialog is open the rest of the document is inert (015 HTML-1): the tree is the
    // dialog's subtree only, as the reference browser exposes it.
    let children = match (d.topmost_modal(), d.document_element()) {
        (Some(m), _) => b.walk(m),
        (None, Some(html)) => b.walk(html),
        (None, None) => Vec::new(),
    };
    let viewport = Bounds {
        x: 0.0,
        y: 0.0,
        w: input.viewport.0,
        h: input.viewport.1,
    };
    b.nodes.insert(
        doc_key,
        A11yNode {
            id: doc_key,
            role: Role::RootWebArea,
            tag: "#document".to_string(),
            html_id: None,
            name: doc_title.clone(),
            description: String::new(),
            value: None,
            placeholder: None,
            level: None,
            numeric: None,
            states: States::default(),
            actions: Vec::new(),
            bounds: Some(viewport),
            children,
            run: None,
            selection: None,
            structure: super::Structure::default(),
            relations: super::Relations::default(),
        },
    );
    let title = if input.title.is_empty() {
        doc_title
    } else {
        input.title.to_string()
    };
    b.nodes.insert(
        ROOT_ID,
        A11yNode {
            id: ROOT_ID,
            role: Role::Window,
            tag: "#window".to_string(),
            html_id: None,
            name: title,
            description: String::new(),
            value: None,
            placeholder: None,
            level: None,
            numeric: None,
            states: States::default(),
            actions: Vec::new(),
            bounds: Some(viewport),
            children: vec![doc_key],
            run: None,
            selection: None,
            structure: super::Structure::default(),
            relations: super::Relations::default(),
        },
    );
    // Only nodes reachable from the root stay (static text folded into a name, for one, was built
    // before its parent decided to drop it): AccessKit rejects an update with unattached nodes.
    let mut reachable = std::collections::BTreeSet::new();
    let mut stack = vec![ROOT_ID];
    while let Some(id) = stack.pop() {
        if reachable.insert(id)
            && let Some(n) = b.nodes.get(&id)
        {
            stack.extend(n.children.iter().copied());
        }
    }
    b.nodes.retain(|id, _| reachable.contains(id));
    super::metadata::decorate(d, &mut b.nodes);
    let focus = input
        .focused
        .map(node_key)
        .filter(|k| b.nodes.contains_key(k))
        .unwrap_or(doc_key);
    if let Some(n) = b.nodes.get_mut(&focus)
        && focus != doc_key
    {
        n.states.focused = true;
    }
    A11yTree {
        nodes: b.nodes,
        focus,
        scale: if input.scale > 0.0 { input.scale } else { 1.0 },
    }
}

struct Builder<'a> {
    d: &'a Document,
    info: &'a dyn RenderInfo,
    nodes: BTreeMap<u64, A11yNode>,
    /// `label[for]` by the id it names.
    labels_for: HashMap<String, Vec<NodeId>>,
    /// Element ids (first in tree order wins).
    ids: HashMap<String, NodeId>,
    referenced: std::collections::HashSet<String>,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split_ascii_whitespace()
}

fn attr_true(e: &ElementData, name: &str) -> bool {
    e.attr(name)
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

fn input_type(e: &ElementData) -> String {
    e.attr("type").unwrap_or("text").trim().to_ascii_lowercase()
}

fn parse_f64(s: Option<&str>) -> Option<f64> {
    s.and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

/// The ARIA role table (013 role-mapping §2, v1 honoured set). `None` = generic / presentational.
fn aria_role(token: &str, e: &ElementData) -> Option<Option<Role>> {
    Some(Some(match token {
        "button" => Role::Button,
        "checkbox" => Role::CheckBox,
        "switch" => Role::Switch,
        "radio" => Role::RadioButton,
        "radiogroup" => Role::RadioGroup,
        "textbox" => {
            if attr_true(e, "aria-multiline") {
                Role::MultilineTextInput
            } else {
                Role::TextInput
            }
        }
        "searchbox" => Role::SearchInput,
        "combobox" => Role::ComboBox,
        "listbox" => Role::ListBox,
        "option" => Role::ListBoxOption,
        "slider" => Role::Slider,
        "spinbutton" => Role::SpinButton,
        "progressbar" => Role::ProgressIndicator,
        "meter" => Role::Meter,
        "link" => Role::Link,
        "tab" => Role::Tab,
        "tablist" => Role::TabList,
        "tabpanel" => Role::TabPanel,
        "menu" => Role::Menu,
        "menubar" => Role::MenuBar,
        "menuitem" => Role::MenuItem,
        "menuitemcheckbox" => Role::MenuItemCheckBox,
        "menuitemradio" => Role::MenuItemRadio,
        "separator" => Role::Splitter,
        "tooltip" => Role::Tooltip,
        "heading" => Role::Heading,
        "paragraph" => Role::Paragraph,
        "list" => Role::List,
        "listitem" => Role::ListItem,
        "img" | "image" => Role::Image,
        "figure" => Role::Figure,
        "group" => Role::Group,
        "table" => Role::Table,
        "grid" => Role::Grid,
        "treegrid" => Role::TreeGrid,
        "row" => Role::Row,
        "rowgroup" => return Some(None),
        "cell" => Role::Cell,
        "gridcell" => Role::GridCell,
        "columnheader" => Role::ColumnHeader,
        "rowheader" => Role::RowHeader,
        "article" => Role::Article,
        "document" => Role::Document,
        "note" => Role::Note,
        "blockquote" => Role::Blockquote,
        "banner" => Role::Banner,
        "complementary" => Role::Complementary,
        "contentinfo" => Role::ContentInfo,
        "form" => Role::Form,
        "main" => Role::Main,
        "navigation" => Role::Navigation,
        "region" => Role::Region,
        "search" => Role::Search,
        "alert" => Role::Alert,
        "status" => Role::Status,
        "log" => Role::Log,
        "dialog" => Role::Dialog,
        "alertdialog" => Role::AlertDialog,
        "toolbar" => Role::Toolbar,
        "tree" => Role::Tree,
        "treeitem" => Role::TreeItem,
        "presentation" | "none" | "generic" => return Some(None),
        _ => return None,
    }))
}

/// Roles whose name comes from their content (accname step 2F).
fn name_from_content(r: Role) -> bool {
    matches!(
        r,
        Role::Button
            | Role::Link
            | Role::Heading
            | Role::Cell
            | Role::GridCell
            | Role::ColumnHeader
            | Role::RowHeader
            | Role::ListBoxOption
            | Role::MenuListOption
            | Role::Tab
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::Tooltip
            | Role::DisclosureTriangle
            | Role::Caption
            | Role::Legend
            | Role::FigureCaption
            | Role::Term
            | Role::TreeItem
    )
}

/// Roles whose DOM children are not walked (their content is their name or value).
fn is_leaf(r: Role) -> bool {
    matches!(
        r,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::EmailInput
            | Role::UrlInput
            | Role::PhoneNumberInput
            | Role::PasswordInput
            | Role::NumberInput
            | Role::ComboBox
            | Role::Slider
            | Role::SpinButton
            | Role::ProgressIndicator
            | Role::Meter
            | Role::Image
            | Role::ListBoxOption
            | Role::Splitter
    )
}

fn is_text_field(r: Role) -> bool {
    matches!(
        r,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::EmailInput
            | Role::UrlInput
            | Role::PhoneNumberInput
            | Role::PasswordInput
    )
}

/// Elements never in the tree (with their subtrees).
fn never_rendered(local: &str) -> bool {
    matches!(
        local,
        "head"
            | "script"
            | "style"
            | "template"
            | "noscript"
            | "meta"
            | "link"
            | "title"
            | "base"
            | "datalist"
            | "param"
            | "source"
            | "track"
    )
}

/// Tags that separate words in name-from-content even without style (a fallback for `is_block`).
fn block_tag(local: &str) -> bool {
    matches!(
        local,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "br"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "ul"
    )
}

impl<'a> Builder<'a> {
    fn new(input: &BuildInput<'a>) -> Builder<'a> {
        let d = input.document;
        let mut labels_for: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut ids: HashMap<String, NodeId> = HashMap::new();
        let mut referenced = std::collections::HashSet::new();
        for n in d.arena().subtree(d.document_node()) {
            let Some(e) = d.arena().get(n).and_then(|x| x.as_element()) else {
                continue;
            };
            for attr in [
                "aria-labelledby",
                "aria-describedby",
                "aria-controls",
                "aria-details",
                "aria-flowto",
                "aria-owns",
                "aria-activedescendant",
                "aria-errormessage",
                "headers",
            ] {
                if let Some(value) = e.attr(attr) {
                    referenced.extend(tokens(value).map(str::to_owned));
                }
            }
            if let Some(id) = e.attr("id").filter(|s| !s.is_empty()) {
                ids.entry(id.to_string()).or_insert(n);
            }
            if e.is_html_named("label")
                && let Some(f) = e.attr("for").filter(|s| !s.is_empty())
            {
                labels_for.entry(f.to_string()).or_default().push(n);
            }
        }
        Builder {
            d,
            info: input.info,
            nodes: BTreeMap::new(),
            labels_for,
            ids,
            referenced,
        }
    }

    fn el(&self, n: NodeId) -> Option<&'a ElementData> {
        self.d.arena().get(n).and_then(|x| x.as_element())
    }

    fn children(&self, n: NodeId) -> &'a [NodeId] {
        self.d.arena().get(n).map(|x| x.children()).unwrap_or(&[])
    }

    fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.d.arena().get(n).and_then(|x| x.parent())
    }

    /// Excluded with its whole subtree (`aria-hidden`, `hidden`, `display: none`, non-rendered tags).
    fn excluded(&self, n: NodeId, e: &ElementData) -> bool {
        // An inert subtree (the `inert` attribute; 015 HTML-1) is not exposed.
        if attr_true(e, "aria-hidden")
            || never_rendered(e.local_name())
            || (e.is_html() && e.attr("inert").is_some())
        {
            return true;
        }
        if e.is_html()
            && (e.attr("hidden").is_some()
                || (e.local_name() == "input" && input_type(e) == "hidden"))
        {
            return true;
        }
        self.info.rendering(n) == Rendering::NotRendered
    }

    fn walk_children(&mut self, n: NodeId) -> Vec<u64> {
        let mut out = Vec::new();
        for &c in self.children(n) {
            out.extend(self.walk(c));
        }
        out
    }

    /// The tree nodes `n` contributes to its parent: itself, or (flattened) its children.
    fn walk(&mut self, n: NodeId) -> Vec<u64> {
        let Some(node) = self.d.arena().get(n) else {
            return Vec::new();
        };
        match node.kind() {
            NodeKind::Text => self.text(n),
            NodeKind::Element => self.element(n),
            _ => Vec::new(),
        }
    }

    fn text(&mut self, n: NodeId) -> Vec<u64> {
        if let Some(p) = self.parent(n)
            && self.el(p).is_some()
            && self.info.rendering(p) != Rendering::Rendered
        {
            return Vec::new();
        }
        let text = collapse(
            self.d
                .arena()
                .get(n)
                .and_then(|x| x.character_data())
                .unwrap_or(""),
        );
        if text.is_empty() {
            return Vec::new();
        }
        let id = node_key(n);
        let bounds = self
            .info
            .bounds(n)
            .or_else(|| self.parent(n).and_then(|p| self.info.bounds(p)));
        self.nodes.insert(
            id,
            plain(id, Role::Label, "#text".to_string(), None, text, bounds),
        );
        vec![id]
    }

    fn element(&mut self, n: NodeId) -> Vec<u64> {
        let Some(e) = self.el(n) else {
            return Vec::new();
        };
        if self.excluded(n, e) {
            return Vec::new();
        }
        if !e.is_html() {
            // SVG/MathML: an image only when labelled (icons are usually aria-hidden or unlabelled).
            let mut label = e.attr("aria-label").map(collapse).unwrap_or_default();
            // F0907: an inline `<svg>` is named by its `<title>` child when it has no `aria-label`.
            if label.is_empty() && e.local_name() == "svg" {
                for &c in self.children(n) {
                    if self.el(c).is_some_and(|t| t.local_name() == "title") {
                        label =
                            collapse(&self.d.text_content(c).ok().flatten().unwrap_or_default());
                        break;
                    }
                }
            }
            let explicit = e.attr("role").map(|r| r.trim().to_ascii_lowercase());
            if !label.is_empty() || matches!(explicit.as_deref(), Some("img" | "image")) {
                let id = node_key(n);
                self.nodes.insert(
                    id,
                    plain(
                        id,
                        Role::Image,
                        e.local_name().to_string(),
                        e.attr("id").map(str::to_string),
                        label,
                        self.info.bounds(n),
                    ),
                );
                return vec![id];
            }
            return Vec::new();
        }
        if self.info.rendering(n) == Rendering::Invisible {
            return self.walk_children(n);
        }
        let local = e.local_name();
        if local == "img"
            && e.attr("alt").is_some_and(|a| a.trim().is_empty())
            && e.attr("aria-label").is_none()
            && e.attr("aria-labelledby").is_none()
        {
            // `img alt=""`: presentational, excluded.
            return Vec::new();
        }
        let focusable = crate::interaction::focus::is_focusable(self.d, n);
        let role = self.role_of(n, e, focusable);
        let Some(role) = role else {
            // Generic: flattened unless focusable, named by ARIA or a live region (its text
            // children must keep a live ancestor to be announced).
            let named = e.attr("aria-label").is_some_and(|v| !v.trim().is_empty())
                || e.attr("aria-labelledby").is_some();
            let live = e.attr("aria-live").is_some_and(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "polite" | "assertive"
                )
            });
            let semantic_boundary = e.attr("aria-busy").is_some()
                || e.attr("aria-atomic").is_some()
                || e.attr("id").is_some_and(|id| self.referenced.contains(id));
            if !(focusable || named || live || semantic_boundary) {
                return self.walk_children(n);
            }
            return self.make(n, e, Role::GenericContainer, focusable);
        };
        self.make(n, e, role, focusable)
    }

    fn make(&mut self, n: NodeId, e: &ElementData, role: Role, focusable: bool) -> Vec<u64> {
        let id = node_key(n);
        let (name, from_content) = self.name_of(n, e, role);
        // A select exposes its options in both presentations: a dropdown combobox needs its
        // selected option as a native child for the AT-SPI/UIA selection interface, the active
        // descendant and screen-reader value speech (Core 1.0 C08); its value stays the label.
        let mut children = if e.local_name() == "select" {
            self.options(n)
        } else if is_leaf(role) {
            Vec::new()
        } else {
            self.walk_children(n)
        };
        if from_content {
            // The name already carries the text: drop the duplicate static text directly below.
            children.retain(|c| self.nodes.get(c).is_none_or(|x| x.role != Role::Label));
        }
        let states = self.states(n, e, role, focusable);
        let value = self.value_of(n, e, role);
        let numeric = self.numeric_of(n, e, role);
        let actions = self.actions(n, e, role, &states);
        let level = match role {
            Role::Heading => e
                .attr("aria-level")
                .and_then(|v| v.trim().parse::<u32>().ok())
                .or_else(|| {
                    e.local_name()
                        .strip_prefix('h')
                        .and_then(|l| l.parse::<u32>().ok())
                })
                .filter(|l| (1..=9).contains(l))
                .or(Some(2)),
            _ => None,
        };
        let placeholder = if is_text_field(role) {
            e.attr("placeholder")
                .or(e.attr("aria-placeholder"))
                .map(collapse)
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        let description = self.description(e, &name);
        let bounds = self.info.bounds(n);
        let mut selection = None;
        // A number input is typed into like a text field: it carries the same text runs (Orca reads
        // a spin button's text, not only its numeric value; 1.0 F1108).
        let typed_spin = role == Role::SpinButton && e.local_name() == "input";
        if (is_text_field(role) || typed_spin)
            && let Some(v) = &value
        {
            // The text-run children AT-SPI Text/EditableText and UIA TextPattern read the text from:
            // one per laid-out line of a native control, with character geometry and the caret.
            if matches!(e.local_name(), "input" | "textarea") {
                let (runs, sel) = self.text_runs(n, id, role == Role::PasswordInput, bounds);
                children = runs;
                selection = sel;
            } else {
                let run = run_key(id, 0);
                self.nodes.insert(
                    run,
                    plain(
                        run,
                        Role::TextRun,
                        "#text-run".to_string(),
                        None,
                        v.clone(),
                        bounds,
                    ),
                );
                children = vec![run];
            }
        }
        let tag = match e.attr("id").filter(|s| !s.is_empty()) {
            Some(i) => format!("{}#{i}", e.local_name()),
            None => e.local_name().to_string(),
        };
        self.nodes.insert(
            id,
            A11yNode {
                id,
                role,
                tag,
                html_id: e.attr("id").filter(|s| !s.is_empty()).map(str::to_string),
                name,
                description,
                value,
                placeholder,
                level,
                numeric,
                states,
                actions,
                bounds,
                children,
                run: None,
                selection,
                structure: super::Structure::default(),
                relations: super::Relations::default(),
            },
        );
        vec![id]
    }

    /// The text runs of a native text control, one per laid-out line, and its selection.
    fn text_runs(
        &mut self,
        n: NodeId,
        id: u64,
        password: bool,
        bounds: Option<Bounds>,
    ) -> (Vec<u64>, Option<TextSel>) {
        let value = self.d.control_value(n).unwrap_or_default();
        // UTF-16 offset -> byte offset of the value.
        let mut u16_to_byte: Vec<(u32, usize)> = Vec::with_capacity(value.len() + 1);
        let mut u = 0u32;
        for (b, c) in value.char_indices() {
            u16_to_byte.push((u, b));
            u += c.len_utf16() as u32;
        }
        let len16 = u;
        u16_to_byte.push((len16, value.len()));
        let byte = |o: u32| {
            let i = u16_to_byte.partition_point(|&(x, _)| x < o);
            u16_to_byte.get(i).map_or(value.len(), |&(_, b)| b)
        };
        let lines = self.info.text_lines(n).unwrap_or_else(|| {
            vec![TextLine {
                start: 0,
                end: len16,
                bounds: bounds.unwrap_or_default(),
                clusters: Vec::new(),
            }]
        });
        let mut ids = Vec::new();
        let mut runs: Vec<(u64, RunText)> = Vec::new();
        for (li, line) in lines.iter().enumerate().take(MAX_RUNS) {
            let (start, end) = (line.start.min(len16), line.end.min(len16));
            // Characters: the layout's clusters, with gaps filled by the same Unicode grapheme
            // segmentation used by the editor. AccessKit's byte lengths must cover the full text.
            let mut chars: Vec<RunChar> = Vec::new();
            let mut clusters = line.clusters.clone();
            clusters.sort_by_key(|c| c.0);
            let mut at = start;
            let fill = |chars: &mut Vec<_>, from: u32, to: u32| {
                let mut o = from;
                for c in value[byte(from)..byte(to)].graphemes(true) {
                    let w = c.encode_utf16().count() as u32;
                    chars.push((o, o + w, None, false));
                    o += w;
                }
            };
            for &(s, e, x0, x1, rtl) in &clusters {
                if s < at || e <= s || e > end {
                    continue;
                }
                fill(&mut chars, at, s);
                chars.push((s, e, Some((x0, x1)), rtl));
                at = e;
            }
            fill(&mut chars, at, end);
            let hard_break = end < len16 && value[byte(end)..].starts_with('\n');
            let rb = line.bounds;
            let rtl = !chars.is_empty() && chars.iter().all(|c| c.3);
            let mut run = RunText {
                rtl,
                hard_break,
                ..RunText::default()
            };
            let mut text = String::new();
            let mut last_x = if rtl { rb.x + rb.w } else { rb.x };
            for &(s, e, ext, _) in &chars {
                let piece = if password {
                    "\u{2022}"
                } else {
                    &value[byte(s)..byte(e)]
                };
                let (x0, x1) = ext.unwrap_or((last_x, last_x));
                last_x = if rtl { x0 } else { x1 };
                let position = if rtl {
                    (rb.x + rb.w - x1) as f32
                } else {
                    (x0 - rb.x) as f32
                };
                // A pathological extended grapheme can be longer than AccessKit's u8 length
                // field. Split only that exceptional case at scalar boundaries; never truncate
                // its byte count (which creates invalid native text ranges).
                let mut remaining = piece;
                let mut offset = s;
                while !remaining.is_empty() {
                    let mut end = remaining.len().min(u8::MAX as usize);
                    while !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    let part = &remaining[..end];
                    text.push_str(part);
                    run.offsets.push(offset);
                    run.lengths.push(end as u8);
                    run.positions.push(position);
                    run.widths.push((x1 - x0) as f32);
                    offset += part.encode_utf16().count() as u32;
                    remaining = &remaining[end..];
                }
            }
            if hard_break {
                text.push('\n');
                run.offsets.push(end);
                run.lengths.push(1);
                run.positions.push(if rtl {
                    (rb.x + rb.w - last_x) as f32
                } else {
                    (last_x - rb.x) as f32
                });
                run.widths.push(0.0);
                run.offsets.push(end + 1);
            } else {
                run.offsets.push(end);
            }
            let mut byte_starts = Vec::with_capacity(run.lengths.len());
            let mut byte_start = 0;
            for &length in &run.lengths {
                byte_starts.push(byte_start);
                byte_start += usize::from(length);
            }
            for (byte, segment) in text.split_word_bound_indices() {
                if byte == 0 || segment.chars().all(char::is_whitespace) {
                    continue;
                }
                if let Ok(index) = byte_starts.binary_search(&byte)
                    && index < 256
                {
                    run.word_starts.push(index as u8);
                }
            }
            let rid = run_key(id, li);
            let mut node = plain(
                rid,
                Role::TextRun,
                "#text-run".to_string(),
                None,
                text,
                Some(rb),
            );
            node.run = Some(run.clone());
            self.nodes.insert(rid, node);
            ids.push(rid);
            runs.push((rid, run));
        }
        // No stored selection means the caret is at the end of the value (the editing engine's rule).
        let selection = self
            .d
            .control_state(n)
            .ok()
            .flatten()
            .map(|c| {
                c.selection
                    .map(|s| {
                        let backward = s.direction
                            == ui4me_document::control_state::SelectionDirection::Backward;
                        if backward {
                            (s.end, s.start)
                        } else {
                            (s.start, s.end)
                        }
                    })
                    .unwrap_or((len16, len16))
            })
            .and_then(|(anchor, focus)| {
                Some(TextSel {
                    anchor: position(&runs, anchor.min(len16))?,
                    focus: position(&runs, focus.min(len16))?,
                    value: (anchor.min(len16), focus.min(len16)),
                })
            });
        (ids, selection)
    }

    /// Native role (HTML-AAM) after an explicit ARIA role.
    fn role_of(&self, n: NodeId, e: &ElementData, focusable: bool) -> Option<Role> {
        if let Some(r) = e.attr("role") {
            for t in tokens(r) {
                if let Some(mapped) = aria_role(&t.to_ascii_lowercase(), e) {
                    // `presentation`/`none` on a focusable element is ignored (ARIA conflict resolution).
                    if mapped.is_some() || !focusable {
                        return mapped;
                    }
                    break;
                }
            }
        }
        // A plain-text editing host is a multi-line text field (0.8 F0801).
        if crate::document::editable::is_editing_host(self.d, n) {
            return Some(Role::MultilineTextInput);
        }
        let has_name = || {
            e.attr("aria-label").is_some_and(|v| !v.trim().is_empty())
                || e.attr("aria-labelledby").is_some()
        };
        let landmark_context = || {
            let mut p = self.parent(n);
            while let Some(x) = p {
                if let Some(pe) = self.el(x)
                    && matches!(
                        pe.local_name(),
                        "article" | "aside" | "main" | "nav" | "section"
                    )
                {
                    return false;
                }
                p = self.parent(x);
            }
            true
        };
        Some(match e.local_name() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Role::Heading,
            "a" | "area" if e.attr("href").is_some() => Role::Link,
            "button" => Role::Button,
            "input" => match input_type(e).as_str() {
                "button" | "submit" | "reset" | "image" | "file" => Role::Button,
                "checkbox" => Role::CheckBox,
                "radio" => Role::RadioButton,
                "range" => Role::Slider,
                "number" => Role::SpinButton,
                "search" => Role::SearchInput,
                "email" => Role::EmailInput,
                "url" => Role::UrlInput,
                "tel" => Role::PhoneNumberInput,
                "password" => Role::PasswordInput,
                _ => Role::TextInput,
            },
            "textarea" => Role::MultilineTextInput,
            "select" => {
                if self.d.select_is_dropdown(n) {
                    Role::ComboBox
                } else {
                    Role::ListBox
                }
            }
            "option" => Role::ListBoxOption,
            "optgroup" => Role::Group,
            "progress" => Role::ProgressIndicator,
            "meter" => Role::Meter,
            "ul" | "ol" | "menu" => Role::List,
            "li" => Role::ListItem,
            "table" => Role::Table,
            "tr" => Role::Row,
            "td" => Role::Cell,
            "th" => {
                if e.attr("scope")
                    .is_some_and(|s| s.trim().eq_ignore_ascii_case("row"))
                {
                    Role::RowHeader
                } else {
                    Role::ColumnHeader
                }
            }
            "caption" => Role::Caption,
            "dialog" => Role::Dialog,
            "nav" => Role::Navigation,
            "main" => Role::Main,
            "header" if landmark_context() => Role::Banner,
            "footer" if landmark_context() => Role::ContentInfo,
            "aside" => Role::Complementary,
            "section" if has_name() => Role::Region,
            "form" if has_name() => Role::Form,
            "search" => Role::Search,
            "article" => Role::Article,
            "img" => Role::Image,
            "p" => Role::Paragraph,
            "blockquote" => Role::Blockquote,
            "figure" => Role::Figure,
            "figcaption" => Role::FigureCaption,
            "fieldset" => Role::Group,
            "legend" => Role::Legend,
            "details" => Role::Details,
            "summary" => Role::DisclosureTriangle,
            "dl" => Role::DescriptionList,
            "dt" => Role::Term,
            "dd" => Role::Definition,
            "hr" => Role::Splitter,
            _ => return None,
        })
    }

    /// `option` children of a list box or of a dropdown combobox (whose value is also the selected
    /// label). Dropdown options have no box and report no bounds.
    fn options(&mut self, select: NodeId) -> Vec<u64> {
        let mut out = Vec::new();
        let dropdown = self.d.select_is_dropdown(select);
        for o in self.d.select_options(select) {
            let Some(e) = self.el(o) else { continue };
            if e.attr("hidden").is_some() || attr_true(e, "aria-hidden") {
                continue;
            }
            let id = node_key(o);
            let mut node = plain(
                id,
                Role::ListBoxOption,
                "option".to_string(),
                e.attr("id").map(str::to_string),
                collapse(&self.d.option_label(o)),
                // Dropdown options have no box; never let them shadow the select in hit tests.
                if dropdown {
                    None
                } else {
                    self.info.bounds(o).or_else(|| self.info.bounds(select))
                },
            );
            node.states.selected = Some(self.d.option_selected(o));
            node.states.disabled = self.d.option_disabled(o);
            if !node.states.disabled {
                node.actions = vec![A11yAction::Click];
            }
            self.nodes.insert(id, node);
            out.push(id);
        }
        out
    }

    // ------------------------------------------------------------------ names

    fn by_id(&self, id: &str) -> Option<NodeId> {
        self.ids.get(id).copied()
    }

    /// The name and whether it came from content (accname 1.2 subset, role-mapping §3).
    fn name_of(&self, n: NodeId, e: &ElementData, role: Role) -> (String, bool) {
        if let Some(ids) = e.attr("aria-labelledby") {
            let parts: Vec<String> = tokens(ids)
                .filter_map(|i| self.by_id(i))
                .map(|r| self.text_of(r, true, 0))
                .filter(|s| !s.is_empty())
                .collect();
            let s = collapse(&parts.join(" "));
            if !s.is_empty() {
                return (s, false);
            }
        }
        if let Some(l) = e.attr("aria-label").map(collapse).filter(|s| !s.is_empty()) {
            return (l, false);
        }
        let native = self.native_name(n, e);
        if !native.is_empty() {
            return (native, false);
        }
        if name_from_content(role) {
            let s = collapse(&self.content_text(n, 0));
            if !s.is_empty() {
                return (s, true);
            }
        }
        if let Some(t) = e.attr("title").map(collapse).filter(|s| !s.is_empty()) {
            return (t, false);
        }
        if (is_text_field(role) || role == Role::SpinButton)
            && let Some(p) = e
                .attr("placeholder")
                .or(e.attr("aria-placeholder"))
                .map(collapse)
                .filter(|s| !s.is_empty())
        {
            return (p, false);
        }
        (String::new(), false)
    }

    /// Host-language label (role-mapping §1 "Name source").
    fn native_name(&self, n: NodeId, e: &ElementData) -> String {
        let local = e.local_name();
        match local {
            "input" => {
                let ty = input_type(e);
                match ty.as_str() {
                    "submit" | "reset" | "button" => {
                        if let Some(v) = e.attr("value") {
                            return collapse(v);
                        }
                        return match ty.as_str() {
                            "submit" => "Submit".to_string(),
                            "reset" => "Reset".to_string(),
                            _ => String::new(),
                        };
                    }
                    "image" => {
                        return e
                            .attr("alt")
                            .or(e.attr("value"))
                            .map(collapse)
                            .unwrap_or_else(|| "Submit".to_string());
                    }
                    _ => {}
                }
                self.labels_text(n, e)
            }
            "textarea" | "select" | "progress" | "meter" | "button" => self.labels_text(n, e),
            "img" | "area" => e.attr("alt").map(collapse).unwrap_or_default(),
            "fieldset" => self
                .first_child_named(n, "legend")
                .map(|l| collapse(&self.content_text(l, 0)))
                .unwrap_or_default(),
            "table" => self
                .first_child_named(n, "caption")
                .map(|c| collapse(&self.content_text(c, 0)))
                .unwrap_or_default(),
            "figure" => self
                .first_child_named(n, "figcaption")
                .map(|c| collapse(&self.content_text(c, 0)))
                .unwrap_or_default(),
            "optgroup" => e.attr("label").map(collapse).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn first_child_named(&self, n: NodeId, local: &str) -> Option<NodeId> {
        self.children(n)
            .iter()
            .copied()
            .find(|c| self.el(*c).is_some_and(|e| e.is_html_named(local)))
    }

    /// `label[for=id]` elements and the wrapping `label`, joined; the control's own text is not part of it.
    fn labels_text(&self, n: NodeId, e: &ElementData) -> String {
        let mut labels: Vec<NodeId> = e
            .attr("id")
            .and_then(|i| self.labels_for.get(i))
            .cloned()
            .unwrap_or_default();
        let mut p = self.parent(n);
        while let Some(x) = p {
            if self.el(x).is_some_and(|pe| pe.is_html_named("label")) {
                if !labels.contains(&x) {
                    labels.push(x);
                }
                break;
            }
            p = self.parent(x);
        }
        let parts: Vec<String> = labels
            .into_iter()
            .map(|l| collapse(&self.label_text(l, n)))
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" ")
    }

    /// A label's text without the labelled control.
    fn label_text(&self, label: NodeId, control: NodeId) -> String {
        let mut s = String::new();
        for &c in self.children(label) {
            if c != control {
                s.push_str(&self.text_of(c, false, 1));
            }
        }
        s
    }

    /// Name-from-content text of the children of `n`.
    fn content_text(&self, n: NodeId, depth: u32) -> String {
        let mut s = String::new();
        for &c in self.children(n) {
            s.push_str(&self.text_of(c, false, depth + 1));
        }
        s
    }

    /// Text alternative of one node in a traversal (accname 2C-2G, simplified). `root_hidden_ok`: the
    /// root of an `aria-labelledby` reference counts even when hidden.
    fn text_of(&self, n: NodeId, root_hidden_ok: bool, depth: u32) -> String {
        if depth > 64 {
            return String::new();
        }
        let Some(node) = self.d.arena().get(n) else {
            return String::new();
        };
        match node.kind() {
            NodeKind::Text => node.character_data().unwrap_or("").to_string(),
            NodeKind::Element => {
                let Some(e) = node.as_element() else {
                    return String::new();
                };
                let hidden = attr_true(e, "aria-hidden")
                    || never_rendered(e.local_name())
                    || e.attr("hidden").is_some()
                    || self.info.rendering(n) != Rendering::Rendered;
                if hidden && !(root_hidden_ok && depth == 0) {
                    return String::new();
                }
                if depth > 0
                    && let Some(l) = e.attr("aria-label").map(collapse).filter(|s| !s.is_empty())
                {
                    return format!(" {l} ");
                }
                let local = e.local_name();
                let sep = |s: String| {
                    if block_tag(local) || self.info.is_block(n) {
                        format!(" {s} ")
                    } else {
                        s
                    }
                };
                match local {
                    "img" | "area" => return format!(" {} ", e.attr("alt").unwrap_or("")),
                    "br" => return " ".to_string(),
                    "input" => {
                        let ty = input_type(e);
                        return match ty.as_str() {
                            "submit" | "reset" | "button" => {
                                format!(" {} ", e.attr("value").unwrap_or(""))
                            }
                            "checkbox" | "radio" | "hidden" | "file" | "image" | "range" => {
                                String::new()
                            }
                            _ => format!(" {} ", self.d.control_value(n).unwrap_or_default()),
                        };
                    }
                    "textarea" => {
                        return format!(" {} ", self.d.control_value(n).unwrap_or_default());
                    }
                    "select" => return format!(" {} ", self.d.select_display_label(n)),
                    _ => {}
                }
                if !e.is_html() {
                    return String::new();
                }
                sep(self.content_text(n, depth))
            }
            _ => String::new(),
        }
    }

    fn description(&self, e: &ElementData, name: &str) -> String {
        if let Some(ids) = e.attr("aria-describedby") {
            let parts: Vec<String> = tokens(ids)
                .filter_map(|i| self.by_id(i))
                .map(|r| self.text_of(r, true, 0))
                .collect();
            let s = collapse(&parts.join(" "));
            if !s.is_empty() {
                return s;
            }
        }
        if let Some(d) = e
            .attr("aria-description")
            .map(collapse)
            .filter(|s| !s.is_empty())
        {
            return d;
        }
        match e.attr("title").map(collapse) {
            Some(t) if !t.is_empty() && t != name => t,
            _ => String::new(),
        }
    }

    // ------------------------------------------------------------------ states and values

    fn disabled(&self, n: NodeId, e: &ElementData) -> bool {
        if attr_true(e, "aria-disabled")
            || crate::interaction::activation::is_disabled_form_control(self.d, n)
        {
            return true;
        }
        if !matches!(e.local_name(), "button" | "input" | "select" | "textarea") {
            return false;
        }
        // A disabled fieldset disables its controls (except inside its first legend).
        let mut child = n;
        let mut p = self.parent(n);
        while let Some(x) = p {
            if let Some(pe) = self.el(x)
                && pe.is_html_named("fieldset")
                && pe.attr("disabled").is_some()
            {
                let first_legend = self.first_child_named(x, "legend");
                if first_legend != Some(child) {
                    return true;
                }
            }
            child = x;
            p = self.parent(x);
        }
        false
    }

    fn states(&self, n: NodeId, e: &ElementData, role: Role, focusable: bool) -> States {
        let disabled = self.disabled(n, e);
        let cs = self.d.control_state(n).ok().flatten();
        let local = e.local_name();
        let is_input = |t: &str| local == "input" && input_type(e) == t;
        let checked = if is_input("checkbox") {
            cs.map(|c| {
                if c.indeterminate {
                    Checked::Mixed
                } else if c.checkedness.unwrap_or(false) {
                    Checked::True
                } else {
                    Checked::False
                }
            })
        } else if is_input("radio") {
            cs.map(|c| {
                if c.checkedness.unwrap_or(false) {
                    Checked::True
                } else {
                    Checked::False
                }
            })
        } else if matches!(
            role,
            Role::CheckBox
                | Role::RadioButton
                | Role::Switch
                | Role::MenuItemCheckBox
                | Role::MenuItemRadio
        ) {
            Some(
                match e
                    .attr("aria-checked")
                    .map(|v| v.trim().to_ascii_lowercase())
                    .as_deref()
                {
                    Some("true") => Checked::True,
                    Some("mixed") => Checked::Mixed,
                    _ => Checked::False,
                },
            )
        } else if role == Role::Button {
            match e
                .attr("aria-pressed")
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("true") => Some(Checked::True),
                Some("mixed") => Some(Checked::Mixed),
                Some("false") => Some(Checked::False),
                _ => None,
            }
        } else {
            None
        };
        let expanded = if local == "select" && role == Role::ComboBox {
            Some(cs.is_some_and(|c| c.picker_highlight.is_some()))
        } else if local == "summary" {
            self.parent(n)
                .and_then(|p| self.el(p))
                .filter(|p| p.is_html_named("details"))
                .map(|p| p.attr("open").is_some())
        } else {
            match e
                .attr("aria-expanded")
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("true") => Some(true),
                Some("false") => Some(false),
                // A `popovertarget` invoker exposes whether its popover is showing (F0808).
                _ => crate::interaction::popover::invoker_target(self.d, n)
                    .map(|(t, _)| crate::interaction::popover::is_open(self.d, t)),
            }
        };
        let selected = if local == "option" {
            Some(self.d.option_selected(n))
        } else {
            match e
                .attr("aria-selected")
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("true") => Some(true),
                Some("false") => Some(false),
                _ => None,
            }
        };
        let required = (matches!(local, "input" | "select" | "textarea")
            && e.attr("required").is_some())
            || attr_true(e, "aria-required");
        let aria_invalid = e
            .attr("aria-invalid")
            .map(|v| v.trim().to_ascii_lowercase())
            .is_some_and(|v| !v.is_empty() && v != "false");
        let user_invalid = cs.is_some_and(|c| {
            c.user_validity
                && c.validity_flags & ui4me_document::control_state::validity::VALID == 0
                && c.validity_flags != 0
        });
        let read_only = ((is_text_field(role) || role == Role::SpinButton)
            && local != "div"
            && e.attr("readonly").is_some())
            || attr_true(e, "aria-readonly");
        // `showModal()` makes a dialog modal (015 HTML-1), as `aria-modal` does.
        let modal = matches!(role, Role::Dialog | Role::AlertDialog)
            && (attr_true(e, "aria-modal")
                || e.ui_flags()
                    .contains(ui4me_document::ui_state::UiFlags::MODAL));
        let multiselectable = (local == "select" && self.d.select_is_multiple(n))
            || attr_true(e, "aria-multiselectable");
        let live = match e
            .attr("aria-live")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("polite") => LivePoliteness::Polite,
            Some("assertive") => LivePoliteness::Assertive,
            Some("off") => LivePoliteness::Off,
            // ARIA implicit values: status and log are polite live regions, alert is assertive.
            _ => match role {
                Role::Status | Role::Log => LivePoliteness::Polite,
                Role::Alert => LivePoliteness::Assertive,
                _ => LivePoliteness::Off,
            },
        };
        States {
            focusable: focusable && !disabled,
            focused: false,
            disabled,
            checked,
            expanded,
            selected,
            required,
            invalid: aria_invalid
                || user_invalid
                || (self.d.will_validate(n)
                    && cs.is_some_and(|c| !c.custom_validity_message.is_empty())),
            read_only,
            modal,
            multiselectable,
            live,
            busy: attr_true(e, "aria-busy"),
            live_atomic: e
                .attr("aria-atomic")
                .map(|v| v.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(matches!(role, Role::Status | Role::Alert)),
        }
    }

    fn value_of(&self, n: NodeId, e: &ElementData, role: Role) -> Option<String> {
        let local = e.local_name();
        if is_text_field(role) {
            if matches!(local, "input" | "textarea") {
                let v = self.d.control_value(n).unwrap_or_default();
                return Some(if role == Role::PasswordInput {
                    "\u{2022}".repeat(v.chars().count())
                } else {
                    v
                });
            }
            // ARIA textbox: its text content.
            return Some(collapse(&self.content_text(n, 0)));
        }
        match role {
            Role::ComboBox if local == "select" => Some(self.d.select_display_label(n)),
            Role::Slider | Role::SpinButton if local == "input" => self
                .d
                .range_value(n)
                .map(ui4me_document::control_values::format_float)
                .or_else(|| self.d.control_value(n)),
            Role::Slider | Role::SpinButton | Role::ProgressIndicator | Role::Meter => {
                e.attr("aria-valuetext").map(collapse)
            }
            _ => None,
        }
    }

    fn numeric_of(&self, n: NodeId, e: &ElementData, role: Role) -> Option<Numeric> {
        let local = e.local_name();
        match (local, role) {
            ("input", Role::Slider | Role::SpinButton) => {
                let m = self.d.input_step_model(n)?;
                Some(Numeric {
                    now: self.d.input_value_as_number(n),
                    min: m.min,
                    max: m.max,
                    step: m.step,
                })
            }
            ("progress", _) => {
                let max = parse_f64(e.attr("max")).filter(|m| *m > 0.0).unwrap_or(1.0);
                // A negative or unparsable `value` is invalid: indeterminate (no `now`).
                let now = parse_f64(e.attr("value"))
                    .filter(|v| *v >= 0.0)
                    .map(|v| v.min(max));
                Some(Numeric {
                    now,
                    min: Some(0.0),
                    max: Some(max),
                    step: None,
                })
            }
            ("meter", _) => {
                let min = parse_f64(e.attr("min")).unwrap_or(0.0);
                let max = parse_f64(e.attr("max")).unwrap_or(1.0).max(min);
                let now = parse_f64(e.attr("value")).unwrap_or(0.0).clamp(min, max);
                Some(Numeric {
                    now: Some(now),
                    min: Some(min),
                    max: Some(max),
                    step: None,
                })
            }
            (_, Role::Slider | Role::SpinButton | Role::ProgressIndicator | Role::Meter) => {
                Some(Numeric {
                    now: parse_f64(e.attr("aria-valuenow")),
                    min: parse_f64(e.attr("aria-valuemin")),
                    max: parse_f64(e.attr("aria-valuemax")),
                    step: None,
                })
            }
            _ => None,
        }
    }

    fn actions(&self, n: NodeId, e: &ElementData, role: Role, s: &States) -> Vec<A11yAction> {
        let mut out = Vec::new();
        if s.disabled {
            return out;
        }
        if s.focusable {
            out.push(A11yAction::Focus);
        }
        let clickable_role = matches!(
            role,
            Role::Button
                | Role::Link
                | Role::CheckBox
                | Role::RadioButton
                | Role::Switch
                | Role::Tab
                | Role::MenuItem
                | Role::MenuItemCheckBox
                | Role::MenuItemRadio
                | Role::ListBoxOption
                | Role::DisclosureTriangle
                | Role::ComboBox
        );
        if clickable_role
            || crate::interaction::activation::has_activation_behaviour(self.d, n)
            || self.info.has_listeners(n)
        {
            out.push(A11yAction::Click);
            // A clickable node with an expanded state (aria-expanded, <summary>, an open select) is
            // expanded / collapsed by activating it (UIA ExpandCollapse; beta Windows defect D1).
            if s.expanded.is_some() {
                out.push(A11yAction::Expand);
                out.push(A11yAction::Collapse);
            }
        }
        let local = e.local_name();
        let native_field = matches!(local, "input" | "textarea");
        if !s.read_only
            && native_field
            && (is_text_field(role) || matches!(role, Role::Slider | Role::SpinButton))
        {
            out.push(A11yAction::SetValue);
        }
        if native_field && is_text_field(role) {
            out.push(A11yAction::SetTextSelection);
            if !s.read_only {
                out.push(A11yAction::ReplaceSelectedText);
            }
        }
        if !s.read_only && local == "input" && matches!(role, Role::Slider | Role::SpinButton) {
            out.push(A11yAction::Increment);
            out.push(A11yAction::Decrement);
        }
        out
    }
}

/// The AccessKit position of UTF-16 offset `o` among a field's runs: a soft-wrapped line's end is
/// the next line's start (downstream); at the end of a line with a hard break the position is on
/// the break character (AccessKit `character_lengths`).
fn position(runs: &[(u64, RunText)], o: u32) -> Option<(u64, usize)> {
    for (i, (id, r)) in runs.iter().enumerate() {
        let chars = r.offsets.len() - 1;
        let text_end = if r.hard_break {
            r.offsets[chars - 1]
        } else {
            r.offsets[chars]
        };
        let last = i + 1 == runs.len();
        if o < text_end || (o == text_end && (r.hard_break || last)) {
            let index = r.offsets[..chars].partition_point(|&x| x < o);
            return Some((*id, index));
        }
    }
    runs.last().map(|(id, r)| (*id, r.offsets.len() - 1))
}

/// A node with a role, name and bounds only.
fn plain(
    id: u64,
    role: Role,
    tag: String,
    html_id: Option<String>,
    name: String,
    bounds: Option<Bounds>,
) -> A11yNode {
    let value = matches!(role, Role::TextRun).then(|| name.clone());
    A11yNode {
        id,
        role,
        tag,
        html_id,
        name: if role == Role::TextRun {
            String::new()
        } else {
            name
        },
        description: String::new(),
        value,
        placeholder: None,
        level: None,
        numeric: None,
        states: States::default(),
        actions: Vec::new(),
        bounds,
        children: Vec::new(),
        run: None,
        selection: None,
        structure: super::Structure::default(),
        relations: super::Relations::default(),
    }
}
