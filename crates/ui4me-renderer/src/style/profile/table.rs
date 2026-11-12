//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Profile table of `ui4me-css/1` 1.1.0 (006 `contracts/css-feature-profile.md` §4–§6).
//! Adapter-internal: engine pref names appear only here (AD-33), never in U4C contracts.
//!
//! * [`BOOL_PREFS`]/[`INT_PREFS`] are the engine prefs set explicitly at apply time (§4; every
//!   row is set even when equal to the engine default). One macro generates both the data
//!   (digest input, consistency checks) and the `set_pref!` calls, so they cannot drift.
//! * [`FEATURES`] is the public feature list (id, state) of §1–§3/§7.
//! * [`property_feature`] / [`selector_feature`] / [`at_rule_feature`] classify parse-time
//!   drops for reporting (§2): a construct whose feature is Off, Gated-not-passed or
//!   EngineUnavailable is `UNSUPPORTED_FEATURE` with that id.

use ui4me_engine_types::diag::FeatureId;

/// Public feature state (§7 `state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeatureState {
    On,
    Off,
    /// Gated; `gate` not PASS in this build.
    Gated {
        gate: &'static str,
    },
    /// The engine build cannot parse it (§5).
    Unavailable,
    /// Removed or reported by the profile filter (§6).
    Filtered,
}

impl FeatureState {
    pub const fn name(self) -> &'static str {
        match self {
            FeatureState::On => "on",
            FeatureState::Off => "off",
            FeatureState::Gated { .. } => "gated",
            FeatureState::Unavailable => "unavailable",
            FeatureState::Filtered => "filtered",
        }
    }
}

macro_rules! bool_prefs {
    ($( $name:tt = $val:tt ),* $(,)?) => {
        /// Boolean engine prefs set at apply time (§4).
        pub const BOOL_PREFS: &[(&str, bool)] = &[ $( ($name, $val) ),* ];
        pub(crate) fn apply_bool_prefs() {
            $( stylo_static_prefs::set_pref!($name, $val); )*
        }
        /// Read back every boolean pref (consistency check after apply).
        pub(crate) fn read_bool_prefs() -> Vec<(&'static str, bool)> {
            vec![ $( ($name, stylo_static_prefs::pref!($name)) ),* ]
        }
    };
}

