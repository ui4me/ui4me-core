//! owner: 008-layout-engine (lane L-LIST-r9) - generated boxes: `::before`, `::after` and
//! `::marker` (FR-012, FR-082, FR-086, FR-089; T112, T115).
//!
//! `::before` / `::after` become the first and last children of the originating element's box,
//! only when their `content` is neither `none` nor `normal`; their boxes carry the owner
//! `Pseudo(element, kind)` and never a DOM identity. `::marker` is generated for each list item:
//! an `inside` marker is the first inline box of the item; an `outside` marker is an atomic
//! inline in the item's first line whose margin box is zero wide (`atomic.rs`), so it hangs
//! outside the line start edge, aligned to the first line's baseline, and a list item without
//! other content still has a line for it. No `::first-letter` box is ever generated (OD-20).

use crate::style::view::generated::{Content, ContentItem, CounterStyleRef};
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

use super::anonymous::{only_collapsible, wrap_inline_runs};
use super::build::{BoxOwner, BoxTag, Builder, Proto, static_feature};
use super::lists;
use super::{
    BoxStyle, CollapseMode, DisplayValue, InnerDisplay, MarkerPlacement, PseudoKind, StyledSource,
    TableTag, WhiteSpaceValue, table_fixup,
};

impl<S: StyledSource> Builder<'_, S> {
    /// Apply an element's counter operations and open the counter frame of its children.
    pub(crate) fn enter_counters(&mut self, node: NodeId, style: &BoxStyle) {
        self.counters.begin();
        self.apply_counter_ops(node, style, style.display == DisplayValue::ListItem);
        // Style containment scopes the counter changes and the quote depth of the element's subtree
        // (its pseudo-elements and descendants); the element's own counter properties still act on the
        // counters in scope, as in the reference browser (measured, lane L-CQ D-LCQ-07).
        self.counters.enter_element(style.containment.style);
        self.quotes.enter(style.containment.style);
        self.counters.push();
    }

    pub(crate) fn leave_counters(&mut self) {
        self.counters.pop();
        self.quotes.leave();
        self.counters.leave_element();
        self.counters.end();
    }

    /// `counter-reset`, then `counter-set` (the HTML `li` `value` attribute), then
    /// `counter-increment`, then the implicit `list-item` increment of a list item (CSS Lists 3
    /// section 4.4).
    fn apply_counter_ops(&mut self, node: NodeId, style: &BoxStyle, list_item: bool) {
        self.apply_counter_ops_in(node, style, list_item, true)
    }

    /// `element` is false for a pseudo-element (it has no following siblings in scope).
    pub(crate) fn apply_counter_ops_in(
        &mut self,
        node: NodeId,
        style: &BoxStyle,
        list_item: bool,
        element: bool,
    ) {
        let g = style.generated.clone();
        let li_value = if list_item && element {
            self.li_value(node, style)
        } else {
            None
        };
        for op in &g.counter_reset {
            if li_value.is_some() && op.name == "list-item" && !op.reversed {
                // The presentational hint of the `value` attribute (006 emits it as a
                // `counter-reset` one below the value because `counter-set` is not available in the
                // style engine): it sets the nearest counter, it does not create one (below).
                continue;
            }
            // `reversed(name)` without an integer starts at the value that makes the items in the
            // counter's scope count down to their own increments (CSS Lists 3 section 4.4.1).
            let value = match op.value {
                Some(v) => v,
                None if element => self.reversed_initial(node, style, &op.name),
                None => 1,
            };
            self.counters.instantiate(&op.name, value, op.reversed);
        }
        let incremented_list_item = g.counter_increment.iter().any(|o| o.name == "list-item");
        if let Some(v) = li_value {
            // The ordinal value is what the item displays: the counter is set before the
            // increments so that it ends on `v`.
            let by: i32 = g
                .counter_increment
                .iter()
                .filter(|o| o.name == "list-item")
                .map(|o| o.value.unwrap_or(1))
                .sum();
            self.counters.set_before_increment(
                "list-item",
                v,
                if incremented_list_item {
                    Some(by)
                } else {
                    None
                },
            );
        }
        for op in &g.counter_increment {
            self.counters.increment(&op.name, op.value.unwrap_or(1));
        }
        if list_item && !incremented_list_item {
            self.counters.increment_implicit("list-item");
        }
    }

    /// The value of the `value` attribute of an `li` whose style carries its presentational hint.
    fn li_value(&self, node: NodeId, style: &BoxStyle) -> Option<i32> {
        if self.src.tag_name(node) != Some("li") {
            return None;
        }
        let v: i32 = self.src.attr(node, "value")?.trim().parse().ok()?;
        style
            .generated
            .counter_reset
            .iter()
            .any(|o| o.name == "list-item" && o.value == Some(v.saturating_sub(1)))
            .then_some(v)
    }

    /// The initial value of a reversed counter `name` created on `node` with no integer. The
    /// counter's scope is the element, its descendants, its following siblings and their
    /// descendants (up to a sibling that creates the same counter again). Taking those elements
    /// in tree order: when one of them sets the counter (`li value`) the first such setter fixes
    /// the value (what the elements before it removed is added back); otherwise the items count
    /// down so that the last one ends on its own increment (1 for the default).
    fn reversed_initial(&self, node: NodeId, style: &BoxStyle, name: &str) -> i32 {
        struct Ev {
            set: Option<i32>,
            inc: i32,
        }
        fn own<S: StyledSource>(
            b: &Builder<'_, S>,
            node: NodeId,
            st: &BoxStyle,
            name: &str,
            out: &mut Vec<Ev>,
        ) {
            let g = &st.generated;
            let explicit: Vec<i32> = g
                .counter_increment
                .iter()
                .filter(|o| o.name == name)
                .map(|o| o.value.unwrap_or(1))
                .collect();
            let li = name == "list-item" && st.display == DisplayValue::ListItem;
            let inc = if explicit.is_empty() {
                if li { -1 } else { 0 }
            } else {
                explicit.iter().sum()
            };
            let set = if li {
                b.li_value(node, st).map(|v| v - inc)
            } else {
                None
            };
            if inc != 0 || set.is_some() {
                out.push(Ev { set, inc });
            }
        }
        fn walk<S: StyledSource>(
            b: &Builder<'_, S>,
            children: &[NodeId],
            name: &str,
            out: &mut Vec<Ev>,
        ) {
            for &c in children {
                let super::SourceNode::Element(st) = b.src.node(c) else {
                    continue;
                };
                if st.display == DisplayValue::None {
                    continue;
                }
                let contents = st.display == DisplayValue::Contents;
                let li_hack = name == "list-item" && b.li_value(c, st).is_some();
                if !contents
                    && st
                        .generated
                        .counter_reset
                        .iter()
                        .any(|o| o.name == name && !(li_hack && !o.reversed))
                {
                    // The same counter is created again: it replaces this one for the rest of
                    // the sibling list.
                    return;
                }
                if !contents {
                    own(b, c, st, name, out);
                }
                walk(
                    b,
                    &super::html_rendering::rendered_children(b.src, c),
                    name,
                    out,
                );
            }
        }
        let mut evs = Vec::new();
        own(self, node, style, name, &mut evs);
        walk(
            self,
            &super::html_rendering::rendered_children(self.src, node),
            name,
            &mut evs,
        );
        if let Some((sibs, from)) = self.later_siblings.last() {
            walk(self, &sibs[(*from).min(sibs.len())..], name, &mut evs);
        }
        if evs.is_empty() {
            return 1;
        }
        match evs.iter().position(|e| e.set.is_some()) {
            Some(i) => {
                let before: i32 = evs[..i].iter().map(|e| e.inc).sum();
                evs[i].set.unwrap_or(0).saturating_sub(before)
            }
            None => {
                let total: i32 = evs.iter().map(|e| e.inc).sum();
                let last = evs.last().map_or(0, |e| e.inc);
                0i32.saturating_sub(total).saturating_sub(last)
            }
        }
    }

    /// The text of a `content` value for `node`: strings, `counter()`, `counters()`, `attr()` and the
    /// quote keywords. Images are not rendered (reported).
    fn content_text(&mut self, node: NodeId, style: &BoxStyle, items: &[ContentItem]) -> String {
        let mut out = String::new();
        for item in items {
            match item {
                ContentItem::String(s) => out.push_str(s),
                ContentItem::Counter { name, style: cs } => {
                    self.report_counter_style(node, cs);
                    let v = self.counters.value(name);
                    out.push_str(&lists::counter_text(cs, v));
                }
                ContentItem::Counters {
                    name,
                    separator,
                    style: cs,
                } => {
                    self.report_counter_style(node, cs);
                    let vs = self.counters.values(name);
                    let parts: Vec<String> =
                        vs.into_iter().map(|v| lists::counter_text(cs, v)).collect();
                    out.push_str(&parts.join(separator));
                }
                ContentItem::OpenQuote
                | ContentItem::CloseQuote
                | ContentItem::NoOpenQuote
                | ContentItem::NoCloseQuote => {
                    let q = self
                        .quotes
                        .quote(item, &style.generated.quotes, &style.text.font.lang);
                    out.push_str(&q);
                }
                ContentItem::Image(_) => {
                    self.report(feature("css.value.content-image"), Stage::Box, node);
                }
            }
        }
        out
    }

    fn report_counter_style(&mut self, node: NodeId, cs: &CounterStyleRef) {
        if let Some(name) = lists::unsupported_name(cs) {
            self.report(
                feature(&format!("css.value.list-style-type.{name}")),
                Stage::Box,
                node,
            );
        }
    }

    /// `::before` or `::after` of `node`, pushed on `out` (FR-012). Nothing when the pseudo-element
    /// has no style, `content` is `none` or `normal`, or `display` is `none`.
    pub(crate) fn pseudo_element(
        &mut self,
        node: NodeId,
        kind: PseudoKind,
        parent_style: &BoxStyle,
        out: &mut Vec<Proto>,
    ) {
        let Some(style) = self.src.pseudo(node, kind).cloned() else {
            return;
        };
        let Content::Items(items) = &style.generated.content else {
            return;
        };
        let items = items.clone();
        if style.display == DisplayValue::None {
            return;
        }
        self.counters.begin();
        self.apply_counter_ops_in(node, &style, false, false);
        // `content: <url>` alone: the pseudo-element is an ordinary box holding one anonymous inline
        // replaced box, the image at its natural size (CSS Content 3 section 3; WPT
        // `pseudo-element-inline-box`: the border of an inline pseudo-element wraps the line, not the image).
        // Images mixed with other items stay reported (`content_text`). A failed image leaves the box empty.
        let image = match items.as_slice() {
            [ContentItem::Image(crate::style::view::paint_groups::Image::Url(_))] => {
                let property = if kind == PseudoKind::Before {
                    "content::before"
                } else {
                    "content::after"
                };
                self.volatile += 1;
                Some(self.src.css_image_state(node, property))
            }
            _ => None,
        };
        let text = if image.is_some() {
            String::new()
        } else {
            self.content_text(node, &style, &items)
        };
        self.counters.end();
        let mut style = style;
        style.decorations = super::build::propagate_decorations(parent_style, &style);
        // 015 TXT-5: the generated content's style on the first line of a block with `::first-line`.
        let fl_kind = if kind == PseudoKind::Before {
            PseudoKind::FirstLineBefore
        } else {
            PseudoKind::FirstLineAfter
        };
        if let Some(fl) = self.src.pseudo(node, fl_kind) {
            let mut fl = fl.clone();
            let base = parent_style
                .first_line
                .as_ref()
                .map_or_else(|| style.decorations.clone(), |p| p.decorations.clone());
            fl.decorations = super::build::first_line_decorations(&base, &fl);
            style.first_line = Some(std::sync::Arc::new(fl));
        }
        let children = match image {
            Some(state)
                if !matches!(state, Some(ui4me_assets::view::ImageState::Broken { .. })) =>
            {
                let info = crate::layout::replaced::images::ImageInfo {
                    alt: None,
                    attr_size: None,
                    state,
                };
                let content = crate::layout::replaced::ReplacedContent {
                    block_level: false,
                    kind: crate::layout::replaced::ReplacedKind::Image(info),
                    inner: None,
                };
                vec![Proto::Replaced {
                    node,
                    pseudo: Some(kind),
                    style: BoxStyle::inheriting(&style, DisplayValue::Inline),
                    content,
                }]
            }
            Some(_) => Vec::new(),
            None if text.is_empty() => Vec::new(),
            None => vec![Proto::Text {
                text,
                style: style.clone(),
                owner: node,
                pseudo: Some(kind),
            }],
        };
        let owner = BoxOwner::Pseudo(node, kind);
        let display = self.display(node, &style);
        match display {
            DisplayValue::None => {}
            DisplayValue::Contents => out.extend(children),
            DisplayValue::Table | DisplayValue::InlineTable => {
                let kids = table_fixup::fix_table_children(node, &style, children);
                out.push(table_fixup::assemble(
                    BoxOwner::Anonymous(node),
                    owner,
                    style,
                    kids,
                    display == DisplayValue::InlineTable,
                ));
            }
            DisplayValue::TableCell => {
                let tag = BoxTag::Table(TableTag::Cell {
                    col_span: 1,
                    row_span: 1,
                });
                let kids = wrap_inline_runs(node, &style, children, false);
                out.push(Proto::Block {
                    owner,
                    style,
                    children: kids,
                    inner: InnerDisplay::FlowRoot,
                    list_item: false,
                    tag,
                    atomic: false,
                });
            }
            DisplayValue::TableCaption => {
                let kids = wrap_inline_runs(node, &style, children, false);
                out.push(Proto::Block {
                    owner,
                    style,
                    children: kids,
                    inner: InnerDisplay::FlowRoot,
                    list_item: false,
                    tag: BoxTag::Table(TableTag::Caption),
                    atomic: false,
                });
            }
            d if d.is_block_level() || d.is_atomic_inline() => {
                let inner = d.inner().unwrap_or(InnerDisplay::Flow);
                let always = matches!(inner, InnerDisplay::Flex | InnerDisplay::Grid);
                let kids = wrap_inline_runs(node, &style, children, always);
                out.push(Proto::Block {
                    owner,
                    style,
                    children: kids,
                    inner,
                    list_item: d == DisplayValue::ListItem,
                    tag: BoxTag::None,
                    atomic: d.is_atomic_inline(),
                });
            }
            d => {
                // `inline`; the table-internal values other than cell and caption are not
                // generated for pseudo-elements (boxed as `inline`, reported).
                if d.is_table_family() {
                    self.report(
                        feature(&format!("css.display.{}", d.keyword())),
                        Stage::Box,
                        node,
                    );
                }
                out.push(Proto::Inline {
                    node,
                    pseudo: Some(kind),
                    style,
                    children,
                });
            }
        }
    }

    /// The marker box of list item `node` (FR-082, FR-083), if it has one: `(box, outside)`.
    pub(crate) fn marker(&mut self, node: NodeId, li: &BoxStyle) -> Option<(Proto, bool)> {
        let inherited = BoxStyle::inheriting(li, DisplayValue::Inline);
        let mut ms = self
            .src
            .pseudo(node, PseudoKind::Marker)
            .cloned()
            .unwrap_or(inherited);
        let list = li.generated.list.clone();
        let outside = !list.inside;
        let text = match &ms.generated.content {
            Content::None => return None,
            Content::Items(items) => {
                let items = items.clone();
                Some(self.content_text(node, &ms, &items))
            }
            Content::Normal => None,
        };
        // Look up URL state once. Pending images keep the image box; a failed
        // image falls back to the list style while retaining its image gap.
        let image_state = if text.is_none()
            && matches!(list.image, crate::style::view::paint_groups::Image::Url(_))
        {
            self.volatile += 1;
            Some(self.src.css_image_state(node, "list-style-image"))
        } else {
            None
        };
        let image_broken = matches!(
            &image_state,
            Some(Some(ui4me_assets::view::ImageState::Broken { .. }))
        );
        // A bullet is drawn, not typeset (as the reference browser does): a replaced box.
        if text.is_none()
            && let CounterStyleRef::Name(n) = &list.style_type
            && (li_image_is_none(&list) || image_broken)
        {
            let shape = match n.as_str() {
                "disc" => Some(crate::layout::replaced::MarkerShape::Disc),
                "circle" => Some(crate::layout::replaced::MarkerShape::Circle),
                "square" => Some(crate::layout::replaced::MarkerShape::Square),
                // Inside disclosure markers use a drawn triangle; outside markers retain the
                // existing text-marker layout until their RTL placement is implemented.
                "disclosure-open" if !outside && !image_broken => {
                    Some(crate::layout::replaced::MarkerShape::DisclosureOpen)
                }
                "disclosure-closed" if !outside && !image_broken => {
                    Some(if li.direction == super::Direction::Rtl {
                        crate::layout::replaced::MarkerShape::DisclosureClosedRtl
                    } else {
                        crate::layout::replaced::MarkerShape::DisclosureClosedLtr
                    })
                }
                _ => None,
            };
            if let Some(shape) = shape {
                ms.display = DisplayValue::Inline;
                ms.marker_outside = outside;
                ms.decorations = std::sync::Arc::from(Vec::new());
                let content = crate::layout::replaced::ReplacedContent {
                    block_level: false,
                    kind: crate::layout::replaced::ReplacedKind::MarkerSymbol {
                        shape,
                        failed_image: image_broken,
                    },
                    inner: None,
                };
                return Some((
                    Proto::Replaced {
                        node,
                        pseudo: Some(PseudoKind::Marker),
                        style: ms,
                        content,
                    },
                    outside,
                ));
            }
        }
        // `list-style-image: url()`: the marker is the image at its natural size (CSS Lists 3 section 3.1),
        // followed by a 7 px gap (the reference browser's `kCMarkerPaddingPx`; `host_tree.rs`). A failed
        // image falls back to the `list-style-type` marker.
        if text.is_none()
            && let crate::style::view::paint_groups::Image::Url(_) = &list.image
        {
            let state = image_state.flatten();
            if !image_broken {
                ms.display = DisplayValue::Inline;
                ms.marker_outside = outside;
                ms.decorations = std::sync::Arc::from(Vec::new());
                let info = crate::layout::replaced::images::ImageInfo {
                    alt: None,
                    attr_size: None,
                    state,
                };
                let content = crate::layout::replaced::ReplacedContent {
                    block_level: false,
                    kind: crate::layout::replaced::ReplacedKind::Image(info),
                    inner: None,
                };
                return Some((
                    Proto::Replaced {
                        node,
                        pseudo: Some(PseudoKind::Marker),
                        style: ms,
                        content,
                    },
                    outside,
                ));
            }
        }
        let text = match text {
            Some(t) => t,
            None => {
                // Other `list-style-image` values (gradients, image functions) are not rendered:
                // fall back to the type, and say so.
                if !matches!(
                    list.image,
                    crate::style::view::paint_groups::Image::None
                        | crate::style::view::paint_groups::Image::Url(_)
                ) {
                    self.report(feature("css.property.list-style-image"), Stage::Box, node);
                }
                if let CounterStyleRef::Name(_) | CounterStyleRef::Symbols = list.style_type {
                    self.report_counter_style(node, &list.style_type);
                }
                // A style whose marker does not show the value (`none`, the symbols, a string) does not read
                // it (a counter read makes the incremental box build redo a build that took a counter writer
                // over). A list item's `list-item` counter exists here (its increment created it), so
                // reading it changes nothing.
                let shows_value = match &list.style_type {
                    CounterStyleRef::None | CounterStyleRef::String(_) => false,
                    CounterStyleRef::Name(n) => !matches!(
                        n.as_str(),
                        "none"
                            | "disc"
                            | "circle"
                            | "square"
                            | "disclosure-open"
                            | "disclosure-closed"
                    ),
                    _ => true,
                };
                let value = if shows_value {
                    self.counters.value("list-item")
                } else {
                    0
                };
                // A no-break space keeps the marker with the item's first word where the item cannot wrap
                // (`white-space: nowrap | pre`), and for an outside marker, which never takes part in wrapping.
                lists::marker_text(&list.style_type, value, outside || !li.white_space.wrap)?
            }
        };
        if text.is_empty() {
            return None;
        }
        // The disclosure triangles are glyphs followed by a gap of 0.4588 em in the reference browser
        // (measured on Debian 13 / Noto Sans Mono: 16.94 px for the glyph and gap at 16 px); the gap
        // is padding after the glyph here, not a space.
        let mut text = text;
        let disclosure = matches!(&list.style_type, CounterStyleRef::Name(n) if n.starts_with("disclosure-"))
            && matches!(ms.generated.content, Content::Normal);
        if disclosure {
            text = text.trim_end_matches([' ', '\u{a0}']).to_string();
            ms.padding.right = super::LengthPercentage::Px(0.4588 * ms.text.font_size);
        }
        // Failed image markers retain their 7px image gap when falling back to
        // a text counter. Outside marker placement is unchanged.
        if image_broken && !outside {
            if li.direction == super::Direction::Rtl {
                ms.padding.left = super::LengthPercentage::Px(7.0);
            } else {
                ms.padding.right = super::LengthPercentage::Px(7.0);
            }
        }
        // The marker's text is never collapsed, wrapped or transformed (UA `::marker` rules).
        ms.white_space = WhiteSpaceValue {
            collapse: CollapseMode::Preserve,
            wrap: false,
        };
        ms.flow.text_transform = Default::default();
        ms.text_align = super::TextAlignValue::Start;
        // The marker's own text is not indented or justified (the item's `text-indent` is the line's).
        ms.text_align_last = None;
        ms.text_indent = super::LengthPercentage::ZERO;
        let owner = BoxOwner::Pseudo(node, PseudoKind::Marker);
        let run = |style: &BoxStyle| Proto::Text {
            text: text.clone(),
            style: style.clone(),
            owner: node,
            pseudo: Some(PseudoKind::Marker),
        };
        if outside {
            ms.display = DisplayValue::InlineBlock;
            ms.marker_outside = true;
            // An outside marker is an atomic inline: no decoration of the item reaches it.
            ms.decorations = ms.own_decoration.iter().cloned().collect::<Vec<_>>().into();
            let children = vec![run(&ms)];
            Some((
                Proto::Block {
                    owner,
                    style: ms,
                    children,
                    inner: InnerDisplay::FlowRoot,
                    list_item: false,
                    tag: BoxTag::Marker(MarkerPlacement::Outside),
                    atomic: true,
                },
                true,
            ))
        } else {
            ms.display = DisplayValue::Inline;
            ms.decorations = li.decorations.clone();
            let children = vec![run(&ms)];
            Some((
                Proto::Inline {
                    node,
                    pseudo: Some(PseudoKind::Marker),
                    style: ms,
                    children,
                },
                false,
            ))
        }
    }
}

