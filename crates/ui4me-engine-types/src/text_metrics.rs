//! owner: 001-vertical-slice (file); trait 009-text-inline (AD-P-009-b) — created by 001 slice (R-28)
//!
//! `FontMetricsSource` (AD-08; 009 `contracts/text-services.md` §4; 009 research R-15).
//! Declared here so that `ui4me-style` reaches 009's metrics service without a compile-time
//! dependency on `ui4me-text`. Implemented by `ui4me_text::metrics::MetricsService`.
//!
//! No upstream (Parley, Fontique, Stylo) type appears here (AD-33). All lengths are CSS px.

/// CSS generic family names (007 FR-050; 009 `font-matching.md` §1). Any other name is an
/// ordinary family name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
    UiSerif,
    UiSansSerif,
    UiMonospace,
    UiRounded,
    Emoji,
    Math,
}

impl GenericFamily {
    pub const ALL: &'static [GenericFamily] = &[
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
        GenericFamily::UiSerif,
        GenericFamily::UiSansSerif,
        GenericFamily::UiMonospace,
        GenericFamily::UiRounded,
        GenericFamily::Emoji,
        GenericFamily::Math,
    ];

    /// The CSS keyword.
    pub const fn css_name(self) -> &'static str {
        match self {
            GenericFamily::Serif => "serif",
            GenericFamily::SansSerif => "sans-serif",
            GenericFamily::Monospace => "monospace",
            GenericFamily::Cursive => "cursive",
            GenericFamily::Fantasy => "fantasy",
            GenericFamily::SystemUi => "system-ui",
            GenericFamily::UiSerif => "ui-serif",
            GenericFamily::UiSansSerif => "ui-sans-serif",
            GenericFamily::UiMonospace => "ui-monospace",
            GenericFamily::UiRounded => "ui-rounded",
            GenericFamily::Emoji => "emoji",
            GenericFamily::Math => "math",
        }
    }

    /// The generic for an unquoted CSS keyword (ASCII case-insensitive), else `None`.
    pub fn from_css_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|g| g.css_name().eq_ignore_ascii_case(name))
    }
}

/// One entry of a computed `font-family` list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FamilyName {
    /// A family name (quoted or a sequence of identifiers); matched case-insensitively.
    Named(String),
    Generic(GenericFamily),
}

/// Computed `font-style`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontStyleSpec {
    Normal,
    Italic,
    /// Oblique angle in degrees.
    Oblique(f32),
}

/// The computed font a metrics query is about (009 `text-services.md` §4 `FontSpec`).
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    /// Shared: a computed style's family list is cloned with every box style (L-PERF-b2).
    pub families: std::sync::Arc<[FamilyName]>,
    /// CSS weight, 1–1000.
    pub weight: f32,
    pub style: FontStyleSpec,
    /// `font-stretch` as a percentage (100 = normal).
    pub stretch: f32,
    /// BCP 47 content language; empty = unknown (spec Clarification Q2).
    pub lang: String,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec {
            families: std::sync::Arc::from([]),
            weight: 400.0,
            style: FontStyleSpec::Normal,
            stretch: 100.0,
            lang: String::new(),
        }
    }
}

/// Which metrics came from the font rather than the CSS Values 4 / FR-073 fallbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetricsProvenance {
    pub ascent: bool,
    pub descent: bool,
    pub line_gap: bool,
    pub x_height: bool,
    pub cap_height: bool,
    pub zero_advance: bool,
    pub ic_advance: bool,
    pub avg_char_advance: bool,
}