bool_prefs! {
    // §4.1 On
    "layout.css.properties-and-values.enabled" = true,
    "layout.css.color-mix-multi-color.enabled" = true,
    "layout.css.alpha-color-function.enabled" = true,
    "layout.css.calc-typed-arithmetic.enabled" = true,
    "layout.css.progress-function.enabled" = true,
    "layout.css.supports.at-rule.enabled" = true,
    "layout.css.revert-rule.enabled" = true,
    "layout.css.marker.restricted" = true,
    "layout.css.text-overflow.enabled" = true,
    "layout.css.counters.enabled" = true,
    "layout.css.appearance.enabled" = true,
    "layout.css.appearance-base.enabled" = false,
    "layout.css.fit-content-function.enabled" = true,
    "layout.css.stretch-size-keyword.enabled" = true,
    "layout.css.webkit-fill-available.enabled" = true,
    "layout.css.webkit-fill-available.all-size-properties.enabled" = true,
    "layout.css.system-ui.enabled" = true,
    "layout.css.display-animations.enabled" = false, // 1.24.0: stays Off (F0902): the box tree drops a display:none element at once, so `display` cannot be held by allow-discrete
    "layout.css.font-tech.enabled" = false,
    // §4.2 Gated (release values)
    "layout.css.has-selector.enabled" = true, // 1.5.0: On (G-STYLE-HAS PASS, RESULT-2; owned invalidation driver has.rs)
    // §4.3 Off
    "layout.css.container-queries.enabled" = true, // 1.6.0: On (@container, container-type, container-name, container, cq* units; carried patch P-006-4 and the live loop, 015 CQ-1..3, lane L-CQ)
    "layout.css.writing-mode.enabled" = true, // 1.13.0: On (vertical writing modes laid out in a logical frame, upright CJK, tate-chu-yoko; 015 WM-1, lane L-WM)
    "layout.css.writing-mode.text-orientation.enabled" = true, // 1.13.0: On (vertical writing modes laid out in a logical frame, upright CJK, tate-chu-yoko; 015 WM-1, lane L-WM)
    "layout.css.writing-mode.sideways.enabled" = true, // 1.13.0: On (vertical writing modes laid out in a logical frame, upright CJK, tate-chu-yoko; 015 WM-1, lane L-WM)
    "layout.css.text-combine-upright.enabled" = true, // 1.13.0: On (vertical writing modes laid out in a logical frame, upright CJK, tate-chu-yoko; 015 WM-1, lane L-WM)
    "layout.css.dir-pseudo-class.enabled" = true, // 1.13.0: On (:dir(ltr|rtl); carried patch P-006-8, register UP-12; 015 WM-2, lane L-WM)
    "layout.css.backdrop-filter.enabled" = true,
    "layout.css.masking.enabled" = true, // 1.2.0: On (mask-image, -mode, -repeat, -position, -size, -clip, -origin, -composite, mask; 010 mask scopes, lane L-DARK-r9)
    "layout.css.contain.enabled" = true, // 1.6.0: On (contain: size, inline-size, layout, paint, style, content, strict; 015 CQ-2, lane L-CQ)
    "layout.css.columns.enabled" = true, // 1.7.0: the multicol and break values compute (015 FRAG-2a, lane L-FRAG; P-006-5); 1.9.0: the rows are On (015 MC-1, FRAG-2c, lane L-MC; P-006-7 adds orphans/widows)
    "layout.css.first-line.enabled" = true, // 1.14.0: On (::first-line, ::first-letter laid out; 015 TXT-5, lane L-PSEUDO)
    "layout.css.at-scope.enabled" = true, // 1.5.0: On (@scope; 015 L-HAS)
    "layout.css.starting-style-at-rules.enabled" = true, // 1.24.0: On (@starting-style before-change style for inserted and newly displayed elements; F0902)
    "layout.css.custom-media.enabled" = false,
    "layout.css.nth-child-of.enabled" = true, // 1.5.0: On (:nth-child(An+B of S); 015 L-HAS)
    "layout.css.line-clamp.enabled" = true, // 1.10.0: On (line-clamp, -webkit-line-clamp, -webkit-box-orient; display: -webkit-box; carried patch P-006-6; 015 TXT-4)
    "layout.css.hyphens.enabled" = true, // 1.12.0: On (hyphens: none | manual | auto, hyphenate-character; Liang patterns for en-US, de, fr, es; 015 TXT-3)
    "layout.css.text-underline.enabled" = true, // 1.10.0: On (text-underline-offset, text-underline-position; 015 TXT-6)
    "layout.css.text-wrap-style.balance.enabled" = true, // 1.10.0: On (text-wrap: balance, text-wrap-style: balance; 015 TXT-4)
    "layout.css.text-wrap-style.pretty.enabled" = false,
    "layout.css.word-break.break-word.enabled" = false,
    "layout.css.white-space-collapse.preserve-spaces.enabled" = false,
    "layout.css.text-indent-keywords.enabled" = false,
    "layout.css.display-grid-lanes.enabled" = false,
    "layout.css.scroll-behavior.enabled" = true, // 1.22.0: On (smooth scrolling; 0.8 F0803)
    "layout.css.scroll-snap.enabled" = true, // 1.22.0: On (scroll-snap-*, scroll-padding, scroll-margin; carried patch P-006-16; 0.8 F0804)
    "layout.css.overscroll-behavior.enabled" = true, // 1.22.0: On (0.8 F0804)
    "layout.css.scrollbar-styling.enabled" = true,
    "layout.css.font-variations.enabled" = true, // 1.10.0: On (font-variation-settings, font-optical-sizing; 015 TXT-2)
    "layout.css.font-size-adjust.enabled" = false,
    "layout.css.font-synthesis.enabled" = true, // 1.10.0: On (font-synthesis shorthand, -weight, -style, -small-caps, -position; 015 TXT-2)
    "layout.css.font-variant-emoji.enabled" = false,
    "layout.css.font-variant-caps.all-values.enabled" = true, // 1.10.0: On (all-small-caps, petite-caps, all-petite-caps; 015 TXT-2)
    "layout.css.background-clip.text.enabled" = true, // 1.4.0: On (background-clip: text and -webkit-background-clip: text; 010 text mask scopes, lane L-PAINT3-r14)
    "layout.css.background-clip.border-area.enabled" = false,
    "layout.css.webkit-text-fill-color.enabled" = false,
    "layout.css.zoom.enabled" = false,
    "layout.css.grid-template-subgrid-value.enabled" = true, // 1.11.0: On (CSS Grid 2 subgrid; 015 GRID-1, lane L-GRID; Taffy P-008-4)
    "layout.css.flexbox-balance.enabled" = false,
    "layout.css.light-dark.images.enabled" = true, // 1.26.0: On (F0909: light-dark(<image>, <image>) computes to the image of the used colour scheme)
    "layout.css.cross-fade.enabled" = false,
    "layout.css.random.enabled" = false,
    "layout.css.tree-counting-functions.enabled" = false,
    "layout.css.basic-shape-shape.enabled" = false,
    "dom.viewTransitions.cross-document.enabled" = false,
    "layout.css.view-transitions.enabled" = false,
    "layout.css.scroll-driven-animations.enabled" = false,
    "layout.css.corner-shape.enabled" = false,
    "layout.css.ellipse-corners.enabled" = true, // 1.3.0: On (ellipse() closest-corner / farthest-corner radii; 010 clip scopes, lane L-PAINT2-r12)
    "layout.css.initial-letter.enabled" = false,
    "layout.css.text-box.enabled" = false,
    "layout.css.text-emphasis.enabled" = false,
    "layout.css.ruby.enabled" = false,
    "layout.css.math-properties.enabled" = false,
    "layout.css.motion-path.enabled" = false,
    "layout.css.forced-color-adjust.enabled" = false,
    "layout.css.text-autospace.enabled" = false,
    "layout.css.dominant-baseline.enabled" = false,
    "layout.css.text-decoration-inset.enabled" = false,
    "layout.css.text-decoration-inset-percentage.enabled" = false,
    "layout.css.anchor-positioning.enabled" = true, // 1.23.0: On (F0808: anchor-name, position-anchor and anchor() insets; position-area, position-try* are dropped and reported)
    "layout.css.content-visibility.enabled" = true, // 1.25.0: On (0.9 F0903; hidden skips contents, auto never skips; carried patch P-006-19)
    "layout.css.contain-intrinsic-size.enabled" = true, // 1.25.0: On (0.9 F0903; <length> and auto <length>; the remembered auto size is not kept)
    "layout.css.field-sizing.enabled" = true, // 1.25.0: On (0.9 F0903; text inputs and textareas; select keeps its size)
    "layout.css.resize.enabled" = true, // 1.25.0: On (0.9 F0903; corner drag sets the inline width/height; no grip is drawn)
    "layout.css.style-queries.enabled" = false,
    "layout.css.scroll-state.enabled" = false,
    "layout.css.font-palette.enabled" = false,
    // Misc. engine flags (§4.3 last row)
    "dom.select.customizable_select.enabled" = false,
    "gfx.font_rendering.opentype_svg.enabled" = false,
    "layout.css.content.alt-text.enabled" = false,
    "layout.css.margin-rules.enabled" = true, // 1.15.0: On (the 16 page-margin boxes of @page; carried patch P-006-9; 015 PAGE-1, lane L-PAGE)
    "layout.css.page-rule.enabled" = true, // 1.15.0: On (@page, size, page; carried patch P-006-9, register UP-13; 015 PAGE-1, lane L-PAGE)
    "layout.css.overflow-moz-hidden-unscrollable.enabled" = false,
    "layout.css.motion-path-url.enabled" = false,
}

