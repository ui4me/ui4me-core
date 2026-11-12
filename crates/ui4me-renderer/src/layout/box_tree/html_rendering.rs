//! owner: 008-layout-engine (T026, L-INL-r2) — the HTML rendering rules (WHATWG HTML §15; FR-013–FR-016,
//! FR-070–FR-075).
//!
//! Box generation consults these rules for the elements that are not plain CSS boxes:
//!
//! * `button` (and `input type=button|submit|reset|file|image`) → an atomic inline-level box
//!   (`inline-block`, block-level when the author made it `display: block`) with one anonymous
//!   flow-root `ButtonContent` box that holds the content and is centred vertically at layout.
//! * `img`, `canvas`, `video`, `audio[controls]`, `iframe`, `embed`, `object` → replaced boxes
//!   (`replaced/images.rs`, `replaced/embedded.rs`); a broken, unsized image with `alt` text becomes
//!   an inline text run of the `alt` value (FR-073).
//! * `input` (text-like, `number`, `checkbox`, `radio`, `range`), `textarea`, `select`, `progress`,
//!   `meter` → control boxes with natural sizes (`replaced/controls.rs`); `input type=hidden` has no
//!   box. Input types without a sizing row in controls.md §4 (`color`, the date and time types) are
//!   boxed as text lines and reported `html.control.<type>`.
//! * `fieldset` → a box that establishes a block formatting context; its rendered legend (first `legend`
//!   child neither floated nor absolutely positioned) is placed over the top border in an anonymous legend
//!   area, the rest goes into the anonymous fieldset content box (`fieldset`, 015 HTML-3).
//! * `details` without `open` → the first `summary` child is laid out; the other children go into a
//!   zero-height clipping slot (`closed_details_slot`).
//!
//! Legacy `align` / `center` alignment arrives as presentational hints from 006 and needs no rule
//! here.

use ui4me_assets::view::ImageState;
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

use super::build::{BoxOwner, BoxTag, Builder, Proto};
use super::{BoxStyle, DisplayValue, HtmlInfo, InnerDisplay, SizeValue, StyledSource};
use crate::layout::replaced::controls::ControlSpec;
use crate::layout::replaced::images::ImageInfo;
use crate::layout::replaced::inner::InnerSource;
use crate::layout::replaced::{ReplacedContent, ReplacedKind, embedded};

/// What box generation does with an element.
pub(crate) enum Special {
    /// An ordinary CSS box.
    Ordinary,
    /// The element generates no box.
    Skip,
    Generated(Box<Proto>),
}

/// Elements the HTML identity is built for (everything else is an ordinary box).
pub(crate) fn is_html_special(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "br" | "button"
            | "input"
            | "select"
            | "textarea"
            | "img"
            | "progress"
            | "meter"
            | "canvas"
            | "video"
            | "audio"
            | "iframe"
            | "embed"
            | "object"
            | "svg"
            | "details"
            | "summary"
            | "fieldset"
            | "legend"
            | "option"
            | "optgroup"
    )
}

/// The children of `parent` that generate boxes: all of them. A `details` without `open` keeps its
/// non-summary children too: they are laid out inside the closed-details slot (`closed_details_slot`),
/// which takes no space and paints nothing.
pub(crate) fn rendered_children<S: StyledSource>(src: &S, parent: NodeId) -> Vec<NodeId> {
    src.children(parent)
}

/// For a `details` without `open`: the index (in `src.children(parent)`) of its first `summary` child,
/// whose siblings form the hidden content slot. `None` for any other element, or an open `details`.
pub(crate) fn closed_details_summary<S: StyledSource>(
    src: &S,
    parent: NodeId,
) -> Option<Option<usize>> {
    match src.html(parent) {
        Some(info) if info.name == "details" && !info.has("open") => Some(
            src.children(parent)
                .iter()
                .position(|c| src.html(*c).is_some_and(|i| i.name == "summary")),
        ),
        _ => None,
    }
}

