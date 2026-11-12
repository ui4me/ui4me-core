//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Typed `ComputedStyleView` groups (006 `computed-style-view.md` §2–§3; AD-12; T043). No
//! engine (upstream) type appears in these types. Values are **computed values**: lengths in
//! CSS px (`f32`), percentages as fractions (`0.5` = 50 %), `calc()` as a [`CalcHandle`],
//! colours resolved (`currentColor` already replaced by the element's `color`).
//!
//! The slice exposes the 001 FR-017 consumed set plus the properties consumers must *report*
//! when non-initial (position, offsets, radii; FR-021/FR-022). Everything else is reachable by
//! name through [`super::ComputedStyleView::non_initial_properties`] for reporting.

pub use super::calc::CalcHandle;
pub use super::paint_groups::*;
use style::properties::ComputedValues;
use style::values::computed::LengthPercentage as StyloLp;
use style::values::computed::length_percentage::Unpacked;
use style::values::generics::NonNegative;
use style::values::generics::length::{GenericMargin, GenericMaxSize, GenericSize};
use style::values::generics::position::GenericInset;
use style_traits::ToCss;

// ------------------------------------------------------------------ value types

/// `Px` | `Percent` (fraction) | `Calc`.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentage {
    Px(f32),
    /// A fraction: `50%` is `Percent(0.5)`.
    Percent(f32),
    Calc(CalcHandle),
}

impl LengthPercentage {
    /// Resolve against a percentage basis in px.
    pub fn resolve(&self, basis_px: f32) -> f32 {
        match self {
            LengthPercentage::Px(v) => *v,
            LengthPercentage::Percent(p) => p * basis_px,
            LengthPercentage::Calc(h) => h.resolve(basis_px),
        }
    }
    pub fn is_zero(&self) -> bool {
        matches!(self, LengthPercentage::Px(v) | LengthPercentage::Percent(v) if *v == 0.0)
    }
}