/// Integer engine prefs set at apply time. `layout.threads = 0`: no style thread pool
/// (§4.1 "Global: sequential styling"); traversal is `pool = None` in any case (AD-07).
pub const INT_PREFS: &[(&str, i64)] = &[("layout.threads", 0)];

pub(crate) fn apply_int_prefs() {
    stylo_static_prefs::set_pref!("layout.threads", 0);
}

pub(crate) fn read_int_prefs() -> Vec<(&'static str, i64)> {
    vec![(
        "layout.threads",
        i64::from(stylo_static_prefs::pref!("layout.threads")),
    )]
}

/// Prefs deliberately left at the engine default (no feature semantics in sequential mode).
pub const LEFT_AT_DEFAULT: &[&str] = &[
    "layout.css.outline-offset.snapping",
    "layout.css.stylo-local-work-queue.in-main-thread",
    "layout.css.stylo-local-work-queue.in-worker",
    "layout.css.stylo-work-unit-size",
];

use FeatureState::*;

/// Public feature list (§7). Ordered by id; the digest covers it.
pub const FEATURES: &[(&str, FeatureState)] = &[
    ("css.at-rule.container", On),
    ("css.at-rule.counter-style", Unavailable),
    ("css.at-rule.custom-media", Off),
    ("css.at-rule.font-face", On),
    ("css.at-rule.font-feature-values", Filtered),
    ("css.at-rule.font-palette-values", Off),
    ("css.at-rule.import", On),
    ("css.at-rule.keyframes", On), // 1.24.0: F0901, document clock drives transitions and animations
    ("css.at-rule.layer", On),
    ("css.at-rule.media", On),
    ("css.at-rule.namespace", On),
    ("css.at-rule.page", On), // 1.27.0: public pagination, print and PDF export (1.0 F1001/F1002)
    ("css.at-rule.page-vertical", Off), // 1.15.0: pages of a vertical root are laid out horizontally, reported (015 PAGE-1)
    ("css.at-rule.page-width-change", Off), // 1.15.0: a paragraph broken across pages of different widths keeps the earlier width, reported (015 PAGE-1)
    ("css.at-rule.property", On),
    ("css.at-rule.scope", On),
    ("css.at-rule.scope-implicit", Off),
    ("css.at-rule.starting-style", On), // 1.24.0: F0902
    ("css.at-rule.supports", On),
    ("css.at-rule.view-transition", Off),
    ("css.display.-webkit-box", On),
    ("css.display.-webkit-inline-box", On),
    ("css.display.block", On),
    ("css.display.contents", On),
    ("css.display.flex", On),
    ("css.display.flow-root", On),
    ("css.display.grid", On),
    ("css.display.grid-lanes", Off),
    ("css.display.inline", On),
    ("css.display.inline-block", On),
    ("css.display.inline-flex", On),
    ("css.display.inline-grid", On),
    ("css.display.inline-grid-lanes", Off),
    ("css.display.inline-table", On),
    ("css.display.list-item", On),
    ("css.display.none", On),
    ("css.display.ruby", Unavailable),
    ("css.display.ruby-base", Unavailable),
    ("css.display.ruby-base-container", Unavailable),
    ("css.display.ruby-text", Unavailable),
    ("css.display.ruby-text-container", Unavailable),
    ("css.display.table", On),
    ("css.function.calc", On),
    ("css.function.color-mix", On),
    ("css.function.container-style-query", Off),
    ("css.function.counter", On),
    ("css.function.cross-fade", Off),
    ("css.function.fit-content", On),
    ("css.function.light-dark", On),
    ("css.function.light-dark-image", On), // 1.26.0: F0909
    ("css.function.oklch", On),
    ("css.function.progress", On),
    ("css.function.random", Off),
    ("css.media.prefers-reduced-motion", On), // patch P-006-1 adopted (gate X-STYLE-PATCH = PASS; profile 1.1.0)
    ("css.property.-webkit-line-clamp", On),
    ("css.property.-webkit-text-fill-color", Off),
    ("css.property.-webkit-text-stroke", Unavailable),
    ("css.property.accent-color", On),
    ("css.property.anchor-name", On), // 1.23.0: F0808, Stylo patch P-006-17
    ("css.property.animation", On),   // 1.24.0: F0901 (animation-timeline stays Off)
    ("css.property.animation-timeline", Off),
    ("css.property.appearance", On),
    ("css.property.backdrop-filter", On), // 1.26.0: F0909 (painted; only the url() forms stay reported as css.value.filter-url)
    ("css.property.baseline-source", Filtered),
    ("css.property.break-after", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.break-before", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.break-inside", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.column-count", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.column-fill", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.column-rule", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.column-span", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.column-width", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.columns", On), // 1.9.0: multicolumn and column breaks end to end (015 MC-1, FRAG-2c, lane L-MC)
    ("css.property.contain", On),
    ("css.property.contain-intrinsic-size", On), // 1.25.0: 0.9 F0903
    ("css.property.container", On),
    ("css.property.container-name", On),
    ("css.property.container-type", On),
    ("css.property.content-visibility", On), // 1.25.0: 0.9 F0903
    ("css.property.corner-shape", Off),
    ("css.property.counter-increment", On),
    ("css.property.counter-reset", On),
    ("css.property.counter-set", Unavailable),
    ("css.property.field-sizing", On), // 1.25.0: 0.9 F0903
    ("css.property.font-optical-sizing", On),
    ("css.property.font-size-adjust", Off),
    ("css.property.font-synthesis", On),
    ("css.property.font-variant-emoji", Off),
    ("css.property.font-variation-settings", On),
    ("css.property.hyphenate-character", On),
    ("css.property.hyphens", On),
    ("css.property.image-orientation", Unavailable),
    ("css.property.initial-letter", Off), // 1.14.0: stays Off, reported (015 TXT-5: `::first-letter` is laid out without the sunken initial)
    ("css.property.line-clamp", On),
    ("css.property.mask", On),
    ("css.property.overscroll-behavior", On),
    ("css.property.page", On),            // 1.27.0: named pages (1.0 F1001)
    ("css.property.position-anchor", On), // 1.23.0: F0808, Stylo patch P-006-17
    ("css.property.position-area", Off), // 1.23.0: parsed by the engine, dropped and reported (F0808 subset)
    ("css.property.position-try", Off), // 1.23.0: position-try-fallbacks / -order / the shorthand: dropped and reported
    ("css.property.print-color-adjust", Unavailable),
    ("css.property.resize", On), // 1.25.0: 0.9 F0903
    ("css.property.scroll-behavior", On),
    ("css.property.scroll-margin", On),
    ("css.property.scroll-padding", On),
    ("css.property.scroll-snap", On),
    ("css.property.scrollbar-color", On),
    ("css.property.scrollbar-gutter", On),
    ("css.property.scrollbar-width", On),
    ("css.property.shape-outside", Unavailable),
    ("css.property.text-combine-upright", On),
    ("css.property.text-decoration-skip-ink", Unavailable),
    ("css.property.text-orientation", On),
    ("css.property.text-overflow", On),
    ("css.property.text-underline-offset", On),
    ("css.property.text-underline-position", On),
    ("css.property.transition", On), // 1.24.0: F0901
    ("css.property.view-transition", Off),
    ("css.property.writing-mode", On),
    ("css.property.zoom", Off),
    ("css.pseudo.backdrop", On), // 1.16.0: the backdrop of a modal dialog in the top layer (015 HTML-1, lane L-HTML)
    ("css.pseudo.color-swatch", Filtered),
    ("css.pseudo.details-content", Off),
    ("css.pseudo.file-selector-button", Filtered),
    ("css.pseudo.first-letter", On),
    ("css.pseudo.first-line", On),
    ("css.pseudo.marker", On),
    ("css.pseudo.part", Filtered),
    ("css.pseudo.slotted", Filtered),
    ("css.selector.dir", On),
    ("css.selector.has", On),
    ("css.selector.host", Filtered),
    ("css.selector.modal", On),
    ("css.selector.nth-child-of", On),
    ("css.selector.open", On),
    ("css.syntax.nesting", On),
    ("css.unit.cq", On),
    ("css.unit.lh-normal", Off),
    ("css.value.-webkit-fill-available", On),
    ("css.value.background-clip-border-area", Off),
    ("css.value.background-clip-text", On),
    ("css.value.clip-path-url", Filtered),
    ("css.value.filter-url", Filtered),
    ("css.value.font-variant-caps-all", On),
    ("css.value.revert-rule", On),
    ("css.value.shape-function", Off),
    ("css.value.stretch", On),
    ("css.value.subgrid", On),
    ("css.value.system-ui", On),
    ("css.value.text-indent-each-line", Off),
    ("css.value.text-indent-hanging", Off),
    ("css.value.text-wrap-balance", On),
    ("css.value.text-wrap-pretty", Off),
    ("css.value.text-wrap-style-stable", On),
    ("css.value.white-space-collapse-preserve-spaces", Off),
    ("css.value.word-break-break-word", Off),
];