/// The slot that holds the content of a closed `details` (HTML 15.5.4 "the details and summary elements":
/// the content is hidden by `content-visibility: hidden` on the `::details-content` slot, as in the
/// reference browser): a block of no height that clips, inside which the content is laid out like an open
/// one (so geometry reads of it succeed) but neither takes space nor paints.
pub(crate) fn closed_details_slot(
    parent: NodeId,
    parent_style: &BoxStyle,
    children: Vec<Proto>,
) -> Proto {
    let mut style = BoxStyle::inheriting(parent_style, DisplayValue::Block);
    style.height = SizeValue::Lp(super::LengthPercentage::ZERO);
    style.overflow_x = super::OverflowValue::Hidden;
    style.overflow_y = super::OverflowValue::Hidden;
    Proto::Block {
        owner: BoxOwner::Anonymous(parent),
        style,
        children,
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::None,
        atomic: false,
    }
}

fn positive_int(info: &HtmlInfo, name: &str) -> Option<u32> {
    info.attr(name)?
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0 && *n < 1_000_000)
        .map(|n| n as u32)
}

fn px_attr(info: &HtmlInfo, name: &str) -> Option<f32> {
    let v = info.attr(name)?.trim();
    let v = v.strip_suffix("px").unwrap_or(v);
    v.parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0)
}

fn feature(id: &'static str) -> FeatureId {
    FeatureId::new(id).expect("html is a registered root")
}

pub(crate) fn feature_id(id: &'static str) -> FeatureId {
    feature(id)
}

