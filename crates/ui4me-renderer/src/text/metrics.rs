//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! Font-metrics service (AD-08; 009 `contracts/text-services.md` §4; 009 research R-15;
//! 001 research R-18). Implements `ui4me_engine_types::text_metrics::FontMetricsSource` so that
//! the style stage reaches it without depending on the shaping implementation. `Send + Sync`: a `Mutex`-guarded
//! clone of the font collection with its own source cache and layout context, plus a memo keyed
//! by the typed font specification and exact size for the current font epoch. Never blocks on I/O, never starts a
//! traversal, never calls the owner-thread facade.

use parley::{FontFamily, FontFamilyName, LayoutContext, PositionedLayoutItem, StyleProperty};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::{
    FamilyName, FontMetricsSource, FontSpec, FontStyleSpec, MetricsProvenance, MetricsRecord,
};

use crate::text::fonts::FontEnvironment;
use crate::text::paragraph::source::SpanBrush;

pub(crate) const METRICS_CACHE_LIMIT: usize = 4096;
/// Typed key: changing Debug formatting must never change text-cache identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FontQueryKey {
    families: std::sync::Arc<[FamilyName]>,
    weight: u32,
    style: (u8, u32),
    stretch: u32,
    lang: String,
    size: u32,
}
impl FontQueryKey {
    pub(crate) fn new(spec: &FontSpec, size: f32) -> Self {
        let style = match spec.style {
            FontStyleSpec::Normal => (0, 0),
            FontStyleSpec::Italic => (1, 0),
            FontStyleSpec::Oblique(angle) => (2, angle.to_bits()),
        };
        Self {
            families: spec.families.clone(),
            weight: spec.weight.to_bits(),
            style,
            stretch: spec.stretch.to_bits(),
            lang: spec.lang.clone(),
            size: size.to_bits(),
        }
    }
}
struct Inner {
    env: FontEnvironment,
    lcx: LayoutContext<SpanBrush>,
    memo: HashMap<FontQueryKey, MetricsRecord>,
    vertical_reported: bool,
    diagnostics: Vec<Status>,
}

pub struct MetricsService {
    epoch: AtomicU64,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for MetricsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsService")
            .field("epoch", &self.epoch.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Parley properties for a computed font (shared with paragraph shaping).
pub(crate) fn font_properties<'a>(
    env: &FontEnvironment,
    spec: &FontSpec,
    size: f32,
) -> Vec<StyleProperty<'a, SpanBrush>> {
    env.note_face_use(spec, None);
    let names: Vec<FontFamilyName<'a>> = env
        .resolve_families(&spec.families)
        .into_iter()
        .map(|n| FontFamilyName::Named(Cow::Owned(n)))
        .collect();
    let style = match spec.style {
        FontStyleSpec::Normal => parley::FontStyle::Normal,
        FontStyleSpec::Italic => parley::FontStyle::Italic,
        FontStyleSpec::Oblique(a) => parley::FontStyle::Oblique(Some(a)),
    };
    vec![
        StyleProperty::FontFamily(FontFamily::List(Cow::Owned(names))),
        StyleProperty::FontSize(size),
        StyleProperty::FontWeight(parley::FontWeight::new(spec.weight)),
        StyleProperty::FontStyle(style),
        StyleProperty::FontWidth(parley::FontWidth::from_percentage(spec.stretch)),
    ]
}