pub fn feature_state(id: &str) -> Option<FeatureState> {
    FEATURES.iter().find(|(f, _)| *f == id).map(|(_, s)| *s)
}

/// The excluded-value feature of a declaration (`name: value`) that the engine rejects at parse
/// time because its pref is off (section 4.3): `None` when the value is allowed.
pub fn value_feature(name: &str, value: &str) -> Option<FeatureId> {
    let n = name.trim().to_ascii_lowercase();
    let v = value.trim().trim_end_matches(';').to_ascii_lowercase();
    let has_token = |t: &str| {
        v.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
            .any(|x| x == t)
    };
    let id = if matches!(n.as_str(), "clip-path" | "shape-outside" | "offset-path")
        && has_function(&v, "shape")
    {
        "css.value.shape-function"
    } else if matches!(n.as_str(), "clip-path" | "filter" | "backdrop-filter") && v.contains("url(")
    {
        if n == "filter" || n == "backdrop-filter" {
            "css.value.filter-url"
        } else {
            "css.value.clip-path-url"
        }
    } else if v.contains("cross-fade(") {
        "css.function.cross-fade"
    } else if v
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .any(|w| w == "random")
        && v.contains("random(")
    {
        "css.function.random"
    } else {
        match n.as_str() {
            "background-clip" if has_token("border-area") => {
                "css.value.background-clip-border-area"
            }
            "font-variant-caps" if has_token("all-small-caps") || has_token("all-petite-caps") => {
                "css.value.font-variant-caps-all"
            }
            "grid-template-columns" | "grid-template-rows" | "grid-template"
                if has_token("subgrid") =>
            {
                "css.value.subgrid"
            }
            "text-indent" if has_token("each-line") => "css.value.text-indent-each-line",
            "text-indent" if has_token("hanging") => "css.value.text-indent-hanging",
            "text-wrap" | "text-wrap-style" if has_token("pretty") => "css.value.text-wrap-pretty",
            "white-space-collapse" if has_token("preserve-spaces") => {
                "css.value.white-space-collapse-preserve-spaces"
            }
            "word-break" if has_token("break-word") => "css.value.word-break-break-word",
            _ => return None,
        }
    };
    FeatureId::new(id)
}