/// Text of an element's text-node descendants, white space collapsed.
fn text_of<S: StyledSource>(src: &S, node: NodeId) -> String {
    let mut out = String::new();
    for c in src.children(node) {
        match src.node(c) {
            super::SourceNode::Text(t) => out.push_str(t),
            super::SourceNode::Element(_) => out.push_str(&text_of(src, c)),
            super::SourceNode::Ignored => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Option labels of a `select` (options inside optgroups included), in order.
fn options_of<S: StyledSource>(src: &S, node: NodeId, out: &mut Vec<String>) {
    for c in src.children(node) {
        match src.html(c).map(|i| (i.name.clone(), i)) {
            Some((n, info)) if n == "option" => {
                let label = info
                    .attr("label")
                    .map(str::to_string)
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| text_of(src, c));
                out.push(label);
            }
            Some((n, _)) if n == "optgroup" => options_of(src, c, out),
            _ => {}
        }
    }
}

fn input_type(info: &HtmlInfo) -> String {
    ui4me_document::control_semantics::InputType::parse(info.attr("type"))
        .as_str()
        .to_owned()
}

/// The HTML rendering rules for one element that has an HTML identity.
pub(crate) fn special<S: StyledSource>(
    b: &mut Builder<'_, S>,
    node: NodeId,
    info: &HtmlInfo,
    style: &BoxStyle,
    d: DisplayValue,
) -> Special {
    // A block-level outer display (`block`, `flow-root`, `list-item`, `flex`, `grid`); inside an
    // inline box such a box splits it (FR-007, `build::split_inline`).
    let block_level = d.is_block_level();
    let leaf = |_b: &mut Builder<'_, S>, kind: ReplacedKind| -> Special {
        Special::Generated(Box::new(Proto::Replaced {
            node,
            pseudo: None,
            style: style.clone(),
            content: ReplacedContent {
                block_level,
                kind,
                inner: None,
            },
        }))
    };
    // A text control: a leaf that also shows its value or placeholder (`replaced/inner.rs`).
    let text_leaf =
        |_b: &mut Builder<'_, S>, kind: ReplacedKind, multiline: bool, password: bool| -> Special {
            let inner = InnerSource {
                multiline,
                password,
                value: info.control_value.clone().unwrap_or_else(|| {
                    if multiline {
                        String::new()
                    } else {
                        info.attr("value").unwrap_or("").to_string()
                    }
                }),
                placeholder: info.attr("placeholder").unwrap_or("").to_string(),
                select: None,
            };
            Special::Generated(Box::new(Proto::Replaced {
                node,
                pseudo: None,
                style: style.clone(),
                content: ReplacedContent {
                    block_level,
                    kind,
                    inner: Some(inner),
                },
            }))
        };
    match info.name.as_str() {
        // WHATWG HTML §15.3.? `br`: a forced line break. Its box is an inline box holding one
        // preserved segment break (spaces before it collapse away, as at any line end).
        "br" if d == DisplayValue::Inline => {
            let mut text_style = style.clone();
            text_style.white_space = super::WhiteSpaceValue {
                collapse: super::CollapseMode::PreserveBreaks,
                wrap: true,
            };
            if let Some(fl) = text_style.first_line.as_mut() {
                std::sync::Arc::make_mut(fl).white_space = text_style.white_space;
            }
            let text = Proto::Text {
                text: "\n".to_string(),
                style: text_style,
                owner: node,
                pseudo: None,
            };
            Special::Generated(Box::new(Proto::Inline {
                node,
                pseudo: None,
                style: style.clone(),
                children: vec![text],
            }))
        }
        // A button with `display: flex | grid` is that container (the reference browser honours it): its
        // children are the items and there is no anonymous inner box (Bootstrap `.accordion-button`,
        // whose `::after` icon is a flex item beside the label; L-GEO-r9).
        "button"
            if !matches!(
                d,
                DisplayValue::Flex
                    | DisplayValue::InlineFlex
                    | DisplayValue::Grid
                    | DisplayValue::InlineGrid
            ) =>
        {
            Special::Generated(Box::new(button(b, node, style, block_level, None)))
        }
        "input" => {
            let ty = input_type(info);
            match ty.as_str() {
                "hidden" => Special::Skip,
                "button" | "submit" | "reset" | "file" | "image" => {
                    // A file input is a button that opens the native picker (0.6 F0606).
                    let value = info.attr("value").filter(|v| !v.is_empty());
                    let label = match ty.as_str() {
                        "submit" => value.unwrap_or("Submit").to_string(),
                        "reset" => value.unwrap_or("Reset").to_string(),
                        "file" => {
                            // Selection is supplied by Document, never inferred from the value
                            // attribute or a script-provided filesystem path.
                            b.volatile += 1;
                            b.src
                                .file_input_label(node)
                                .unwrap_or("Choose File")
                                .to_string()
                        }
                        "image" => info
                            .attr("alt")
                            .filter(|a| !a.is_empty())
                            .unwrap_or("Submit")
                            .to_string(),
                        _ => value.unwrap_or("").to_string(),
                    };
                    Special::Generated(Box::new(button(b, node, style, block_level, Some(label))))
                }
                "checkbox" => leaf(b, ReplacedKind::Control(ControlSpec::Checkbox)),
                "radio" => leaf(b, ReplacedKind::Control(ControlSpec::Radio)),
                "range" => leaf(b, ReplacedKind::Control(ControlSpec::Range)),
                "number" => text_leaf(b, ReplacedKind::Control(ControlSpec::Number), false, false),
                "" | "text" | "search" | "tel" | "url" | "email" | "password" => {
                    let size = positive_int(info, "size").unwrap_or(20);
                    text_leaf(
                        b,
                        ReplacedKind::Control(ControlSpec::TextLine { size }),
                        false,
                        ty == "password",
                    )
                }
                "color" => {
                    // A swatch box (no picker): reported, drawn by the scene from the value.
                    b.report(feature("html.control.color"), Stage::Box, node);
                    leaf(b, ReplacedKind::Control(ControlSpec::Color))
                }
                other => {
                    let id = match other {
                        "color" => "html.control.color",
                        "date" => "html.control.date",
                        "datetime-local" => "html.control.datetime-local",
                        "month" => "html.control.month",
                        "week" => "html.control.week",
                        "time" => "html.control.time",
                        _ => "html.control.unknown",
                    };
                    b.report(feature(id), Stage::Box, node);
                    leaf(b, ReplacedKind::Control(ControlSpec::TextLine { size: 20 }))
                }
            }
        }
        "textarea" => {
            let cols = positive_int(info, "cols").unwrap_or(20);
            let rows = positive_int(info, "rows").unwrap_or(2);
            text_leaf(
                b,
                ReplacedKind::Control(ControlSpec::Textarea { cols, rows }),
                true,
                false,
            )
        }
        "select" => {
            let mut options = Vec::new();
            options_of(b.src, node, &mut options);
            let size = info.select.as_ref().map_or_else(
                || ui4me_document::select::display_size(info.attr("size"), info.has("multiple")),
                |facts| facts.display_size,
            );
            let rows = (size > 1).then_some(size.min(999_999));
            match &info.select {
                // The document's rows (hidden options left out) and the popup state: the box also
                // shows the selected label, the listbox rows and the open popup (`inner.rs`).
                Some(facts) => {
                    let options: Vec<String> = facts
                        .rows
                        .iter()
                        .filter(|r| !r.group)
                        .map(|r| r.label.clone())
                        .collect();
                    let inner = InnerSource {
                        multiline: false,
                        password: false,
                        value: String::new(),
                        placeholder: String::new(),
                        select: Some(facts.clone()),
                    };
                    Special::Generated(Box::new(Proto::Replaced {
                        node,
                        pseudo: None,
                        style: style.clone(),
                        content: ReplacedContent {
                            block_level,
                            kind: ReplacedKind::Control(ControlSpec::Select {
                                options,
                                rows,
                                native: style.native_control,
                            }),
                            inner: Some(inner),
                        },
                    }))
                }
                None => leaf(
                    b,
                    ReplacedKind::Control(ControlSpec::Select {
                        options,
                        rows,
                        native: style.native_control,
                    }),
                ),
            }
        }
        "progress" => leaf(b, ReplacedKind::Control(ControlSpec::Progress)),
        "meter" => leaf(b, ReplacedKind::Control(ControlSpec::Meter)),
        "img" => {
            let alt = info.attr("alt").map(str::to_string);
            let attr_size = match (px_attr(info, "width"), px_attr(info, "height")) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            };
            b.volatile += 1;
            let state = b.src.image_state(node);
            let sized = attr_size.is_some()
                || !matches!(style.width, SizeValue::Auto)
                || !matches!(style.height, SizeValue::Auto);
            if matches!(state, Some(ImageState::Broken { .. }))
                && !sized
                && let Some(alt) = alt.as_deref().filter(|a| !a.is_empty())
            {
                // FR-073: an unsized failed image is an inline text run of its `alt` value.
                let text = Proto::Text {
                    text: alt.to_string(),
                    style: style.clone(),
                    owner: node,
                    pseudo: None,
                };
                return Special::Generated(Box::new(Proto::Inline {
                    node,
                    pseudo: None,
                    style: style.clone(),
                    children: vec![text],
                }));
            }
            leaf(
                b,
                ReplacedKind::Image(ImageInfo {
                    alt,
                    attr_size,
                    state,
                }),
            )
        }
        name => {
            // An `embed` without `src` has nothing to embed: no box (Chromium does not render it).
            if name == "embed" && info.attr("src").is_none_or(str::is_empty) {
                return Special::Skip;
            }
            if let Some(mut e) = embedded::classify(name, info.has("controls")) {
                if let Some(f) = e.feature() {
                    b.report(f, Stage::Box, node);
                }
                e.ratio = info.attr("viewbox").and_then(embedded::view_box_ratio);
                return leaf(b, ReplacedKind::Embedded(e));
            }
            if name == "fieldset" && (block_level || d.is_atomic_inline()) {
                return fieldset(b, node, style, !block_level);
            }
            Special::Ordinary
        }
    }
}