/// Add the marker of a list item to its children `kids` (before wrapping): an `inside` marker
/// leads the item; an `outside` marker joins the first line of the first in-flow line-bearing
/// descendant (CSS Lists 3 section 3.1), else leads the item.
/// The elements whose boxes the marker went into are pushed on `entered` (their box subtrees are no
/// longer their own build's: the incremental box build does not take them over).
pub(crate) fn place_marker(
    kids: &mut Vec<Proto>,
    (marker, outside): (Proto, bool),
    entered: &mut Vec<NodeId>,
) {
    if !outside {
        kids.insert(0, marker);
        return;
    }
    if let Some(unplaced) = place_in_first_line(kids, marker, entered) {
        kids.insert(0, unplaced);
    }
}

fn li_image_is_none(list: &crate::style::view::generated::ListStyle) -> bool {
    matches!(list.image, crate::style::view::paint_groups::Image::None)
}

/// Insert `marker` before the first line-bearing child; descends into plain block containers.
/// Returns the marker when no place was found.
fn place_in_first_line(
    kids: &mut Vec<Proto>,
    marker: Proto,
    entered: &mut Vec<NodeId>,
) -> Option<Proto> {
    let mut at = None;
    let mut descend = None;
    for (i, k) in kids.iter().enumerate() {
        if k.is_out_of_flow() || k.style().float != super::FloatValue::None || only_collapsible(k) {
            continue;
        }
        if !k.is_block_level() {
            at = Some(i);
        } else if matches!(
            k,
            Proto::Block {
                tag: BoxTag::None,
                inner: InnerDisplay::Flow | InnerDisplay::FlowRoot,
                atomic: false,
                ..
            }
        ) {
            descend = Some(i);
        }
        break;
    }
    if let Some(i) = at {
        kids.insert(i, marker);
        return None;
    }
    if let Some(i) = descend
        && let Proto::Block {
            children, owner, ..
        } = &mut kids[i]
    {
        if let BoxOwner::Element(n) = owner {
            entered.push(*n);
        }
        return place_in_first_line(children, marker, entered);
    }
    Some(marker)
}

fn feature(id: &str) -> FeatureId {
    static_feature(id.to_string())
}