/// Big-endian sfnt table lookup (face `index` inside a collection). Returns the table bytes.
pub(crate) fn sfnt_table<'a>(data: &'a [u8], index: u32, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let be32 = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let be16 = |o: usize| {
        data.get(o..o + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let base = if data.get(..4) == Some(b"ttcf") {
        be32(12 + 4 * index as usize)?
    } else {
        0
    };
    let num = be16(base + 4)?;
    for i in 0..num {
        let rec = base + 12 + 16 * i;
        if data.get(rec..rec + 4)? == tag {
            let (off, len) = (be32(rec + 8)?, be32(rec + 12)?);
            return data.get(off..off.checked_add(len)?);
        }
    }
    None
}

/// OS/2 `xAvgCharWidth` / `head.unitsPerEm`, scaled to `size` (0 or absent = `None`).
fn avg_char_width(data: &[u8], index: u32, size: f32) -> Option<f32> {
    let os2 = sfnt_table(data, index, b"OS/2")?;
    let head = sfnt_table(data, index, b"head")?;
    let avg = i16::from_be_bytes([*os2.get(2)?, *os2.get(3)?]);
    let upem = u16::from_be_bytes([*head.get(18)?, *head.get(19)?]);
    (avg > 0 && upem > 0).then(|| avg as f32 * size / upem as f32)
}

/// `head.xMax - head.xMin` / `head.unitsPerEm`, scaled to `size` (absent = `None`).
fn max_char_width(data: &[u8], index: u32, size: f32) -> Option<f32> {
    let head = sfnt_table(data, index, b"head")?;
    let upem = u16::from_be_bytes([*head.get(18)?, *head.get(19)?]);
    let x_min = i16::from_be_bytes([*head.get(36)?, *head.get(37)?]);
    let x_max = i16::from_be_bytes([*head.get(40)?, *head.get(41)?]);
    (upem > 0 && x_max > x_min).then(|| (x_max as f32 - x_min as f32) * size / upem as f32)
}

struct Probe {
    advance: f32,
    has_glyph: bool,
    metrics: parley::FontMetrics,
    font_data: parley::fontique::Blob<u8>,
    font_index: u32,
}

impl Inner {
    /// Shape `text` with the spec and report its first run.
    fn probe(
        &mut self,
        spec: &FontSpec,
        size: f32,
        text: &str,
    ) -> (Option<Probe>, Option<parley::SpanMetrics>) {
        self.probe_with_defaults(spec, size, text, None)
    }

    fn probe_with_defaults(
        &mut self,
        spec: &FontSpec,
        size: f32,
        text: &str,
        defaults: Option<ui4me_assets::fonts::FaceSettings>,
    ) -> (Option<Probe>, Option<parley::SpanMetrics>) {
        let mut props = font_properties(&self.env, spec, size);
        if let Some(defaults) = &defaults {
            let settings = crate::text::fonts::features::FontSettings::default()
                .with_face_defaults(defaults, 0.0);
            props.extend(crate::text::fonts::features::style_properties(
                &settings,
                size,
                self.env.opsz_available(),
                None,
            ));
        }
        let mut b = self.lcx.ranged_builder(&mut self.env.fcx, text, 1.0, false);
        for p in props {
            b.push_default(p);
        }
        let mut layout = b.build(text);
        let span = layout.root_span_metrics();
        layout.break_all_lines(None);
        let mut out = None;
        if let Some(line) = layout.lines().next() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(gr) = item {
                    let run = gr.run();
                    out = Some(Probe {
                        advance: gr.advance(),
                        has_glyph: gr.glyphs().all(|g| g.id != 0),
                        metrics: *run.font_metrics(),
                        font_data: run.font().data.clone(),
                        font_index: run.font().index,
                    });
                    break;
                }
            }
        }
        if defaults.is_none()
            && self.env.face_table().has_settings()
            && let Some(first) = &out
        {
            let font = parley::FontData {
                data: first.font_data.clone(),
                index: first.font_index,
            };
            if let Some(settings) = self
                .env
                .settings_for_shaped_font(spec, text.chars().next().unwrap_or(' '), &font)
                .filter(|d| !d.is_empty())
            {
                return self.probe_with_defaults(spec, size, text, Some(settings));
            }
        }
        (out, span)
    }

    fn compute(&mut self, spec: &FontSpec, size: f32) -> MetricsRecord {
        let mut rec = MetricsRecord::fallback(size);
        let mut prov = MetricsProvenance::default();
        // First available font: the first face covering U+0020 (CSS Fonts 4).
        let (space, span) = self.probe(spec, size, " ");
        if let (Some(space), Some(span)) = (space.filter(|p| p.has_glyph), span) {
            rec.ascent = span.ascent;
            rec.descent = span.descent;
            rec.line_gap = space.metrics.leading;
            prov.ascent = true;
            prov.descent = true;
            prov.line_gap = true;
            if let Some(x) = space.metrics.x_height.filter(|v| *v > 0.0) {
                rec.x_height = x;
                prov.x_height = true;
            }
            match space.metrics.cap_height.filter(|v| *v > 0.0) {
                Some(c) => {
                    rec.cap_height = c;
                    prov.cap_height = true;
                }
                None => rec.cap_height = rec.ascent,
            }
            if let Some(zero) = self.probe(spec, size, "0").0.filter(|p| p.has_glyph) {
                rec.zero_advance = zero.advance;
                prov.zero_advance = true;
            }
            if let Some(ic) = self.probe(spec, size, "\u{6C34}").0.filter(|p| p.has_glyph) {
                rec.ic_advance = ic.advance;
                prov.ic_advance = true;
            }
            // the probed run's own font bytes (a registered blob or a system font file)
            let avg = avg_char_width(space.font_data.as_ref(), space.font_index, size);
            rec.max_char_width =
                max_char_width(space.font_data.as_ref(), space.font_index, size).unwrap_or(0.0);
            match avg {
                Some(a) => {
                    rec.avg_char_advance = a;
                    prov.avg_char_advance = true;
                }
                None => rec.avg_char_advance = rec.zero_advance,
            }
        }
        rec.provenance = prov;
        rec
    }
}

