//! owner: 015-core-completion (PAGE-1, lane L-PAGE) — the page context of CSS Paged Media 3.
//!
//! `@page` rules (parsed by Stylo with the carried patch P-006-9 / UP-13) are cascaded here, not by
//! Stylo's stylist (servo mode keeps no page-rule map): for one page (its index, `:first`, `:blank`,
//! `:left` / `:right` and its name) every effective `@page` rule of the author sheets that matches is
//! taken in order of specificity (CSS Paged Media 3 section 4.2: name, then `:first` / `:blank`, then
//! `:left` / `:right`) and source order, normal declarations before `!important` ones, into one declaration
//! block. The **page context** is that block computed with the root element's style as parent (the
//! reference browser inherits the page context from the root element), and each **page-margin box** is the
//! block of its `@top-left` ... rules (section 5) computed with the page context as parent.
//!
//! Values that layout needs (the `size` descriptor, the page margins) are typed here; the rest stay opaque
//! [`PageValues`] that a synthetic [`StyleSnapshot`] carries to the box tree and the scene of the margin
//! boxes (`ui4me-pipeline` `paged.rs`), so the margin boxes are laid out and painted by the ordinary code.

use std::sync::Arc;

type BlockArc = StyloArc<Locked<PropertyDeclarationBlock>>;
/// Specificity, source order, declarations and indexed margin-box declarations.
type MatchedPageRule = (u32, usize, BlockArc, Vec<(usize, BlockArc)>);

use style::properties::{ComputedValues, Importance, PropertyDeclarationBlock};
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::{Locked, StylesheetGuards};
use style::stylesheets::{
    CssRule, CssRuleType, CustomMediaMap, PagePseudoClassFlags, PageRule, StylesheetInDocument,
};
use ui4me_document::document::Document;
use ui4me_engine_types::ids::{DocumentId, NodeId};

use crate::style::traverse::StyleBridge;
use crate::style::view::StyleSnapshot;
use crate::style::view::generated::ContentItem;

/// The sixteen page-margin boxes, in Stylo's `MarginRuleType` order.
pub const MARGIN_BOX_NAMES: [&str; 16] = [
    "top-left-corner",
    "top-left",
    "top-center",
    "top-right",
    "top-right-corner",
    "bottom-left-corner",
    "bottom-left",
    "bottom-center",
    "bottom-right",
    "bottom-right-corner",
    "left-top",
    "left-middle",
    "left-bottom",
    "right-top",
    "right-middle",
    "right-bottom",
];

/// Which page a page context is computed for (CSS Paged Media 3 section 4.3 page selectors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageQuery<'a> {
    /// `:first`: the first page of the document.
    pub first: bool,
    /// `:blank`: a page left empty by a forced `left` / `right` / `recto` / `verso` break.
    pub blank: bool,
    /// `:left` (else `:right`).
    pub left: bool,
    /// The page's name (`page` property of its first content); `""` for an unnamed page.
    pub name: &'a str,
}

/// Computed values of a page context or a margin box, opaque outside this crate.
#[derive(Clone)]
pub struct PageValues(pub(crate) StyloArc<ComputedValues>);

impl std::fmt::Debug for PageValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageValues")
    }
}

impl PartialEq for PageValues {
    fn eq(&self, other: &Self) -> bool {
        StyloArc::ptr_eq(&self.0, &other.0)
    }
}

/// The `size` descriptor of a page context (section 7.1), with paper sizes resolved to CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageSizeSpec {
    Auto,
    /// `landscape` (`true`) or `portrait` without a size: the default size in that orientation.
    Orientation {
        landscape: bool,
    },
    /// Width and height in CSS px.
    Px(f32, f32),
}

/// One page margin (section 7.2): `auto` resolves to 0 in this engine's page model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageMargin {
    Auto,
    Px(f32),
    /// A fraction of the page box's width (left/right) or height (top/bottom).
    Percent(f32),
}

impl PageMargin {
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            PageMargin::Auto => 0.0,
            PageMargin::Px(p) => p,
            PageMargin::Percent(f) => f * basis,
        }
    }
}

/// `vertical-align` of a margin box (section 5.3.3; `baseline` acts as `top`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarginAlign {
    Top,
    Middle,
    Bottom,
}