/// `button` / input button: the element's box plus one anonymous `ButtonContent` flow-root box.
fn button<S: StyledSource>(
    b: &mut Builder<'_, S>,
    node: NodeId,
    style: &BoxStyle,
    block_level: bool,
    label: Option<String>,
) -> Proto {
    let content_style = BoxStyle::inheriting(style, DisplayValue::Block);
    let kids = match label {
        Some(l) if !l.is_empty() => vec![Proto::Text {
            text: l,
            style: content_style.clone(),
            owner: node,
            pseudo: None,
        }],
        Some(_) => Vec::new(),
        None => {
            let kids = b.children(node, &content_style);
            super::anonymous::wrap_inline_runs(node, &content_style, kids, false)
        }
    };
    let content = Proto::Block {
        owner: BoxOwner::Anonymous(node),
        style: content_style,
        children: kids,
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::ButtonContent,
        atomic: false,
    };
    Proto::Block {
        owner: BoxOwner::Element(node),
        style: style.clone(),
        children: vec![content],
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::None,
        atomic: !block_level,
    }
}

/// `fieldset` (HTML 15.3.13; 015 HTML-3): a block formatting context. Without a rendered legend it is one
/// box holding its content. With one (its first `legend` child that is neither floated nor absolutely
/// positioned) it is laid out as the reference browser does it:
///
/// * the fieldset box keeps its border and padding except at the block start, where layout sees
///   neither (the real top border width rides on `BoxStyle::fieldset_border_top` for paint, which draws
///   it centred on the legend and cuts the legend's gap out of it);
/// * its first child is an anonymous legend area: a column flex container at least as tall as the top
///   border, holding the legend shrink-to-fit, centred over the border when the border is taller,
///   aligned inline by its margins and the legacy `align` attribute (`justify-self`), its block-start
///   margin ignored;
/// * its second child is the anonymous fieldset content box (flow-root, or flex / grid when the
///   fieldset's display says so) with the fieldset's block-start padding as its top margin, holding
///   everything else (`::before` and `::after` included).
///
/// A definite `height` / `min-height` / `max-height` in px under `box-sizing: content-box` grows by
/// the removed border and padding, so the border box is the reference's.
fn fieldset<S: StyledSource>(
    b: &mut Builder<'_, S>,
    node: NodeId,
    style: &BoxStyle,
    atomic: bool,
) -> Special {
    // A fieldset establishes a block formatting context (WHATWG §15.3.13): its inner display is
    // at least `flow-root`; an author `display: flex | grid` keeps that container type.
    let inner = match style.display.inner() {
        Some(InnerDisplay::Flex) => InnerDisplay::Flex,
        Some(InnerDisplay::Grid) => InnerDisplay::Grid,
        _ => InnerDisplay::FlowRoot,
    };
    let legend = b.src.children(node).into_iter().find(|&c| {
        b.src.html(c).is_some_and(|i| i.name == "legend")
            && matches!(b.src.node(c), super::SourceNode::Element(ls) if ls.float == super::FloatValue::None && !ls.position.is_out_of_flow() && !matches!(ls.display, DisplayValue::None | DisplayValue::Contents))
    });
    let Some(legend) = legend else {
        return Special::Generated(Box::new(b.block_boxed(
            BoxOwner::Element(node),
            node,
            style.clone(),
            inner,
            false,
            BoxTag::Fieldset,
            atomic,
        )));
    };
    let mut kids = b.children(node, style);
    // The legend's box is edited below: no child subtree stays taken over (incremental.rs).
    b.expand_reused(&mut kids);
    let pos = kids.iter().position(
        |c| matches!(c, Proto::Block { owner: BoxOwner::Element(n), .. } if *n == legend),
    );
    let Some(pos) = pos else {
        return Special::Generated(Box::new(b.block_boxed(
            BoxOwner::Element(node),
            node,
            style.clone(),
            inner,
            false,
            BoxTag::Fieldset,
            atomic,
        )));
    };
    let mut legend_proto = kids.remove(pos);
    b.tainted.push(legend);
    let border_top = style.border.top;
    if let Proto::Block { tag, style: ls, .. } = &mut legend_proto {
        *tag = BoxTag::RenderedLegend;
        // The block-start margin of the rendered legend is ignored (as the reference browser does).
        ls.legend_report_margin_top = Some(ls.margin.top.clone());
        ls.margin.top = super::MarginValue::Lp(super::LengthPercentage::ZERO);
        // Inline alignment: `justify-self` (the legacy `align` attribute maps to it in the UA sheet; its
        // `left` / `right` values come through the attribute).
        let align = b
            .src
            .html(legend)
            .and_then(|i| i.attr("align").map(|a| a.trim().to_ascii_lowercase()));
        let rtl = ls.direction == super::Direction::Rtl;
        let keyword = match (align.as_deref(), ls.align.justify_self.keyword) {
            (Some("center"), _) | (_, super::AlignKeyword::Center) => super::AlignKeyword::Center,
            (Some("right"), _) if !rtl => super::AlignKeyword::FlexEnd,
            (Some("left"), _) if rtl => super::AlignKeyword::FlexEnd,
            (
                _,
                super::AlignKeyword::End
                | super::AlignKeyword::FlexEnd
                | super::AlignKeyword::SelfEnd,
            ) => super::AlignKeyword::FlexEnd,
            _ => super::AlignKeyword::FlexStart,
        };
        ls.align.align_self = super::AlignValue {
            keyword,
            safe: None,
        };
        if align.is_some() {
            ls.unconsumed_layout
                .retain(|f| f.as_str() != "css.property.justify-self");
        }
    }
    // The legend area: a column flex container at least as tall as the top border.
    let mut area_style = BoxStyle::inheriting(style, DisplayValue::Flex);
    area_style.flex.direction = super::FlexDirectionValue::Column;
    area_style.align.justify_content = super::AlignValue {
        keyword: super::AlignKeyword::Center,
        safe: None,
    };
    area_style.align.align_items = super::AlignValue {
        keyword: super::AlignKeyword::FlexStart,
        safe: None,
    };
    area_style.min_height = SizeValue::Lp(super::LengthPercentage::Px(border_top));
    let area = Proto::Block {
        owner: BoxOwner::Anonymous(node),
        style: area_style,
        children: vec![legend_proto],
        inner: InnerDisplay::Flex,
        list_item: false,
        tag: BoxTag::None,
        atomic: false,
    };
    // The anonymous fieldset content box.
    let content_display = match inner {
        InnerDisplay::Flex => DisplayValue::Flex,
        InnerDisplay::Grid => DisplayValue::Grid,
        _ => DisplayValue::FlowRoot,
    };
    let mut content_style = BoxStyle::inheriting(style, content_display);
    content_style.margin.top = super::MarginValue::Lp(style.padding.top.clone());
    content_style.flex = style.flex.clone();
    content_style.align = style.align.clone();
    content_style.grid = style.grid.clone();
    let always = matches!(inner, InnerDisplay::Flex | InnerDisplay::Grid);
    let content_kids = super::anonymous::wrap_inline_runs(node, &content_style, kids, always);
    let content = Proto::Block {
        owner: BoxOwner::Anonymous(node),
        style: content_style,
        children: content_kids,
        inner,
        list_item: false,
        tag: BoxTag::None,
        atomic: false,
    };
    // The fieldset box: no block-start border or padding for layout.
    let mut fs = style.clone();
    let pad_top = std::mem::replace(&mut fs.padding.top, super::LengthPercentage::ZERO);
    fs.border.top = 0.0;
    fs.fieldset_border_top = Some((
        border_top,
        if let super::LengthPercentage::Px(pt) = pad_top {
            pt
        } else {
            0.0
        },
    ));
    if fs.box_sizing == super::BoxSizing::ContentBox
        && let super::LengthPercentage::Px(pt) = pad_top
    {
        let grow = |v: &mut SizeValue| {
            if let SizeValue::Lp(super::LengthPercentage::Px(h)) = v {
                *h += border_top + pt;
            }
        };
        grow(&mut fs.height);
        grow(&mut fs.min_height);
        if let super::MaxSizeValue::Lp(super::LengthPercentage::Px(h)) = &mut fs.max_height {
            *h += border_top + pt;
        }
    }
    let p = Proto::Block {
        owner: BoxOwner::Element(node),
        style: fs,
        children: vec![area, content],
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::Fieldset,
        atomic,
    };
    Special::Generated(Box::new(p))
}