/// `width`/`height`/`min-*`/`max-*` (`None` only for `max-*`).
#[derive(Clone, Debug, PartialEq)]
pub enum Size {
    Auto,
    None,
    LengthPercentage(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent(Option<LengthPercentage>),
    /// `stretch` and `-webkit-fill-available`.
    Stretch,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Margin {
    Auto,
    LengthPercentage(LengthPercentage),
}

/// `top`/`right`/`bottom`/`left`.
#[derive(Clone, Debug, PartialEq)]
pub enum Inset {
    Auto,
    LengthPercentage(LengthPercentage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

/// CSS Color 4 colour spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98,
    ProPhoto,
    Rec2020,
    Lab,
    Lch,
    Oklab,
    Oklch,
    XyzD50,
    XyzD65,
    Hsl,
    Hwb,
}

/// A computed colour in its own space, with an sRGB conversion done at view time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub space: ColorSpace,
    pub c0: f32,
    pub c1: f32,
    pub c2: f32,
    pub alpha: f32,
    srgb: [f32; 4],
}

impl Color {
    /// Non-premultiplied sRGB `[r, g, b, a]` in 0..=1 (gamut-clamped).
    pub fn to_srgb_f32(self) -> [f32; 4] {
        self.srgb
    }
    /// 8-bit sRGB `[r, g, b, a]`.
    pub fn to_srgb_u8(self) -> [u8; 4] {
        self.srgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
    pub fn is_transparent(&self) -> bool {
        self.alpha == 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayOutside {
    None,
    Contents,
    Block,
    Inline,
    TableCaption,
    InternalTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayInside {
    None,
    Contents,
    Flow,
    FlowRoot,
    Flex,
    Grid,
    Table,
    /// table-row-group, table-row, table-cell, … (008 FR-002 table-internal values).
    TableInternal,
    /// Any other inside type (ruby, -webkit-box, grid-lanes): reported by consumers.
    Other,
}

/// Computed `display`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Display {
    pub outside: DisplayOutside,
    pub inside: DisplayInside,
    pub list_item: bool,
    keyword: String,
}

impl Display {
    /// The single-keyword serialisation (`inline flow-root` → `inline-block`).
    pub fn keyword(&self) -> &str {
        &self.keyword
    }
    /// `css.display.<value>` (006 css-feature-profile §3; XR-025).
    pub fn feature_id(&self) -> String {
        format!("css.display.{}", self.keyword)
    }
    pub fn is_none(&self) -> bool {
        self.outside == DisplayOutside::None && self.inside == DisplayInside::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Float {
    None,
    Left,
    Right,
    InlineStart,
    InlineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    None,
    Hidden,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
    Math,
    Emoji,
}

/// One entry of the computed `font-family` list (007 FR-050 mapping input).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Named(String),
    Generic(GenericFamily),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Px(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
    /// The legacy `-webkit-left`, `-webkit-right` and `-webkit-center` keywords (Compat Standard;
    /// the `align` attribute and the `center` element): they align the text like `left`, `right`
    /// and `center` and additionally align block-level children (008 FR-013).
    WebkitLeft,
    WebkitRight,
    WebkitCenter,
    /// Engine-internal keywords (`match-parent`, …).
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WhiteSpaceCollapse {
    Collapse,
    Preserve,
    PreserveBreaks,
    BreakSpaces,
    PreserveSpaces,
}

/// The case part of `text-transform`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextTransformCase {
    #[default]
    None,
    Capitalize,
    Uppercase,
    Lowercase,
}

/// `text-transform` (CSS Text 3 section 2.1): a case keyword with the `full-width` and
/// `full-size-kana` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextTransformValue {
    pub case: TextTransformCase,
    pub full_width: bool,
    pub full_size_kana: bool,
}

/// `tab-size` (CSS Text 3 section 4.2): a number of space advances or an absolute length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSize {
    Number(f32),
    Length(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

/// `word-break`; the legacy `break-word` keyword is normalized to `normal` plus
/// `overflow-wrap: anywhere` when exporting the computed text properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
}

/// `hyphens` (CSS Text 3 section 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hyphens {
    None,
    Manual,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineBreak {
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// One side of `text-overflow`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextOverflowSide {
    Clip,
    Ellipsis,
    String(String),
}

/// `text-overflow`. A single specified value is the **end** side (`first` is `Clip`, `logical`
/// true); two values are the physical left and right sides.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextOverflow {
    pub first: TextOverflowSide,
    pub second: TextOverflowSide,
    pub logical: bool,
}

impl TextOverflow {
    pub const CLIP: TextOverflow = TextOverflow {
        first: TextOverflowSide::Clip,
        second: TextOverflowSide::Clip,
        logical: true,
    };

    /// The (inline-start, inline-end) sides for a paragraph direction.
    pub fn start_end(&self, rtl: bool) -> (&TextOverflowSide, &TextOverflowSide) {
        match (self.logical, rtl) {
            (true, _) => (&self.first, &self.second),
            (false, false) => (&self.first, &self.second),
            (false, true) => (&self.second, &self.first),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Ltr,
    Rtl,
}

// ------------------------------------------------------------------ groups

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayFlow {
    pub display: Display,
    /// `display` before computed-value fix-ups: for a flex/grid item, the cascaded `display`
    /// before item blockification (read from the element's matched rules, since the engine
    /// overwrites its own `original_display` on item and root fix-ups); otherwise the engine's
    /// `original_display` (before float/abspos blockification). Consumers that box an
    /// unsupported container as `inline` (001 FR-021) use this for its children.
    pub original_display: Display,
    pub position: Position,
    /// The computed keyword, for `css.property.position` reports.
    pub position_keyword: &'static str,
    pub float: Float,
    pub clear_keyword: String,
    pub visibility: Visibility,
    pub box_sizing_border_box: bool,
    pub order: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sizing {
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: Size,
    pub max_height: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxModel {
    pub margin: Edges<Margin>,
    pub padding: Edges<LengthPercentage>,
    /// Computed border widths in px (0 when the style is `none`/`hidden`).
    pub border_width: Edges<f32>,
    pub border_style: Edges<BorderStyle>,
    pub border_color: Edges<Color>,
    /// `(horizontal, vertical)` radius per corner.
    pub border_radius: Corners<(LengthPercentage, LengthPercentage)>,
}

/// The side of an anchor an `anchor()` inset refers to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnchorSide {
    Top,
    Right,
    Bottom,
    Left,
    Center,
    Start,
    End,
    SelfStart,
    SelfEnd,
    Inside,
    Outside,
    /// Percentage between the start (0) and end (1) sides.
    Percent(f32),
}

/// One `anchor([<anchor-name>] <side>)` inset.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorInset {
    /// `None`: the default anchor (`position-anchor`, else the implicit anchor).
    pub name: Option<String>,
    pub side: AnchorSide,
}

/// The anchor-positioning properties of a box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorProps {
    /// `anchor-name`.
    pub names: Vec<String>,
    /// `position-anchor: <dashed-ident>`.
    pub position_anchor: Option<String>,
    /// `top`, `right`, `bottom`, `left` when they are `anchor()` functions.
    pub insets: [Option<AnchorInset>; 4],
}

impl AnchorProps {
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.insets.iter().all(Option::is_none)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PositionOffsets {
    pub inset: Edges<Inset>,
    /// CSS Anchor Positioning subset (F0808): `anchor-name`, `position-anchor` and `anchor()` insets.
    pub anchor: AnchorProps,
    /// `None` = `auto`.
    pub z_index: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontGroup {
    pub family: Vec<FontFamily>,
    pub size_px: f32,
    pub weight: f32,
    pub italic: bool,
    /// Oblique angle in degrees (`Some(14.0)` for `oblique`), `None` for normal/italic.
    pub oblique_deg: Option<f32>,
    /// `font-width` as a percentage (100 = normal).
    pub width_percent: f32,
    pub line_height: LineHeight,
}

/// Line-level layout switches (015 TXT-4): `text-wrap-style` and `line-clamp`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineFlow {
    /// `text-wrap-style: balance`.
    pub balance: bool,
    /// `text-wrap-style: pretty` (Off in the profile: never computed).
    pub pretty: bool,
    /// The legacy clamp the reference performs: `-webkit-line-clamp: N` on a `display: -webkit-box` with
    /// `-webkit-box-orient: vertical` (Stylo turns such a box into a `flow-root` one at computed-value time).
    pub legacy_clamp: Option<u32>,
    /// The standard `line-clamp` (CSS Overflow 4, not the legacy `-webkit-line-clamp`) is not `none`: the
    /// reference ignores it, this engine reports it.
    pub standard_clamp: bool,
}

pub(crate) fn line_flow(cv: &ComputedValues) -> LineFlow {
    use style::values::specified::box_::DisplayInside;
    let b = cv.get_box();
    let lc = b.get_line_clamp();
    let it = cv.get_inherited_text();
    let style = css(&it.text_wrap_style);
    let lines = lc.max_lines.lines_value().map(|n| n.0.max(1) as u32);
    let webkit_box = b.original_display.inside() == DisplayInside::WebkitBox;
    let vertical = css(&cv.get_xul()._moz_box_orient) == "vertical";
    LineFlow {
        balance: style == "balance",
        pretty: style == "pretty",
        legacy_clamp: if lc.webkit_legacy && webkit_box && vertical {
            lines
        } else {
            None
        },
        standard_clamp: !lc.is_none() && !lc.webkit_legacy,
    }
}

/// The OpenType side of the font properties (CSS Fonts 4; 015 TXT-2): `font-variation-settings`,
/// `font-optical-sizing`, and the OpenType features that `font-kerning`, `font-variant-*` and
/// `font-feature-settings` ask for, in the order the shaper applies them (the variant properties
/// first, `font-feature-settings` last, so that it wins on a tag both name; Blink's order).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FontOpenType {
    /// `font-variation-settings` (`normal` is empty): axis tag and value.
    pub variations: Vec<([u8; 4], f32)>,
    /// `font-optical-sizing: none` (`auto` is the initial value).
    pub optical_sizing_none: bool,
    /// OpenType features: tag and value (0 off, 1 on, n selects an alternate).
    pub features: Vec<([u8; 4], u32)>,
    /// `font-variant-caps` is `small-caps` or `all-small-caps` (the box may synthesize them).
    pub small_caps: bool,
    pub all_small_caps: bool,
    /// `font-synthesis-small-caps: none`.
    pub no_synthesis_small_caps: bool,
    /// `font-synthesis-weight: none` and `font-synthesis-style: none`.
    pub no_synthesis_weight: bool,
    pub no_synthesis_style: bool,
    /// `font-synthesis-position: none`.
    pub no_synthesis_position: bool,
}

pub(crate) fn font_open_type(cv: &ComputedValues) -> FontOpenType {
    let f = cv.get_font();
    let tag = |t: u32| t.to_be_bytes();
    let variations = f
        .font_variation_settings
        .0
        .iter()
        .map(|v| (tag(v.tag.0), v.value))
        .collect();
    let mut features: Vec<([u8; 4], u32)> = Vec::new();
    let mut on = |t: &[u8; 4], v: u32| features.push((*t, v));
    // font-kerning: `none` switches kerning off, `normal` on; `auto` leaves it to the shaper (on).
    match css(&f.font_kerning).as_str() {
        "none" => on(b"kern", 0),
        "normal" => on(b"kern", 1),
        _ => {}
    }
    let ligs = css(&f.font_variant_ligatures);
    if ligs == "none" {
        for t in [b"liga", b"clig", b"dlig", b"hlig", b"calt"] {
            on(t, 0);
        }
    } else {
        for w in ligs.split_whitespace() {
            match w {
                "common-ligatures" => {
                    on(b"liga", 1);
                    on(b"clig", 1);
                }
                "no-common-ligatures" => {
                    on(b"liga", 0);
                    on(b"clig", 0);
                }
                "discretionary-ligatures" => on(b"dlig", 1),
                "no-discretionary-ligatures" => on(b"dlig", 0),
                "historical-ligatures" => on(b"hlig", 1),
                "no-historical-ligatures" => on(b"hlig", 0),
                "contextual" => on(b"calt", 1),
                "no-contextual" => on(b"calt", 0),
                _ => {}
            }
        }
    }
    let caps = css(&f.font_variant_caps);
    match caps.as_str() {
        "small-caps" => on(b"smcp", 1),
        "all-small-caps" => {
            on(b"smcp", 1);
            on(b"c2sc", 1);
        }
        "petite-caps" => on(b"pcap", 1),
        "all-petite-caps" => {
            on(b"pcap", 1);
            on(b"c2pc", 1);
        }
        "unicase" => on(b"unic", 1),
        "titling-caps" => on(b"titl", 1),
        _ => {}
    }
    for w in css(&f.font_variant_numeric).split_whitespace() {
        match w {
            "lining-nums" => on(b"lnum", 1),
            "oldstyle-nums" => on(b"onum", 1),
            "proportional-nums" => on(b"pnum", 1),
            "tabular-nums" => on(b"tnum", 1),
            "diagonal-fractions" => on(b"frac", 1),
            "stacked-fractions" => on(b"afrc", 1),
            "ordinal" => on(b"ordn", 1),
            "slashed-zero" => on(b"zero", 1),
            _ => {}
        }
    }
    for w in css(&f.font_variant_east_asian).split_whitespace() {
        match w {
            "jis78" => on(b"jp78", 1),
            "jis83" => on(b"jp83", 1),
            "jis90" => on(b"jp90", 1),
            "jis04" => on(b"jp04", 1),
            "simplified" => on(b"smpl", 1),
            "traditional" => on(b"trad", 1),
            "full-width" => on(b"fwid", 1),
            "proportional-width" => on(b"pwid", 1),
            "ruby" => on(b"ruby", 1),
            _ => {}
        }
    }
    match css(&f.font_variant_position).as_str() {
        "sub" => on(b"subs", 1),
        "super" => on(b"sups", 1),
        _ => {}
    }
    for v in f.font_feature_settings.0.iter() {
        features.push((tag(v.tag.0), v.value.max(0) as u32));
    }
    FontOpenType {
        variations,
        optical_sizing_none: css(&f.font_optical_sizing) == "none",
        features,
        small_caps: matches!(caps.as_str(), "small-caps" | "all-small-caps"),
        all_small_caps: caps == "all-small-caps",
        no_synthesis_small_caps: css(&f.font_synthesis_small_caps) == "none",
        no_synthesis_weight: css(&f.font_synthesis_weight) == "none",
        no_synthesis_style: css(&f.font_synthesis_style) == "none",
        no_synthesis_position: css(&f.font_synthesis_position) == "none",
    }
}

/// `unicode-bidi` (CSS Writing Modes 4 section 2.4). `-webkit-isolate`, `-webkit-isolate-override` and
/// `-webkit-plaintext` are the same values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

/// `writing-mode` (CSS Writing Modes 4 section 3.1). The SVG 1.1 keywords compute to these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WritingMode {
    #[default]
    HorizontalTb,
    VerticalRl,
    VerticalLr,
    SidewaysRl,
    SidewaysLr,
}

impl WritingMode {
    pub fn is_vertical(self) -> bool {
        self != WritingMode::HorizontalTb
    }
}

/// `text-orientation` (CSS Writing Modes 4 section 5.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InlineText {
    pub text_align: TextAlign,
    /// `text-align-last`; `None` is `auto`.
    pub text_align_last: Option<TextAlign>,
    pub white_space_collapse: WhiteSpaceCollapse,
    /// `text-wrap-mode: wrap`.
    pub text_wrap: bool,
    pub direction: Direction,
    /// `unicode-bidi` (not inherited; CSS Writing Modes 4 section 2.4).
    pub unicode_bidi: UnicodeBidi,
    /// `writing-mode` is a vertical one (`vertical-*`, `sideways-*`).
    pub vertical_writing_mode: bool,
    /// `writing-mode` (inherited; CSS Writing Modes 4 section 3.1; 015 WM-1).
    pub writing_mode: WritingMode,
    /// `text-orientation` (inherited; CSS Writing Modes 4 section 5.1).
    pub text_orientation: TextOrientation,
    /// `text-combine-upright: all` (inherited; CSS Writing Modes 4 section 9.1).
    pub text_combine_upright: bool,
    pub color: Color,
    pub letter_spacing_px: f32,
    pub word_spacing: LengthPercentage,
    pub text_indent: LengthPercentage,
    pub tab_size: TabSize,
    pub text_transform: TextTransformValue,
    pub overflow_wrap: OverflowWrap,
    pub word_break: WordBreak,
    pub line_break: LineBreak,
    pub text_overflow: TextOverflow,
    pub hyphens: Hyphens,
    /// `hyphenate-character`: `None` is `auto` (U+2010, or `-` when the font has no U+2010).
    pub hyphenate_character: Option<String>,
}

/// `text-decoration-style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextDecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// `text-decoration-thickness`.
#[derive(Clone, Debug, PartialEq)]
pub enum DecorationThickness {
    Auto,
    FromFont,
    LengthPercentage(LengthPercentage),
}

/// The `text-decoration` longhands of one element (CSS Text Decoration 3). Not inherited; 008/009
/// propagate the lines to the in-flow descendants (decorating boxes). `color` is resolved at this
/// element (`currentcolor` is its own `color`). `text-underline-offset`, `text-underline-position`
/// and `text-decoration-skip-ink` are off in the profile (reported by the cascade).
#[derive(Clone, Debug, PartialEq)]
pub struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    /// `blink`: computed, never animated (CSS Text Decoration 3: UAs may ignore it); reported by layout.
    pub blink: bool,
    pub style: TextDecorationStyle,
    pub color: Color,
    pub thickness: DecorationThickness,
    /// `text-underline-offset`; `None` is `auto`.
    pub underline_offset: Option<LengthPercentage>,
    /// `text-underline-position`.
    pub underline_position: UnderlinePosition,
}

/// `text-underline-position` (CSS Text Decoration 4): `auto`, or `from-font` / `under`, with the
/// vertical-text sides (`left`, `right`) kept for reporting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnderlinePosition {
    pub from_font: bool,
    pub under: bool,
    pub left: bool,
    pub right: bool,
}

impl TextDecoration {
    /// Whether the element has at least one line to draw.
    pub fn has_line(&self) -> bool {
        self.underline || self.overline || self.line_through
    }
}

/// The `paint` group (contract §3). Background layers, shadows, outline, filter and transform
/// are typed in `paint_groups` (lane L-STY-r2); `mix-blend-mode`/`isolation` are carried as
/// keywords. `clip`, `image-rendering`, `will-change` and
/// `box-decoration-break` are reachable through `non_initial_properties` until a consumer needs
/// them typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    /// Inherited accent color, or None for the platform default (`auto`).
    pub accent_color: Option<Color>,
    /// CSS scrollbar width in CSS pixels (0 = none; overlay scrollbars).
    pub scrollbar_width: f32,
    /// Inherited thumb and track colors, or theme defaults for `auto`.
    pub scrollbar_colors: Option<(Color, Color)>,
    pub color: Color,
    pub background_color: Color,
    pub opacity: f32,
    /// `background-image` layers, first layer topmost; empty never (a single `none` layer is the initial value).
    pub background_layers: Vec<BackgroundLayer>,
    /// `mask-image` layers (`mask-*`), first layer topmost; a single `none` layer is the initial value.
    pub mask_layers: Vec<super::paint_groups::MaskLayer>,
    /// `box-shadow` layers, first layer topmost.
    pub box_shadow: Vec<BoxShadow>,
    /// `text-shadow` layers, first layer topmost (inherited).
    pub text_shadow: Vec<TextShadow>,
    pub outline: Outline,
    pub filter: Vec<FilterFunction>,
    pub backdrop_filter: Vec<FilterFunction>,
    pub transform: Transform,
    /// `mix-blend-mode` keyword (`normal`, `multiply`, ...).
    pub mix_blend_mode: &'static str,
    /// `isolation: isolate`.
    pub isolation_isolate: bool,
    /// `image-rendering: pixelated | crisp-edges` (nearest-neighbour sampling).
    pub image_rendering_pixelated: bool,
    /// `object-fit` keyword (`fill`, `contain`, `cover`, `none`, `scale-down`). Listed in the
    /// contract's `sizing` group; carried here so that paint need not touch 008's group.
    pub object_fit: &'static str,
    /// `object-position` as computed length-percentages.
    pub object_position: (LengthPercentage, LengthPercentage),
    /// The `border-image-*` longhands; `None` for `border-image-source: none`.
    pub border_image: Option<super::paint_groups::BorderImage>,
    /// `clip-path` (basic shapes and geometry boxes typed; `url()` never reaches here).
    pub clip_path: super::paint_groups::ClipPath,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewFlags {
    pub depends_on_font_metrics: bool,
    pub uses_viewport_units: bool,
    pub is_animating: bool,
    pub has_generated_content: bool,
}

// ------------------------------------------------------------------ extraction (crate-internal)

pub(crate) fn lp(epoch: u64, v: &StyloLp) -> LengthPercentage {
    match v.unpack() {
        Unpacked::Length(l) => LengthPercentage::Px(l.px()),
        Unpacked::Percentage(p) => LengthPercentage::Percent(p.0),
        Unpacked::Calc(_) => LengthPercentage::Calc(CalcHandle::new(epoch, v)),
    }
}

pub(crate) fn size(epoch: u64, s: &GenericSize<NonNegative<StyloLp>>) -> Size {
    match s {
        GenericSize::LengthPercentage(v) => Size::LengthPercentage(lp(epoch, &v.0)),
        GenericSize::Auto => Size::Auto,
        GenericSize::MaxContent => Size::MaxContent,
        GenericSize::MinContent => Size::MinContent,
        GenericSize::FitContent => Size::FitContent(None),
        GenericSize::FitContentFunction(v) => Size::FitContent(Some(lp(epoch, &v.0))),
        GenericSize::Stretch | GenericSize::WebkitFillAvailable => Size::Stretch,
        // Anchor positioning is off in the profile; never computed.
        GenericSize::AnchorSizeFunction(_) | GenericSize::AnchorContainingCalcFunction(_) => {
            Size::Auto
        }
    }
}

pub(crate) fn max_size(epoch: u64, s: &GenericMaxSize<NonNegative<StyloLp>>) -> Size {
    match s {
        GenericMaxSize::LengthPercentage(v) => Size::LengthPercentage(lp(epoch, &v.0)),
        GenericMaxSize::None => Size::None,
        GenericMaxSize::MaxContent => Size::MaxContent,
        GenericMaxSize::MinContent => Size::MinContent,
        GenericMaxSize::FitContent => Size::FitContent(None),
        GenericMaxSize::FitContentFunction(v) => Size::FitContent(Some(lp(epoch, &v.0))),
        GenericMaxSize::Stretch | GenericMaxSize::WebkitFillAvailable => Size::Stretch,
        GenericMaxSize::AnchorSizeFunction(_) | GenericMaxSize::AnchorContainingCalcFunction(_) => {
            Size::None
        }
    }
}

pub(crate) fn margin(epoch: u64, m: &GenericMargin<StyloLp>) -> Margin {
    match m {
        GenericMargin::Auto => Margin::Auto,
        GenericMargin::LengthPercentage(v) => Margin::LengthPercentage(lp(epoch, v)),
        GenericMargin::AnchorSizeFunction(_) | GenericMargin::AnchorContainingCalcFunction(_) => {
            Margin::Auto
        }
    }
}

pub(crate) fn inset<P>(epoch: u64, i: &GenericInset<P, StyloLp>) -> Inset {
    match i {
        GenericInset::Auto => Inset::Auto,
        GenericInset::LengthPercentage(v) => Inset::LengthPercentage(lp(epoch, v)),
        _ => Inset::Auto,
    }
}

pub(crate) fn color(c: style::color::AbsoluteColor) -> Color {
    use style::color::ColorSpace as S;
    let space = match c.color_space {
        S::Srgb => ColorSpace::Srgb,
        S::Hsl => ColorSpace::Hsl,
        S::Hwb => ColorSpace::Hwb,
        S::Lab => ColorSpace::Lab,
        S::Lch => ColorSpace::Lch,
        S::Oklab => ColorSpace::Oklab,
        S::Oklch => ColorSpace::Oklch,
        S::SrgbLinear => ColorSpace::SrgbLinear,
        S::DisplayP3 | S::DisplayP3Linear => ColorSpace::DisplayP3,
        S::A98Rgb => ColorSpace::A98,
        S::ProphotoRgb => ColorSpace::ProPhoto,
        S::Rec2020 => ColorSpace::Rec2020,
        S::XyzD50 => ColorSpace::XyzD50,
        S::XyzD65 => ColorSpace::XyzD65,
    };
    let raw = *c.raw_components();
    let s = c.into_srgb_legacy();
    let sr = *s.raw_components();
    // Out-of-gamut colours (`oklch()`, `lab()`, `color(display-p3 ...)` beyond sRGB) are clipped
    // per channel into sRGB (the "clip" method of CSS Color 4 section 13.1), and `none`/NaN
    // components are 0, so that every consumer receives channels in 0..=1 (doc of `to_srgb_f32`).
    let clip = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    Color {
        space,
        c0: raw[0],
        c1: raw[1],
        c2: raw[2],
        alpha: c.alpha,
        srgb: [clip(sr[0]), clip(sr[1]), clip(sr[2]), clip(s.alpha)],
    }
}

pub(crate) fn css<T: ToCss>(v: &T) -> String {
    v.to_css_string()
}

pub(crate) fn display(cv: &ComputedValues) -> Display {
    display_of(cv.get_box().display)
}

pub(crate) fn display_of(d: style::values::specified::Display) -> Display {
    use style::values::specified::box_::{DisplayInside as I, DisplayOutside as O};
    let outside = match d.outside() {
        O::None => {
            if d.inside() == I::Contents {
                DisplayOutside::Contents
            } else {
                DisplayOutside::None
            }
        }
        O::Inline => DisplayOutside::Inline,
        O::Block => DisplayOutside::Block,
        O::TableCaption => DisplayOutside::TableCaption,
        O::InternalTable => DisplayOutside::InternalTable,
    };
    let inside = match d.inside() {
        I::None => DisplayInside::None,
        I::Contents => DisplayInside::Contents,
        I::Flow => DisplayInside::Flow,
        I::FlowRoot => DisplayInside::FlowRoot,
        I::Flex => DisplayInside::Flex,
        I::Grid => DisplayInside::Grid,
        I::Table => DisplayInside::Table,
        I::TableRowGroup
        | I::TableColumn
        | I::TableColumnGroup
        | I::TableHeaderGroup
        | I::TableFooterGroup
        | I::TableRow
        | I::TableCell => DisplayInside::TableInternal,
        _ => DisplayInside::Other,
    };
    Display {
        outside,
        inside,
        list_item: d.is_list_item(),
        keyword: css(&d),
    }
}

pub(crate) fn border_style_of(s: style::values::specified::BorderStyle) -> BorderStyle {
    border_style(s)
}

fn border_style(s: style::values::specified::BorderStyle) -> BorderStyle {
    use style::values::specified::BorderStyle as B;
    match s {
        B::None => BorderStyle::None,
        B::Hidden => BorderStyle::Hidden,
        B::Solid => BorderStyle::Solid,
        B::Dotted => BorderStyle::Dotted,
        B::Dashed => BorderStyle::Dashed,
        B::Double => BorderStyle::Double,
        B::Groove => BorderStyle::Groove,
        B::Ridge => BorderStyle::Ridge,
        B::Inset => BorderStyle::Inset,
        B::Outset => BorderStyle::Outset,
    }
}

pub(crate) fn display_flow(
    cv: &ComputedValues,
    cascaded: Option<style::values::specified::Display>,
) -> DisplayFlow {
    let b = cv.get_box();
    let (position, position_keyword) = match css(&b.position).as_str() {
        "relative" => (Position::Relative, "relative"),
        "absolute" => (Position::Absolute, "absolute"),
        "fixed" => (Position::Fixed, "fixed"),
        "sticky" => (Position::Sticky, "sticky"),
        _ => (Position::Static, "static"),
    };
    let float = match css(&b.float).as_str() {
        "left" => Float::Left,
        "right" => Float::Right,
        "inline-start" => Float::InlineStart,
        "inline-end" => Float::InlineEnd,
        _ => Float::None,
    };
    let visibility = match css(&cv.get_inherited_box().visibility).as_str() {
        "hidden" => Visibility::Hidden,
        "collapse" => Visibility::Collapse,
        _ => Visibility::Visible,
    };
    DisplayFlow {
        display: display(cv),
        original_display: display_of(cascaded.unwrap_or(cv.get_box().original_display)),
        position,
        position_keyword,
        float,
        clear_keyword: css(&b.clear),
        visibility,
        box_sizing_border_box: css(&cv.get_position().box_sizing) == "border-box",
        order: cv.get_position().order,
    }
}

pub(crate) fn sizing(epoch: u64, cv: &ComputedValues) -> Sizing {
    let p = cv.get_position();
    Sizing {
        width: size(epoch, &p.width),
        height: size(epoch, &p.height),
        min_width: size(epoch, &p.min_width),
        min_height: size(epoch, &p.min_height),
        max_width: max_size(epoch, &p.max_width),
        max_height: max_size(epoch, &p.max_height),
    }
}

pub(crate) fn box_model(epoch: u64, cv: &ComputedValues) -> BoxModel {
    let m = cv.get_margin();
    let p = cv.get_padding();
    let b = cv.get_border();
    let col = |c: &style::values::computed::Color| color(cv.resolve_color(c));
    let radius = |r: &style::values::computed::BorderCornerRadius| {
        (lp(epoch, &r.0.width.0), lp(epoch, &r.0.height.0))
    };
    BoxModel {
        margin: Edges {
            top: margin(epoch, &m.margin_top),
            right: margin(epoch, &m.margin_right),
            bottom: margin(epoch, &m.margin_bottom),
            left: margin(epoch, &m.margin_left),
        },
        padding: Edges {
            top: lp(epoch, &p.padding_top.0),
            right: lp(epoch, &p.padding_right.0),
            bottom: lp(epoch, &p.padding_bottom.0),
            left: lp(epoch, &p.padding_left.0),
        },
        border_width: Edges {
            top: b.border_top_width.0.to_f32_px(),
            right: b.border_right_width.0.to_f32_px(),
            bottom: b.border_bottom_width.0.to_f32_px(),
            left: b.border_left_width.0.to_f32_px(),
        },
        border_style: Edges {
            top: border_style(b.border_top_style),
            right: border_style(b.border_right_style),
            bottom: border_style(b.border_bottom_style),
            left: border_style(b.border_left_style),
        },
        border_color: Edges {
            top: col(&b.border_top_color),
            right: col(&b.border_right_color),
            bottom: col(&b.border_bottom_color),
            left: col(&b.border_left_color),
        },
        border_radius: Corners {
            top_left: radius(&b.border_top_left_radius),
            top_right: radius(&b.border_top_right_radius),
            bottom_right: radius(&b.border_bottom_right_radius),
            bottom_left: radius(&b.border_bottom_left_radius),
        },
    }
}

fn anchor_inset(
    i: &GenericInset<style::values::computed::Percentage, StyloLp>,
) -> Option<AnchorInset> {
    use style::values::generics::position::{AnchorSideKeyword as K, GenericAnchorSide};
    let GenericInset::AnchorFunction(f) = i else {
        return None;
    };
    let side = match &f.side {
        GenericAnchorSide::Percentage(p) => AnchorSide::Percent(p.0),
        GenericAnchorSide::Keyword(k) => match k {
            K::Inside => AnchorSide::Inside,
            K::Outside => AnchorSide::Outside,
            K::Top => AnchorSide::Top,
            K::Left => AnchorSide::Left,
            K::Right => AnchorSide::Right,
            K::Bottom => AnchorSide::Bottom,
            K::Start => AnchorSide::Start,
            K::End => AnchorSide::End,
            K::SelfStart => AnchorSide::SelfStart,
            K::SelfEnd => AnchorSide::SelfEnd,
            K::Center => AnchorSide::Center,
        },
    };
    let name = f.target_element.value.0.to_string();
    Some(AnchorInset {
        name: (!name.is_empty()).then_some(name),
        side,
    })
}

pub(crate) fn anchor_props(cv: &ComputedValues) -> AnchorProps {
    use style::values::specified::position::PositionAnchorKeyword;
    let p = cv.get_position();
    let names = cv
        .get_box()
        .anchor_name
        .value
        .0
        .iter()
        .map(|i| i.0.to_string())
        .collect();
    let position_anchor = match &p.position_anchor.value {
        PositionAnchorKeyword::Ident(i) => Some(i.0.to_string()),
        _ => None,
    };
    AnchorProps {
        names,
        position_anchor,
        insets: [
            anchor_inset(&p.top),
            anchor_inset(&p.right),
            anchor_inset(&p.bottom),
            anchor_inset(&p.left),
        ],
    }
}

pub(crate) fn position_offsets(epoch: u64, cv: &ComputedValues) -> PositionOffsets {
    let p = cv.get_position();
    let z = css(&p.z_index);
    PositionOffsets {
        anchor: anchor_props(cv),
        inset: Edges {
            top: inset(epoch, &p.top),
            right: inset(epoch, &p.right),
            bottom: inset(epoch, &p.bottom),
            left: inset(epoch, &p.left),
        },
        z_index: z.parse().ok(),
    }
}

pub(crate) fn font(cv: &ComputedValues) -> FontGroup {
    use style::values::computed::font::{GenericFontFamily as G, SingleFontFamily};
    let f = cv.get_font();
    let family = f
        .font_family
        .families
        .iter()
        .filter_map(|s| match s {
            SingleFontFamily::FamilyName(n) => Some(FontFamily::Named(n.name.to_string())),
            SingleFontFamily::Generic(g) => Some(FontFamily::Generic(match g {
                G::Serif => GenericFamily::Serif,
                G::SansSerif => GenericFamily::SansSerif,
                G::Monospace => GenericFamily::Monospace,
                G::Cursive => GenericFamily::Cursive,
                G::Fantasy => GenericFamily::Fantasy,
                G::SystemUi => GenericFamily::SystemUi,
                G::None => return None,
            })),
        })
        .collect();
    let style = f.font_style;
    let italic = style == style::values::computed::font::FontStyle::ITALIC;
    let oblique_deg = (!italic && style != style::values::computed::font::FontStyle::NORMAL)
        .then(|| style.oblique_degrees());
    let line_height = match &f.line_height {
        style::values::generics::font::LineHeight::Normal => LineHeight::Normal,
        style::values::generics::font::LineHeight::Number(n) => LineHeight::Number(n.0),
        style::values::generics::font::LineHeight::Length(l) => LineHeight::Px(l.0.px()),
    };
    FontGroup {
        family,
        size_px: f.font_size.computed_size().px(),
        weight: f.font_weight.value(),
        italic,
        oblique_deg,
        width_percent: f.font_width.to_percentage().0 * 100.0,
        line_height,
    }
}

pub(crate) fn inline_text(epoch: u64, cv: &ComputedValues) -> InlineText {
    let t = cv.get_inherited_text();
    let text_align = match css(&t.text_align).as_str() {
        "start" => TextAlign::Start,
        "end" => TextAlign::End,
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "center" => TextAlign::Center,
        "justify" => TextAlign::Justify,
        // Stylo spells the legacy keywords with the Gecko name; the CSSOM name is `-webkit-*`.
        "-moz-left" | "-webkit-left" => TextAlign::WebkitLeft,
        "-moz-right" | "-webkit-right" => TextAlign::WebkitRight,
        "-moz-center" | "-webkit-center" => TextAlign::WebkitCenter,
        _ => TextAlign::Other,
    };
    let text_align_last = match css(&t.text_align_last).as_str() {
        "start" => Some(TextAlign::Start),
        "end" => Some(TextAlign::End),
        "left" => Some(TextAlign::Left),
        "right" => Some(TextAlign::Right),
        "center" => Some(TextAlign::Center),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    };
    let white_space_collapse = match css(&t.white_space_collapse).as_str() {
        "preserve" => WhiteSpaceCollapse::Preserve,
        "preserve-breaks" => WhiteSpaceCollapse::PreserveBreaks,
        "break-spaces" => WhiteSpaceCollapse::BreakSpaces,
        "preserve-spaces" => WhiteSpaceCollapse::PreserveSpaces,
        _ => WhiteSpaceCollapse::Collapse,
    };
    let direction = if css(&cv.get_inherited_box().direction) == "rtl" {
        Direction::Rtl
    } else {
        Direction::Ltr
    };
    let unicode_bidi = match css(&cv.get_text().unicode_bidi).trim_start_matches("-webkit-") {
        "embed" => UnicodeBidi::Embed,
        "isolate" => UnicodeBidi::Isolate,
        "bidi-override" => UnicodeBidi::BidiOverride,
        "isolate-override" => UnicodeBidi::IsolateOverride,
        "plaintext" => UnicodeBidi::Plaintext,
        _ => UnicodeBidi::Normal,
    };
    let tab_size = {
        // `<number>` (space advances) or an absolute `<length>`; serialised as `8` or `20px`.
        let v = css(&t.tab_size);
        match v.strip_suffix("px") {
            Some(px) => TabSize::Length(px.parse().unwrap_or(0.0)),
            None => TabSize::Number(v.parse().unwrap_or(8.0)),
        }
    };
    let text_transform = {
        let v = css(&t.text_transform);
        let has = |k: &str| v.split_whitespace().any(|w| w == k);
        TextTransformValue {
            case: if has("capitalize") {
                TextTransformCase::Capitalize
            } else if has("uppercase") {
                TextTransformCase::Uppercase
            } else if has("lowercase") {
                TextTransformCase::Lowercase
            } else {
                TextTransformCase::None
            },
            full_width: has("full-width"),
            full_size_kana: has("full-size-kana"),
        }
    };
    let mut overflow_wrap = match css(&t.overflow_wrap).as_str() {
        "break-word" => OverflowWrap::BreakWord,
        "anywhere" => OverflowWrap::Anywhere,
        _ => OverflowWrap::Normal,
    };
    let word_break = match css(&t.word_break).as_str() {
        "break-word" => {
            // CSS Text: legacy break-word has the effect of normal + anywhere,
            // regardless of the author's overflow-wrap value, including min-content.
            overflow_wrap = OverflowWrap::Anywhere;
            WordBreak::Normal
        }
        "break-all" => WordBreak::BreakAll,
        "keep-all" => WordBreak::KeepAll,
        _ => WordBreak::Normal,
    };
    let hyphens = match css(&t.hyphens).as_str() {
        "none" => Hyphens::None,
        "auto" => Hyphens::Auto,
        _ => Hyphens::Manual,
    };
    let hyphenate_character = match &t.hyphenate_character {
        style::values::computed::text::HyphenateCharacter::String(s) => Some(s.to_string()),
        _ => None,
    };
    let line_break = match css(&t.line_break).as_str() {
        "loose" => LineBreak::Loose,
        "normal" => LineBreak::Normal,
        "strict" => LineBreak::Strict,
        "anywhere" => LineBreak::Anywhere,
        _ => LineBreak::Auto,
    };
    let side = |s: &style::values::specified::text::TextOverflowSide| {
        use style::values::specified::text::TextOverflowSide as S;
        match s {
            S::Clip => TextOverflowSide::Clip,
            S::Ellipsis => TextOverflowSide::Ellipsis,
            S::String(a) => TextOverflowSide::String(a.to_string()),
        }
    };
    let to = &cv.get_text().text_overflow;
    let text_overflow = TextOverflow {
        first: side(&to.first),
        second: side(&to.second),
        logical: to.sides_are_logical,
    };
    InlineText {
        text_align,
        text_align_last,
        white_space_collapse,
        text_wrap: css(&t.text_wrap_mode) == "wrap",
        direction,
        unicode_bidi,
        vertical_writing_mode: cv.writing_mode.is_vertical(),
        writing_mode: match css(&cv.get_inherited_box().writing_mode).as_str() {
            "vertical-rl" | "tb-rl" | "tb" => WritingMode::VerticalRl,
            "vertical-lr" => WritingMode::VerticalLr,
            "sideways-rl" => WritingMode::SidewaysRl,
            "sideways-lr" => WritingMode::SidewaysLr,
            _ => WritingMode::HorizontalTb,
        },
        text_orientation: match css(&cv.get_inherited_box().text_orientation).as_str() {
            "upright" => TextOrientation::Upright,
            "sideways" | "sideways-right" => TextOrientation::Sideways,
            _ => TextOrientation::Mixed,
        },
        text_combine_upright: css(&cv.get_inherited_text().text_combine_upright) == "all",
        color: color(t.color),
        letter_spacing_px: t
            .letter_spacing
            .0
            .resolve(style::values::computed::Length::new(0.0))
            .px(),
        word_spacing: lp(epoch, &t.word_spacing),
        text_indent: lp(epoch, &t.text_indent.length),
        tab_size,
        text_transform,
        overflow_wrap,
        word_break,
        line_break,
        text_overflow,
        hyphens,
        hyphenate_character,
    }
}

pub(crate) fn text_decoration(epoch: u64, cv: &ComputedValues) -> TextDecoration {
    use style::values::generics::text::GenericTextDecorationLength as L;
    let t = cv.get_text();
    let line = css(&t.text_decoration_line);
    let has = |k: &str| line.split_whitespace().any(|w| w == k);
    let style = match css(&t.text_decoration_style).as_str() {
        "double" => TextDecorationStyle::Double,
        "dotted" => TextDecorationStyle::Dotted,
        "dashed" => TextDecorationStyle::Dashed,
        "wavy" => TextDecorationStyle::Wavy,
        _ => TextDecorationStyle::Solid,
    };
    let thickness = match &t.text_decoration_thickness {
        L::Auto => DecorationThickness::Auto,
        L::FromFont => DecorationThickness::FromFont,
        L::LengthPercentage(v) => DecorationThickness::LengthPercentage(lp(epoch, v)),
    };
    TextDecoration {
        underline: has("underline"),
        overline: has("overline"),
        line_through: has("line-through"),
        blink: has("blink"),
        style,
        color: color(cv.resolve_color(&t.text_decoration_color)),
        thickness,
        underline_offset: match &cv.get_inherited_text().text_underline_offset {
            style::values::generics::length::GenericLengthPercentageOrAuto::Auto => None,
            style::values::generics::length::GenericLengthPercentageOrAuto::LengthPercentage(v) => {
                Some(lp(epoch, v))
            }
        },
        underline_position: {
            let s = css(&cv.get_inherited_text().text_underline_position);
            let has = |k: &str| s.split_whitespace().any(|w| w == k);
            UnderlinePosition {
                from_font: has("from-font"),
                under: has("under"),
                left: has("left"),
                right: has("right"),
            }
        },
    }
}

pub(crate) fn paint(epoch: u64, cv: &ComputedValues) -> Paint {
    let object_fit = match css(&cv.get_position().object_fit).as_str() {
        "contain" => "contain",
        "cover" => "cover",
        "none" => "none",
        "scale-down" => "scale-down",
        _ => "fill",
    };
    let op = &cv.get_position().object_position;
    Paint {
        accent_color: match &cv.get_inherited_ui().accent_color {
            style::values::generics::color::GenericColorOrAuto::Auto => None,
            style::values::generics::color::GenericColorOrAuto::Color(c) => {
                Some(color(cv.resolve_color(c)))
            }
        },
        scrollbar_width: match css(&cv.get_ui().scrollbar_width).as_str() {
            "none" => 0.0,
            "thin" => 6.0,
            _ => 10.0,
        },
        scrollbar_colors: match &cv.get_inherited_ui().scrollbar_color {
            style::values::generics::ui::GenericScrollbarColor::Auto => None,
            style::values::generics::ui::GenericScrollbarColor::Colors { thumb, track } => Some((
                color(cv.resolve_color(thumb)),
                color(cv.resolve_color(track)),
            )),
        },
        color: color(cv.get_inherited_text().color),
        background_color: color(cv.resolve_color(&cv.get_background().background_color)),
        opacity: cv.get_effects().opacity,
        background_layers: super::paint_groups::background_layers(cv, epoch),
        mask_layers: super::paint_groups::mask_layers(cv, epoch),
        box_shadow: super::paint_groups::box_shadows(cv),
        text_shadow: super::paint_groups::text_shadows(cv),
        outline: super::paint_groups::outline(cv),
        filter: super::paint_groups::filters(cv),
        backdrop_filter: super::paint_groups::backdrop_filters(cv),
        transform: super::paint_groups::transform(cv, epoch),
        mix_blend_mode: super::paint_groups::mix_blend_mode(cv),
        isolation_isolate: css(&cv.get_box().isolation) == "isolate",
        image_rendering_pixelated: matches!(
            css(&cv.get_inherited_box().image_rendering).as_str(),
            "pixelated" | "crisp-edges"
        ),
        object_fit,
        object_position: (lp(epoch, &op.horizontal), lp(epoch, &op.vertical)),
        border_image: super::paint_groups::border_image(cv, epoch),
        clip_path: super::paint_groups::clip_path(cv, epoch),
    }
}