/// One generated page-margin box: its `content` is not `none` / `normal` (section 5.2).
#[derive(Clone, Debug)]
pub struct MarginBoxStyle {
    /// Index into [`MARGIN_BOX_NAMES`].
    pub slot: usize,
    /// Computed `content` items (`counter(page)`, `counter(pages)`, strings ...).
    pub content: Vec<ContentItem>,
    /// `vertical-align` (the slot's default unless the author set one).
    pub align: MarginAlign,
    /// Whether the author gave a definite `width` / `height` (`auto` otherwise; section 5.3.2 sizing).
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Computed values without the placement (UA defaults of the slot + author declarations).
    pub values: PageValues,
    block: BlockArc,
}

/// The computed page context of one page.
#[derive(Clone, Debug)]
pub struct PageStyle {
    pub size: PageSizeSpec,
    /// Top, right, bottom, left.
    pub margin: [PageMargin; 4],
    /// Which of the four margins an `@page` rule declares (the others take the caller's default page margins).
    pub margin_declared: [bool; 4],
    /// The page context's computed values (background, inherited properties of the margin boxes).
    pub context: PageValues,
    /// The background colour of the page context, RGBA8 (`None`: transparent).
    pub background: Option<[u8; 4]>,
    /// Generated margin boxes, in slot order.
    pub boxes: Vec<MarginBoxStyle>,
}

impl PageStyle {
    /// The page box size: `size` against the default page size `default` (CSS px).
    pub fn page_size(&self, default: (f32, f32)) -> (f32, f32) {
        match self.size {
            PageSizeSpec::Auto => default,
            PageSizeSpec::Orientation { landscape } => {
                let (a, b) = (default.0.min(default.1), default.0.max(default.1));
                if landscape { (b, a) } else { (a, b) }
            }
            PageSizeSpec::Px(w, h) => (w, h),
        }
    }
}

/// The UA defaults of a margin box (section 5.3 table): text alignment and vertical alignment.
fn slot_defaults(slot: usize) -> (&'static str, MarginAlign) {
    match MARGIN_BOX_NAMES[slot] {
        "top-left-corner" => ("right", MarginAlign::Middle),
        "top-left" => ("left", MarginAlign::Middle),
        "top-center" => ("center", MarginAlign::Middle),
        "top-right" => ("right", MarginAlign::Middle),
        "top-right-corner" => ("left", MarginAlign::Middle),
        "bottom-left-corner" => ("right", MarginAlign::Middle),
        "bottom-left" => ("left", MarginAlign::Middle),
        "bottom-center" => ("center", MarginAlign::Middle),
        "bottom-right" => ("right", MarginAlign::Middle),
        "bottom-right-corner" => ("left", MarginAlign::Middle),
        "left-top" => ("center", MarginAlign::Top),
        "left-middle" => ("center", MarginAlign::Middle),
        "left-bottom" => ("center", MarginAlign::Bottom),
        "right-top" => ("center", MarginAlign::Top),
        "right-middle" => ("center", MarginAlign::Middle),
        _ => ("center", MarginAlign::Bottom),
    }
}

/// Collect the matching blocks of `rules` (in order) for `q`: (specificity, order, block).
fn matching(rule: &PageRule, q: &PageQuery<'_>, flags: PagePseudoClassFlags) -> Option<u32> {
    if rule.selectors.is_empty() {
        return rule.match_specificity(flags);
    }
    rule.selectors
        .as_slice()
        .iter()
        .filter(|s| s.name.0.is_empty() || &*s.name.0 == q.name)
        .filter_map(|s| s.match_specificity(flags))
        .max()
}

impl StyleBridge {
    /// Whether any effective author sheet has an `@page` rule.
    pub fn has_page_rules(&self) -> bool {
        let guard = self.sidecars.lock.read();
        let mut found = false;
        self.for_each_page_rule(&guard, |_| found = true);
        found
    }