/// Font metrics at a given size, in CSS px, with the FR-073 fallbacks already applied:
/// ascent 0.8 em, descent 0.2 em, line gap 0, `ex` 0.5 em, `ch` 0.5 em, `ic` 1 em,
/// `cap` = ascent, average advance = `ch`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricsRecord {
    /// Distance from the baseline to the top of the alignment box (positive).
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the alignment box (positive).
    pub descent: f32,
    pub line_gap: f32,
    pub x_height: f32,
    pub cap_height: f32,
    /// Advance of "0" (`ch`).
    pub zero_advance: f32,
    /// Advance of "水" (`ic`).
    pub ic_advance: f32,
    /// OS/2 `xAvgCharWidth` scaled to the size, else the "0" advance.
    pub avg_char_advance: f32,
    /// `head.xMax - head.xMin` scaled to the size (the font's widest glyph box; Chromium's
    /// `MaxCharWidth`, which sizes text controls). 0 when the font has no `head` table.
    pub max_char_width: f32,
    pub provenance: MetricsProvenance,
}

impl MetricsRecord {
    /// The record used when no font is reachable (all fallbacks).
    pub fn fallback(size: f32) -> Self {
        let ascent = 0.8 * size;
        MetricsRecord {
            ascent,
            descent: 0.2 * size,
            line_gap: 0.0,
            x_height: 0.5 * size,
            cap_height: ascent,
            zero_advance: 0.5 * size,
            ic_advance: size,
            avg_char_advance: 0.5 * size,
            max_char_width: 0.0,
            provenance: MetricsProvenance::default(),
        }
    }

    /// Used value of `line-height: normal`: ascent + descent + line gap (IF-01).
    pub fn normal_line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

/// 009's synchronous metrics service as seen by the style engine (AD-08).
///
/// Rules: never starts a style traversal, never blocks on I/O, never calls the owner-thread
/// text facade; callable from any thread. `vertical = true` returns the horizontal record (and
/// the implementation records a one-time `UNSUPPORTED_FEATURE`; AD-26, FR-077).
///
/// ```
/// use ui4me_engine_types::text_metrics::{FontMetricsSource, FontSpec, GenericFamily, MetricsRecord};
///
/// #[derive(Debug)]
/// struct Fixed;
/// impl FontMetricsSource for Fixed {
///     fn query(&self, _: &FontSpec, size: f32, _vertical: bool) -> MetricsRecord {
///         MetricsRecord::fallback(size)
///     }
///     fn font_epoch(&self) -> u64 { 0 }
/// }
/// fn needs_send_sync<T: Send + Sync>(_: &T) {}
/// let f = Fixed;
/// needs_send_sync(&f);
/// assert_eq!(f.base_size_for_generic(Some(GenericFamily::Monospace)), 13.0);
/// assert_eq!(f.base_size_for_generic(None), 16.0);
/// assert_eq!(f.query(&FontSpec::default(), 20.0, false).ascent, 16.0);
/// ```
pub trait FontMetricsSource: Send + Sync + std::fmt::Debug {
    /// Metrics of the first available font of `spec` (the first face covering U+0020) at `size` px.
    fn query(&self, spec: &FontSpec, size: f32, vertical: bool) -> MetricsRecord;

    /// Initial font size for a generic family: 16 px, monospace 13 px (FR-075).
    fn base_size_for_generic(&self, generic: Option<GenericFamily>) -> f32 {
        match generic {
            Some(GenericFamily::Monospace) | Some(GenericFamily::UiMonospace) => 13.0,
            _ => 16.0,
        }
    }

    /// The runtime-wide font epoch the answers are valid for (memo key; 007 FR-053).
    fn font_epoch(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallbacks_follow_fr073() {
        let r = MetricsRecord::fallback(20.0);
        assert_eq!((r.ascent, r.descent, r.line_gap), (16.0, 4.0, 0.0));
        assert_eq!(
            (r.x_height, r.zero_advance, r.ic_advance, r.cap_height),
            (10.0, 10.0, 20.0, 16.0)
        );
        assert_eq!(r.normal_line_height(), 20.0);
    }

    #[test]
    fn generic_names_round_trip() {
        for g in GenericFamily::ALL {
            assert_eq!(GenericFamily::from_css_name(g.css_name()), Some(*g));
        }
        assert_eq!(
            GenericFamily::from_css_name("SANS-SERIF"),
            Some(GenericFamily::SansSerif)
        );
        assert_eq!(GenericFamily::from_css_name("fangsong"), None);
    }
}
