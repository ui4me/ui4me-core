//! owner: 006-style-engine (T029) — the effective `@font-face` rules of a document, exported for
//! 007 (`ui4me-resources::sync_font_faces`) and 009 (the per-document face view).
//!
//! The rules come from the stylist after a flush: Stylo has already applied `@media`, `@supports`
//! and `@layer` and ordered them by origin, then layer order, then source order, so the list is in
//! cascade order (a later rule wins over an earlier one with the same descriptors). Each `url()` is
//! already resolved against the URL of the sheet that declared the rule (CSS Fonts 4 §4.3, "relative
//! to the stylesheet"), so no per-sheet base travels with the export. Descriptors are exported
//! unchanged: a single weight / width / style is a degenerate range, an absent `unicode-range` is
//! the empty list (meaning U+0-10FFFF). A rule without a `font-family` or a `src` descriptor is not
//! a valid face and is skipped (the parser records it).
//!
//! Descriptors the engine does not honour are named in [`FontFaceExport::unsupported`] so that the
//! pipeline reports them (`UNSUPPORTED_FEATURE`), never silently ignoring them.

use crate::style::traverse::StyleBridge;
use style::font_face::{
    FontFaceSourceFormat, FontFaceSourceFormatKeyword, FontFaceSourceTechFlags, FontStyleRange,
    Source,
};
use url::Url;

/// One `src` entry.
#[derive(Clone, Debug, PartialEq)]
pub enum FontSourceExport {
    /// `url(...) [format(...)] [tech(...)]`; `url` is absolute, `None` if the reference did not
    /// resolve (an invalid URL fails that source only).
    Url {
        url: Option<Url>,
        reference: String,
        format: Option<String>,
        tech: Vec<&'static str>,
    },
    // `tech` is empty under the pinned profile: `layout.css.font-tech.enabled=false` makes the parser
    // drop a `src` entry that carries `tech()` (css-feature-profile.md), so none reaches the export.
    /// `local(<family>)`.
    Local(String),
}

/// A `font-style` descriptor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontStyleExport {
    Normal,
    Italic,
    Oblique { min_deg: f32, max_deg: f32 },
}

/// One effective `@font-face` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaceExport {
    pub family: String,
    pub sources: Vec<FontSourceExport>,
    /// `font-weight` range (a single value is a degenerate range); default 400.
    pub weight: (f32, f32),
    pub style: FontStyleExport,
    /// `font-width` / `font-stretch` range in percent; default 100.
    pub width: (f32, f32),
    /// Code-point ranges; empty = absent = U+0-10FFFF.
    pub unicode_range: Vec<(u32, u32)>,
    /// The `font-display` keyword, if written.
    pub display: Option<&'static str>,
    /// Descriptors the engine parses but does not honour (stable names).
    pub unsupported: Vec<&'static str>,
    /// Source position of the rule in its sheet (1-based line, column) for reports.
    pub line: u32,
    pub column: u32,
}

fn format_name(f: &FontFaceSourceFormat) -> String {
    match f {
        FontFaceSourceFormat::Keyword(k) => match k {
            FontFaceSourceFormatKeyword::Collection => "collection",
            FontFaceSourceFormatKeyword::EmbeddedOpentype => "embedded-opentype",
            FontFaceSourceFormatKeyword::Opentype => "opentype",
            FontFaceSourceFormatKeyword::Svg => "svg",
            FontFaceSourceFormatKeyword::Truetype => "truetype",
            FontFaceSourceFormatKeyword::Woff => "woff",
            FontFaceSourceFormatKeyword::Woff2 => "woff2",
            FontFaceSourceFormatKeyword::None | FontFaceSourceFormatKeyword::Unknown => "",
        }
        .to_string(),
        FontFaceSourceFormat::String(s) => s.trim().to_ascii_lowercase(),
    }
}

fn tech_names(t: FontFaceSourceTechFlags) -> Vec<&'static str> {
    const ALL: [(FontFaceSourceTechFlags, &str); 11] = [
        (
            FontFaceSourceTechFlags::FEATURES_OPENTYPE,
            "features-opentype",
        ),
        (FontFaceSourceTechFlags::FEATURES_AAT, "features-aat"),
        (
            FontFaceSourceTechFlags::FEATURES_GRAPHITE,
            "features-graphite",
        ),
        (FontFaceSourceTechFlags::COLOR_COLRV0, "color-colrv0"),
        (FontFaceSourceTechFlags::COLOR_COLRV1, "color-colrv1"),
        (FontFaceSourceTechFlags::COLOR_SVG, "color-svg"),
        (FontFaceSourceTechFlags::COLOR_SBIX, "color-sbix"),
        (FontFaceSourceTechFlags::COLOR_CBDT, "color-cbdt"),
        (FontFaceSourceTechFlags::VARIATIONS, "variations"),
        (FontFaceSourceTechFlags::PALETTES, "palettes"),
        (FontFaceSourceTechFlags::INCREMENTAL, "incremental"),
    ];
    ALL.iter()
        .filter(|(f, _)| t.contains(*f))
        .map(|(_, n)| *n)
        .collect()
}