    fn for_each_page_rule<'g>(
        &self,
        guard: &style::shared_lock::SharedRwLockReadGuard<'g>,
        mut f: impl FnMut(&PageRule),
    ) {
        let device = self.stylist.device();
        let custom = CustomMediaMap::default();
        for sheet in &self.sheets.author {
            if !sheet.sheet.enabled()
                || !sheet.sheet.is_effective_for_device(device, &custom, guard)
            {
                continue;
            }
            for rule in sheet
                .sheet
                .contents(guard)
                .effective_rules(device, &custom, guard)
            {
                if let CssRule::Page(p) = rule {
                    f(p.read_with(guard));
                }
            }
        }
    }

    /// The page context of the page `q` (section 4): the matching `@page` rules cascaded, computed with the root
    /// element's style as parent; its margin boxes computed with the page context as parent.
    pub fn page_style(&self, doc: &Document, q: PageQuery<'_>) -> PageStyle {
        let lock = self.sidecars.lock.clone();
        let guard = lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        let mut flags = PagePseudoClassFlags::NONE;
        if q.first {
            flags |= PagePseudoClassFlags::FIRST;
        }
        if q.blank {
            flags |= PagePseudoClassFlags::BLANK;
        }
        flags |= if q.left {
            PagePseudoClassFlags::LEFT
        } else {
            PagePseudoClassFlags::RIGHT
        };
        // (specificity, order) of every matching rule, and its blocks.
        let mut matched: Vec<MatchedPageRule> = Vec::new();
        let mut order = 0usize;
        self.for_each_page_rule(&guard, |rule| {
            order += 1;
            if let Some(spec) = matching(rule, &q, flags) {
                let margins = rule
                    .rules
                    .read_with(&guard)
                    .0
                    .iter()
                    .filter_map(|r| match r {
                        CssRule::Margin(m) => Some((m.rule_type as usize, m.block.clone())),
                        _ => None,
                    })
                    .collect();
                matched.push((spec, order, rule.block.clone(), margins));
            }
        });
        matched.sort_by_key(|m| (m.0, m.1));
        let merge = |blocks: &mut dyn Iterator<Item = &BlockArc>,
                     prefix: Option<PropertyDeclarationBlock>| {
            let blocks: Vec<&BlockArc> = blocks.collect();
            let mut out = prefix.unwrap_or_default();
            for important in [false, true] {
                for b in &blocks {
                    for (d, imp) in b.read_with(&guard).declaration_importance_iter() {
                        if imp.important() == important {
                            out.push(d.clone(), imp);
                        }
                    }
                }
            }
            out
        };
        let context_block = merge(&mut matched.iter().map(|m| &m.2), None);
        let mut margin_declared = [false; 4];
        for d in context_block.declarations() {
            match d.id().name().as_ref() {
                "margin-top" => margin_declared[0] = true,
                "margin-right" => margin_declared[1] = true,
                "margin-bottom" => margin_declared[2] = true,
                "margin-left" => margin_declared[3] = true,
                _ => {}
            }
        }
        let root_style = doc
            .document_element()
            .and_then(|r| self.primary_style(doc, r))
            .unwrap_or_else(|| self.initial.clone());
        let compute = |parent: &ComputedValues,
                       block: PropertyDeclarationBlock|
         -> StyloArc<ComputedValues> {
            let locked = StyloArc::new(lock.wrap(block));
            self.stylist
                .compute_for_declarations::<crate::style::adapter::handle::StyleNode<'_>>(
                    &guards, parent, locked,
                )
        };
        // The page context inherits from the root element (WPT `page-context-inherits-from-root-*`; the reference).
        let context = compute(&root_style, context_block);
        let size = match context.get_box().size {
            style::values::computed::PageSize::Auto => PageSizeSpec::Auto,
            style::values::computed::PageSize::Orientation(o) => PageSizeSpec::Orientation {
                landscape: matches!(o, style::values::computed::PageSizeOrientation::Landscape),
            },
            style::values::computed::PageSize::Size(s) => {
                PageSizeSpec::Px(s.width.0.px(), s.height.0.px())
            }
        };
        let m = context.get_margin();
        let side = |v: &style::values::computed::Margin| -> PageMargin {
            use style::values::generics::length::GenericMargin as M;
            match v {
                M::LengthPercentage(lp) => {
                    if let Some(l) = lp.to_length() {
                        PageMargin::Px(l.px())
                    } else if let Some(p) = lp.to_percentage() {
                        PageMargin::Percent(p.0)
                    } else {
                        PageMargin::Px(lp.resolve(style::values::computed::Length::new(0.0)).px())
                    }
                }
                _ => PageMargin::Auto,
            }
        };
        let margin = [
            side(&m.margin_top),
            side(&m.margin_right),
            side(&m.margin_bottom),
            side(&m.margin_left),
        ];
        let background = {
            let c = context.resolve_color(&context.get_background().background_color);
            let s = c.to_color_space(style::color::ColorSpace::Srgb);
            let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let a = ch(s.alpha);
            (a > 0).then(|| {
                [
                    ch(s.components.0),
                    ch(s.components.1),
                    ch(s.components.2),
                    a,
                ]
            })
        };
        let url = self.url.clone();
        let mut boxes = Vec::new();
        for slot in 0..16 {
            let blocks: Vec<&BlockArc> = matched
                .iter()
                .flat_map(|m| m.3.iter().filter(|(s, _)| *s == slot).map(|(_, b)| b))
                .collect();
            if blocks.is_empty() {
                continue;
            }
            let (text_align, default_align) = slot_defaults(slot);
            let ua = style::properties::parse_style_attribute(
                &format!("text-align: {text_align}"),
                &url,
                None,
                self.quirks,
                CssRuleType::Style,
            );
            let block = merge(&mut blocks.into_iter(), Some(ua));
            let values = compute(&context, block.clone());
            let generated = crate::style::view::generated::generated(0, &values);
            let content = match generated.content {
                crate::style::view::generated::Content::Items(items) => items,
                _ => continue,
            };
            let b = values.get_box();
            let shift = crate::style::view::groups::css(&b.baseline_shift);
            let align = if shift == "top" {
                MarginAlign::Top
            } else if shift == "bottom" {
                MarginAlign::Bottom
            } else if crate::style::view::groups::css(&b.alignment_baseline) == "middle" {
                MarginAlign::Middle
            } else if block.declarations().iter().any(|d| {
                let n = d.id().name();
                n == "alignment-baseline" || n == "baseline-shift"
            }) {
                // `vertical-align: baseline` (or a length): top (section 5.3.3).
                MarginAlign::Top
            } else {
                default_align
            };
            let pos = values.get_position();
            let def = |s: &style::values::computed::Size| -> Option<f32> {
                match s {
                    style::values::generics::length::GenericSize::LengthPercentage(lp) => {
                        lp.0.to_length().map(|l| l.px())
                    }
                    _ => None,
                }
            };
            let (width, height) = (def(&pos.width), def(&pos.height));
            boxes.push(MarginBoxStyle {
                slot,
                content,
                align,
                width,
                height,
                values: PageValues(values),
                block: StyloArc::new(lock.wrap(block)),
            });
        }
        PageStyle {
            size,
            margin,
            margin_declared,
            context: PageValues(context),
            background,
            boxes,
        }
    }

    /// The final computed values of a margin box laid out in a synthetic document: its UA defaults and author
    /// declarations, then `extra` (the placement the pipeline computed, CSS declaration text) on top.
    pub fn margin_box_values(
        &self,
        page: &PageStyle,
        b: &MarginBoxStyle,
        extra: &str,
    ) -> PageValues {
        let lock = self.sidecars.lock.clone();
        let guard = lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        let mut block = b.block.read_with(&guard).clone();
        let more = style::properties::parse_style_attribute(
            extra,
            &self.url,
            None,
            self.quirks,
            CssRuleType::Style,
        );
        for (d, imp) in more.declaration_importance_iter() {
            block.push(
                d.clone(),
                if imp.important() {
                    imp
                } else {
                    Importance::Normal
                },
            );
        }
        let locked = StyloArc::new(lock.wrap(block));
        PageValues(
            self.stylist
                .compute_for_declarations::<crate::style::adapter::handle::StyleNode<'_>>(
                    &guards,
                    &page.context.0,
                    locked,
                ),
        )
    }

    /// Computed values of `css` (declaration text) with `parent` (or the initial values) as parent: the root of
    /// a synthetic margin-box document.
    pub fn synthetic_values(&self, parent: Option<&PageValues>, css: &str) -> PageValues {
        let lock = self.sidecars.lock.clone();
        let guard = lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        let block = style::properties::parse_style_attribute(
            css,
            &self.url,
            None,
            self.quirks,
            CssRuleType::Style,
        );
        let locked = StyloArc::new(lock.wrap(block));
        let parent = parent.map_or(&self.initial, |p| &p.0);
        PageValues(
            self.stylist
                .compute_for_declarations::<crate::style::adapter::handle::StyleNode<'_>>(
                    &guards, parent, locked,
                ),
        )
    }

    /// The used `page` property of an element (CSS Paged Media 3 section 6; `None` = `auto`).
    pub fn page_name(&self, doc: &Document, node: NodeId) -> Option<String> {
        let s = self.primary_style(doc, node)?;
        match s.get_box().page.clone() {
            style::values::computed::PageName::PageName(n) => Some(n.0.to_string()),
            style::values::computed::PageName::Auto => None,
        }
    }
}

/// A style snapshot of a synthetic document (the margin boxes of one page): `entries` are its elements'
/// primary styles.
pub fn synthetic_snapshot(
    document: DocumentId,
    entries: Vec<(NodeId, PageValues)>,
    initial: &PageValues,
) -> Arc<StyleSnapshot> {
    StyleSnapshot::synthetic(
        document,
        entries.into_iter().map(|(n, v)| (n, v.0)).collect(),
        initial.0.clone(),
    )
}

impl StyleBridge {
    /// The initial values (the parent of a synthetic document's root).
    pub fn initial_values(&self) -> PageValues {
        PageValues(self.initial.clone())
    }
}