/// Whether `v` (lower-case) calls the CSS function `name(` as a whole word (`shape(` but not `basic-shape(`).
fn has_function(v: &str, name: &str) -> bool {
    v.match_indices(&format!("{name}(")).any(|(i, _)| {
        i == 0 || !(v.as_bytes()[i - 1].is_ascii_alphanumeric() || v.as_bytes()[i - 1] == b'-')
    })
}

/// Exact property name → feature id for parse-time drops (§4.3, §5).
const PROPERTY_FEATURES: &[(&str, &str)] = &[
    (
        "-webkit-text-fill-color",
        "css.property.-webkit-text-fill-color",
    ),
    ("animation-range", "css.property.animation-timeline"),
    ("animation-range-end", "css.property.animation-timeline"),
    ("animation-range-start", "css.property.animation-timeline"),
    ("animation-timeline", "css.property.animation-timeline"),
    ("counter-set", "css.property.counter-set"),
    ("font-size-adjust", "css.property.font-size-adjust"),
    ("font-variant-emoji", "css.property.font-variant-emoji"),
    ("image-orientation", "css.property.image-orientation"),
    ("position-area", "css.property.position-area"),
    ("position-try", "css.property.position-try"),
    ("position-try-fallbacks", "css.property.position-try"),
    ("position-try-order", "css.property.position-try"),
    ("print-color-adjust", "css.property.print-color-adjust"),
    ("initial-letter", "css.property.initial-letter"),
    ("shape-outside", "css.property.shape-outside"),
    (
        "text-decoration-skip-ink",
        "css.property.text-decoration-skip-ink",
    ),
    ("zoom", "css.property.zoom"),
];

