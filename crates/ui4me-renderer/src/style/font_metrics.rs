//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Font metrics for the style `Device` (T042; AD-08; research R-18; 006 plan §9). Stylo's
//! `FontMetricsProvider` is a thin adapter over 009's metrics service, reached only through the
//! `FontMetricsSource` trait in `ui4me-core/src/text_metrics.rs` (AD-P-009-b) and injected at
//! runtime start, so ui4me-style never depends on ui4me-text at compile time. The service is
//! `Sync` and never starts a traversal.
//!
//! A metric the font does not provide (`MetricsRecord::provenance` false) is passed to the
//! engine as "unknown", which applies the CSS Values 4 fallbacks itself (`ex` = 0.5em, `ch` =
//! 0.5em, `cap` = ascent, `ic` = 1em). Without any source, [`CssFallbackMetrics`] answers with
//! 009's documented FR-073 fallbacks. `base_size_for_generic` is the service's (16 px; 13 px for
//! monospace, 009 FR-075).

use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::properties::style_structs::Font;
use style::values::computed::font::{FontStyle, GenericFontFamily, SingleFontFamily};
use style::values::computed::{CSSPixelLength, Length};
use style::values::specified::font::QueryFontMetricsFlags;
use ui4me_engine_types::text_metrics::{
    FamilyName, FontMetricsSource, FontSpec, FontStyleSpec, GenericFamily, MetricsRecord,
};

/// No font source: 009's documented fallbacks (all metrics marked as not from a font).
#[derive(Debug, Default)]
pub struct CssFallbackMetrics;

impl FontMetricsSource for CssFallbackMetrics {
    fn query(&self, _spec: &FontSpec, size: f32, _vertical: bool) -> MetricsRecord {
        MetricsRecord::fallback(size)
    }
    fn font_epoch(&self) -> u64 {
        0
    }
}

fn generic(g: GenericFontFamily) -> Option<GenericFamily> {
    Some(match g {
        GenericFontFamily::Serif => GenericFamily::Serif,
        GenericFontFamily::SansSerif => GenericFamily::SansSerif,
        GenericFontFamily::Monospace => GenericFamily::Monospace,
        GenericFontFamily::Cursive => GenericFamily::Cursive,
        GenericFontFamily::Fantasy => GenericFamily::Fantasy,
        GenericFontFamily::SystemUi => GenericFamily::SystemUi,
        GenericFontFamily::None => return None,
    })
}

/// The neutral font specification of a computed `font` (no engine type crosses the seam).
pub(crate) fn font_spec(font: &Font) -> FontSpec {
    let families = font
        .font_family
        .families
        .iter()
        .filter_map(|f| match f {
            SingleFontFamily::FamilyName(n) => Some(FamilyName::Named(n.name.to_string())),
            SingleFontFamily::Generic(g) => generic(*g).map(FamilyName::Generic),
        })
        .collect();
    let s = font.font_style;
    let style = if s == FontStyle::NORMAL {
        FontStyleSpec::Normal
    } else if s == FontStyle::ITALIC {
        FontStyleSpec::Italic
    } else {
        FontStyleSpec::Oblique(s.oblique_degrees())
    };
    FontSpec {
        families,
        weight: font.font_weight.value(),
        style,
        stretch: font.font_width.to_percentage().0 * 100.0,
        lang: String::new(),
    }
}

/// The engine-facing adapter.
#[derive(Debug)]
pub(crate) struct MetricsAdapter(pub(crate) Box<dyn FontMetricsSource>);

impl FontMetricsProvider for MetricsAdapter {
    fn query_font_metrics(
        &self,
        vertical: bool,
        font: &Font,
        base: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        let r = self.0.query(&font_spec(font), base.px(), vertical);
        let p = r.provenance;
        let known = |ok: bool, v: f32| ok.then(|| Length::new(v));
        FontMetrics {
            x_height: known(p.x_height, r.x_height),
            zero_advance_measure: known(p.zero_advance, r.zero_advance),
            cap_height: known(p.cap_height, r.cap_height),
            ic_width: known(p.ic_advance, r.ic_advance),
            ascent: Length::new(r.ascent),
            script_percent_scale_down: None,
            script_script_percent_scale_down: None,
        }
    }

    fn base_size_for_generic(&self, g: GenericFontFamily) -> Length {
        Length::new(self.0.base_size_for_generic(generic(g)))
    }
}