impl StyleBridge {
    /// The effective `@font-face` rules in cascade order (UA, then author; within an origin by
    /// layer then source order). Valid after a [`StyleBridge::restyle`] (the stylist is flushed
    /// there). Cheap when the document declares none.
    pub fn font_face_rules(&self) -> Vec<FontFaceExport> {
        let guard = self.sidecars.lock.read();
        let mut out = Vec::new();
        for (data, _origin) in self.stylist.iter_extra_data_origins() {
            for (rule, _layer) in data.font_faces.iter() {
                let rule = rule.read_with(&guard);
                let d = &rule.descriptors;
                let (Some(family), Some(src)) = (d.font_family.as_ref(), d.src.as_ref()) else {
                    continue;
                };
                let sources: Vec<FontSourceExport> = src
                    .0
                    .iter()
                    .map(|s| match s {
                        Source::Url(u) => FontSourceExport::Url {
                            url: u.url.url().map(|a| (**a).clone()),
                            reference: u.url.as_str().to_string(),
                            format: u.format_hint.as_ref().map(format_name),
                            tech: tech_names(u.tech_flags),
                        },
                        Source::Local(name) => FontSourceExport::Local(name.name.to_string()),
                    })
                    .collect();
                let weight = d
                    .font_weight
                    .as_ref()
                    .and_then(|w| w.compute())
                    .map(|w| (w.0.value(), w.1.value()))
                    .unwrap_or((400.0, 400.0));
                let width = d
                    .font_width
                    .as_ref()
                    .and_then(|w| w.compute())
                    .map(|w| (w.0.to_percentage().0 * 100.0, w.1.to_percentage().0 * 100.0))
                    .unwrap_or((100.0, 100.0));
                let style = match d.font_style.as_ref() {
                    None => FontStyleExport::Normal,
                    Some(FontStyleRange::Italic) => FontStyleExport::Italic,
                    Some(r) => match r.compute() {
                        Some(c) => {
                            let (a, b) = (c.0.oblique_degrees(), c.1.oblique_degrees());
                            if a == 0.0 && b == 0.0 {
                                FontStyleExport::Normal
                            } else {
                                FontStyleExport::Oblique {
                                    min_deg: a,
                                    max_deg: b,
                                }
                            }
                        }
                        None => FontStyleExport::Normal,
                    },
                };
                let unicode_range = d
                    .unicode_range
                    .as_ref()
                    .map(|v| v.iter().map(|r| (r.start, r.end)).collect())
                    .unwrap_or_default();
                let display = d.font_display.as_ref().map(|f| match f {
                    style::font_face::FontDisplay::Auto => "auto",
                    style::font_face::FontDisplay::Block => "block",
                    style::font_face::FontDisplay::Swap => "swap",
                    style::font_face::FontDisplay::Fallback => "fallback",
                    style::font_face::FontDisplay::Optional => "optional",
                });
                let mut unsupported = Vec::new();
                if d.size_adjust.is_some() {
                    unsupported.push("size-adjust");
                }
                if d.ascent_override.is_some() {
                    unsupported.push("ascent-override");
                }
                if d.descent_override.is_some() {
                    unsupported.push("descent-override");
                }
                if d.line_gap_override.is_some() {
                    unsupported.push("line-gap-override");
                }
                if d.font_feature_settings.is_some() {
                    unsupported.push("font-feature-settings");
                }
                if d.font_variation_settings.is_some() {
                    unsupported.push("font-variation-settings");
                }
                if d.font_language_override.is_some() {
                    unsupported.push("font-language-override");
                }
                out.push(FontFaceExport {
                    family: family.name.to_string(),
                    sources,
                    weight,
                    style,
                    width,
                    unicode_range,
                    display,
                    unsupported,
                    line: rule.source_location.line,
                    column: rule.source_location.column,
                });
            }
        }
        out
    }
}