/// Property-name prefixes → family feature id.
const PROPERTY_PREFIX_FEATURES: &[(&str, &str)] = &[
    ("-webkit-text-stroke", "css.property.-webkit-text-stroke"),
    ("corner-", "css.property.corner-shape"),
    ("view-transition-", "css.property.view-transition"),
];

/// Properties the engine parses and computes whose rows are Off: a declaration is **kept** and reported by name,
/// as written (the sheet pre-scan and the `style=""` scan; a shorthand reports its own row), never removed by the
/// census (1.7.0). Empty since 1.9.0: the multicolumn and break rows of 1.7.0 are On (015 MC-1, FRAG-2c).
pub const REPORTED_PROPERTIES: &[(&str, &str)] = &[];

/// The row of a property in [`REPORTED_PROPERTIES`] (kept and reported), if any.
pub fn reported_property_feature(name: &str) -> Option<FeatureId> {
    let n = name.trim().to_ascii_lowercase();
    REPORTED_PROPERTIES
        .iter()
        .find(|(p, _)| *p == n)
        .and_then(|(_, f)| FeatureId::new(f))
}

/// The feature id of a property the profile excludes, if any (the kept-and-reported rows of
/// [`REPORTED_PROPERTIES`] included: `@supports` and CSSOM writes treat them as excluded).
pub fn property_feature(name: &str) -> Option<FeatureId> {
    let n = name.trim().to_ascii_lowercase();
    let id = PROPERTY_FEATURES
        .iter()
        .chain(REPORTED_PROPERTIES)
        .find(|(p, _)| *p == n)
        .or_else(|| {
            PROPERTY_PREFIX_FEATURES
                .iter()
                .find(|(p, _)| n.starts_with(p))
        })
        .map(|(_, f)| *f)?;
    FeatureId::new(id)
}