/// Replace non-finite or negative fields of `rec` by their FR-073 fallbacks and clear their
/// provenance. Returns the codes to report: `GEOMETRY_NON_FINITE` for NaN or infinity,
/// `DECODE_FAILED` for negative values (undecodable face tables), each at most once.
pub fn sanitize_record(rec: MetricsRecord, size: f32) -> (MetricsRecord, Vec<Code>) {
    let fb = MetricsRecord::fallback(size);
    let mut out = rec;
    let (mut non_finite, mut negative) = (false, false);
    macro_rules! fix {
        ($f:ident) => {
            if !out.$f.is_finite() {
                out.$f = fb.$f;
                out.provenance.$f = false;
                non_finite = true;
            } else if out.$f < 0.0 {
                out.$f = fb.$f;
                out.provenance.$f = false;
                negative = true;
            }
        };
    }
    fix!(ascent);
    fix!(descent);
    fix!(line_gap);
    fix!(x_height);
    fix!(cap_height);
    fix!(zero_advance);
    fix!(ic_advance);
    fix!(avg_char_advance);
    if !out.max_char_width.is_finite() || out.max_char_width < 0.0 {
        out.max_char_width = 0.0;
    }
    let mut codes = Vec::new();
    if non_finite {
        codes.push(Code::GeometryNonFinite);
    }
    if negative {
        codes.push(Code::DecodeFailed);
    }
    (out, codes)
}

impl MetricsService {
    pub fn new(env: FontEnvironment) -> Self {
        MetricsService {
            epoch: AtomicU64::new(env.font_epoch()),
            inner: Mutex::new(Inner {
                env,
                lcx: LayoutContext::new(),
                memo: HashMap::new(),
                vertical_reported: false,
                diagnostics: Vec::new(),
            }),
        }
    }

    /// New font epoch: swap the collection and clear the memo (`apply_font_epoch`).
    pub fn apply_environment(&self, env: FontEnvironment) {
        if let Ok(mut inner) = self.inner.lock() {
            self.epoch.store(env.font_epoch(), Ordering::Release);
            inner.env = env;
            inner.memo.clear();
        }
    }

    /// Stage diagnostics produced by queries since the last call (vertical `UNSUPPORTED_FEATURE`
    /// once per service, `GEOMETRY_NON_FINITE`, `DECODE_FAILED`); the caller that owns the
    /// report store records them. Never blocks on anything but the service lock.
    pub fn take_diagnostics(&self) -> Vec<Status> {
        self.inner
            .lock()
            .map(|mut i| std::mem::take(&mut i.diagnostics))
            .unwrap_or_default()
    }

    /// Number of memoised records (tests).
    pub fn memo_len(&self) -> usize {
        self.inner.lock().map(|i| i.memo.len()).unwrap_or(0)
    }

    /// Whether a vertical query was made (the one-time `UNSUPPORTED_FEATURE`, FR-077, is
    /// reported by the caller that owns the report store).
    pub fn vertical_requested(&self) -> bool {
        self.inner
            .lock()
            .map(|i| i.vertical_reported)
            .unwrap_or(false)
    }
}

impl FontMetricsSource for MetricsService {
    fn query(&self, spec: &FontSpec, size: f32, vertical: bool) -> MetricsRecord {
        let size = if size.is_finite() && size >= 0.0 {
            size
        } else {
            0.0
        };
        let Ok(mut inner) = self.inner.lock() else {
            return MetricsRecord::fallback(size);
        };
        if vertical && !inner.vertical_reported {
            inner.vertical_reported = true;
            inner.diagnostics.push(
                Status::new(Code::UnsupportedFeature, Stage::Text)
                    .with_message("vertical writing modes are unsupported: the horizontal metrics record is returned (FR-077)"),
            );
        }
        let key = FontQueryKey::new(spec, size);
        if let Some(r) = inner.memo.get(&key) {
            return *r;
        }
        let computed = inner.compute(spec, size);
        let (r, codes) = sanitize_record(computed, size);
        for code in codes {
            let why = match code {
                Code::GeometryNonFinite => "non-finite font metric replaced by its fallback",
                _ => "negative or undecodable font metric replaced by its fallback",
            };
            inner
                .diagnostics
                .push(Status::new(code, Stage::Text).with_message(why));
        }
        // A long-running app may generate a new font size/family on every edit.
        // Keep bounded document-local storage without retaining historical keys.
        if inner.memo.len() == METRICS_CACHE_LIMIT {
            inner.memo.clear();
        }
        inner.memo.insert(key, r);
        r
    }