/// Pseudo-class / pseudo-element names the profile excludes (status U or G-not-PASS in
/// `selector-support.md`; queries fail `PROFILE_EXCLUDED` on them).
const SELECTOR_FEATURES: &[(&str, &str)] = &[
    (":host-context(", "css.selector.host"),
    (":host", "css.selector.host"),
    ("::part(", "css.pseudo.part"),
    ("::slotted(", "css.pseudo.slotted"),
];

/// The feature id of an excluded selector construct appearing in `text`, if any.
pub fn selector_feature(text: &str) -> Option<FeatureId> {
    let t = text.to_ascii_lowercase();
    let on = |f: &str| feature_state(f) == Some(FeatureState::On);
    if let Some(i) = t.find(":nth-child(").or_else(|| t.find(":nth-last-child("))
        && !on("css.selector.nth-child-of")
        && t[i..]
            .split(')')
            .next()
            .is_some_and(|args| args.split_ascii_whitespace().any(|w| w == "of"))
    {
        return FeatureId::new("css.selector.nth-child-of");
    }
    SELECTOR_FEATURES
        .iter()
        .find(|(p, f)| t.contains(p) && !on(f))
        .and_then(|(_, f)| FeatureId::new(f))
}

/// At-rules the profile excludes.
pub fn at_rule_feature(name: &str) -> Option<FeatureId> {
    let id = match name.trim_start_matches('@').to_ascii_lowercase().as_str() {
        "font-feature-values" => "css.at-rule.font-feature-values",
        "scope" => "css.at-rule.scope",
        "starting-style" => "css.at-rule.starting-style",
        "custom-media" => "css.at-rule.custom-media",
        "counter-style" => "css.at-rule.counter-style",
        "page" => "css.at-rule.page",
        "view-transition" => "css.at-rule.view-transition",
        "font-palette-values" => "css.at-rule.font-palette-values",
        _ => return None,
    };
    if feature_state(id) == Some(FeatureState::On) {
        return None;
    }
    FeatureId::new(id)
}

/// How a non-On feature is realised (css-feature-profile.md section 1 "Mechanism"): `pref`,
/// `classifier` (parse-time drop reported via the known-unsupported table), `filter` (section 6)
/// or `patch`. `None` means the row has no mechanism, which the T0 table test rejects.
pub fn mechanism(id: &str) -> Option<&'static str> {
    if let Some((_, m)) = MECHANISMS.iter().find(|(f, _)| *f == id) {
        return Some(m);
    }
    let classified = PROPERTY_FEATURES
        .iter()
        .chain(PROPERTY_PREFIX_FEATURES)
        .chain(SELECTOR_FEATURES)
        .any(|(_, f)| *f == id);
    if classified {
        return Some("classifier");
    }
    let at = id
        .strip_prefix("css.at-rule.")
        .is_some_and(|n| at_rule_feature(n).is_some_and(|f| f.as_str() == id));
    let display = id
        .strip_prefix("css.display.")
        .is_some_and(|_| DISPLAY_CLASSIFIED.contains(&id));
    if at || display {
        Some("classifier")
    } else {
        None
    }
}

/// `css.display.*` ids the diagnostics classifier produces (diagnostics.rs `display_feature`).
const DISPLAY_CLASSIFIED: &[&str] = &[
    "css.display.ruby",
    "css.display.ruby-base",
    "css.display.ruby-text",
    "css.display.ruby-base-container",
    "css.display.ruby-text-container",
    "css.display.grid-lanes",
    "css.display.inline-grid-lanes",
];