    fn font_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use ui4me_assets::fonts::{FontRegistry, PinnedFontConfig};
    use ui4me_engine_types::text_metrics::FamilyName;

    fn service() -> MetricsService {
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
        let reg = FontRegistry::pinned(&PinnedFontConfig::with_files([font])).unwrap();
        MetricsService::new(FontEnvironment::new(&reg.font_sources()).unwrap())
    }

    fn noto() -> FontSpec {
        FontSpec {
            families: vec![FamilyName::Named("Noto Sans".into())].into(),
            ..FontSpec::default()
        }
    }

    /// Reads hhea ascender/descender/lineGap and head.unitsPerEm straight from the file, so the
    /// expectation is derived from the font, not hardcoded.
    fn hhea(size: f32) -> (f32, f32, f32) {
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
        let data = std::fs::read(font).unwrap();
        let hhea = sfnt_table(&data, 0, b"hhea").unwrap();
        let head = sfnt_table(&data, 0, b"head").unwrap();
        let s16 = |t: &[u8], o: usize| i16::from_be_bytes([t[o], t[o + 1]]) as f32;
        let upem = u16::from_be_bytes([head[18], head[19]]) as f32;
        let k = size / upem;
        (s16(hhea, 4) * k, -s16(hhea, 6) * k, s16(hhea, 8) * k)
    }

    #[test]
    fn metrics_service_returns_registered_font_ascent_descent_line_gap() {
        let svc = service();
        let r = svc.query(&noto(), 16.0, false);
        let (a, d, g) = hhea(16.0);
        assert!((r.ascent - a).abs() < 1e-3, "ascent {} vs {}", r.ascent, a);
        assert!(
            (r.descent - d).abs() < 1e-3,
            "descent {} vs {}",
            r.descent,
            d
        );
        assert!(
            (r.line_gap - g).abs() < 1e-3,
            "line gap {} vs {}",
            r.line_gap,
            g
        );
        assert!(r.provenance.ascent && r.provenance.descent && r.provenance.line_gap);
        assert!(
            r.provenance.x_height && r.provenance.zero_advance && r.provenance.avg_char_advance
        );
        // Noto Sans has no "水": ic falls back to 1em (FR-073).
        assert!(!r.provenance.ic_advance);
        assert_eq!(r.ic_advance, 16.0);
        assert!((r.normal_line_height() - (a + d + g)).abs() < 1e-3);
    }

    #[test]
    fn metrics_scale_linearly_and_are_memoised() {
        let svc = service();
        let a = svc.query(&noto(), 16.0, false);
        let b = svc.query(&noto(), 32.0, false);
        assert!((b.ascent - 2.0 * a.ascent).abs() < 1e-3);
        assert_eq!(svc.query(&noto(), 16.0, false), a);
    }

    #[test]
    fn font_size_churn_has_bounded_retention_and_preserves_metrics() {
        let svc = service();
        let reference = svc.query(&noto(), 16.0, false);
        for i in 0..METRICS_CACHE_LIMIT + 32 {
            svc.query(&noto(), 8.0 + i as f32 / 128.0, false);
            assert!(svc.memo_len() <= METRICS_CACHE_LIMIT);
        }
        assert_eq!(svc.query(&noto(), 16.0, false), reference);
    }

    #[test]
    fn generic_and_unknown_families_reach_the_pinned_font() {
        let svc = service();
        let generic = FontSpec {
            families: vec![
                FamilyName::Named("No Such Family".into()),
                FamilyName::Generic(ui4me_engine_types::text_metrics::GenericFamily::Serif),
            ]
            .into(),
            ..FontSpec::default()
        };
        assert_eq!(
            svc.query(&generic, 16.0, false),
            svc.query(&noto(), 16.0, false)
        );
    }

    #[test]
    fn vertical_returns_the_horizontal_record() {
        let svc = service();
        assert_eq!(
            svc.query(&noto(), 16.0, true),
            svc.query(&noto(), 16.0, false)
        );
        assert!(svc.vertical_requested());
    }

    #[test]
    fn service_is_send_and_sync() {
        fn need<T: Send + Sync>() {}
        need::<MetricsService>();
        let _: &dyn FontMetricsSource = &service();
    }
}