/// T0 table consistency (§8): every id is registered under 005's roots and appears once; every
/// classifier id is in the feature list with a non-On state; no pref is set twice; after
/// apply, every pref reads back as set. (Every pref of `stylo_static_prefs/preferences.toml`
/// @37c1ed3 is either in `BOOL_PREFS`/`INT_PREFS` or in [`LEFT_AT_DEFAULT`]; a pref name the
/// engine does not define fails to compile in `set_pref!`.)
pub fn check_consistency() -> Vec<String> {
    let mut problems = vec![];
    let mut seen = std::collections::HashSet::new();
    for (id, _) in FEATURES {
        if FeatureId::new(id).is_none() {
            problems.push(format!("{id}: not under a registered feature root"));
        }
        if !seen.insert(*id) {
            problems.push(format!("{id}: listed twice"));
        }
    }
    let classified = PROPERTY_FEATURES
        .iter()
        .chain(PROPERTY_PREFIX_FEATURES)
        .chain(SELECTOR_FEATURES)
        .chain(REPORTED_PROPERTIES)
        .map(|(_, f)| *f);
    for f in classified {
        match feature_state(f) {
            None => problems.push(format!("{f}: classified but not in FEATURES")),
            Some(On) => problems.push(format!("{f}: classified as excluded but On")),
            _ => {}
        }
    }
    let mut names = std::collections::HashSet::new();
    for (n, _) in BOOL_PREFS {
        if !names.insert(*n) {
            problems.push(format!("pref {n}: set twice"));
        }
    }
    for (n, v) in read_bool_prefs() {
        let want = BOOL_PREFS.iter().find(|(p, _)| *p == n).map(|(_, v)| *v);
        if crate::style::profile::is_applied() && Some(v) != want {
            problems.push(format!("pref {n}: reads {v}, profile sets {want:?}"));
        }
    }
    for (n, v) in read_int_prefs() {
        let want = INT_PREFS.iter().find(|(p, _)| *p == n).map(|(_, v)| *v);
        if crate::style::profile::is_applied() && Some(v) != want {
            problems.push(format!("pref {n}: reads {v}, profile sets {want:?}"));
        }
    }
    problems
}

/// Rows whose mechanism is not a parse-time classifier entry: filter-pass removals (section 6),
/// carried patches (gate X-STYLE-PATCH) and rows realised only by a pref value.
const MECHANISMS: &[(&str, &str)] = &[
    // pref rows (section 4)
    (
        "css.at-rule.scope-implicit",
        "census (an implicit-scope @scope without <scope-start> is removed and reported; no owner node in document sheets)",
    ),
    (
        "css.value.text-wrap-pretty",
        "pref layout.css.text-wrap-style.pretty.enabled",
    ),
    (
        "css.value.shape-function",
        "pref layout.css.basic-shape-shape.enabled=false (CSS Shapes 2 shape(): clip-path, shape-outside; 1.3.0)",
    ),
    (
        "css.value.text-indent-hanging",
        "pref layout.css.text-indent-keywords.enabled",
    ),
    (
        "css.value.text-indent-each-line",
        "pref layout.css.text-indent-keywords.enabled",
    ),
    (
        "css.value.word-break-break-word",
        "pref layout.css.word-break.break-word.enabled",
    ),
    (
        "css.function.container-style-query",
        "pref layout.css.style-queries.enabled=false (`@container style(...)`: the rule stays and is reported; 1.6.0)",
    ),
    (
        "css.unit.lh-normal",
        "report (FR-035 as amended, E-018: lh/rlh under line-height: normal; the pinned Stylo resolves it to 0)",
    ),
    // engine-default rows (section 4: the engine parses them; the pref list names no switch)
    ("css.function.cross-fade", "default"),
    (
        "css.function.light-dark-image",
        "pref layout.css.light-dark.images.enabled (On since 1.26.0: F0909)",
    ),
    ("css.function.random", "default"),
    ("css.value.background-clip-border-area", "default"),
    (
        "css.at-rule.page-vertical",
        "report (015 PAGE-1: a vertical root element's pages are laid out in the horizontal page model)",
    ),
    (
        "css.at-rule.page-width-change",
        "report (015 PAGE-1: no line-level resumption across pages of different content widths)",
    ),
    (
        "css.value.subgrid",
        "pref layout.css.grid-template-subgrid-value.enabled (On since 1.11.0: CSS Grid 2 subgrid, Taffy patch P-008-4, 015 GRID-1)",
    ),
    ("css.value.white-space-collapse-preserve-spaces", "default"),
    (
        "css.at-rule.font-feature-values",
        "default (rule ignored; reported once per sheet)",
    ),
    // filter rows (section 6; profile/filter.rs, T045)
    (
        "css.property.backdrop-filter",
        "pref layout.css.backdrop-filter.enabled (On since 1.26.0: F0909; URL filter functions are excluded and reported as css.value.filter-url)",
    ),
    ("css.property.baseline-source", "filter"),
    ("css.value.clip-path-url", "filter"),
    ("css.value.filter-url", "filter"),
    // report-once rows (rules that can never match; section 3)
    ("css.pseudo.details-content", "report-once"),
    ("css.pseudo.file-selector-button", "report-once"),
    ("css.pseudo.color-swatch", "report-once"),
    // carried patch (gate X-STYLE-PATCH = PASS; adopted in profile 1.1.0)
    (
        "css.media.prefers-reduced-motion",
        "patch P-006-1 (gate X-STYLE-PATCH PASS)",
    ),
];
