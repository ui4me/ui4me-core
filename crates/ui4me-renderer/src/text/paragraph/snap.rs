//! owner: 009-text-inline — added by M1 lane M (slice geometry vs the Chromium baseline)
//!
//! The text layer's **whole-pixel text metrics policy** (009 `text-services.md` §3, §5; decision
//! recorded in `verification/gates/M1-L/evidence/slice/lane-m/text-metrics-policy.md`).
//!
//! The reference platform (Chromium on Linux at device scale 1, the `chromium.linux-kde-wayland`
//! reference set) lays text out without subpixel glyph positioning. Measured against the pinned
//! Noto Sans (1295 glyphs, 5 sizes, kerning on and off; lane-M evidence), its glyph advances are
//!
//! ```text
//! advance(glyph) = round(round(nominal advance × 64) / 64) + round(shaped advance − nominal advance)
//! ```
//!
//! where the nominal advance is the font's `hmtx` advance scaled to the font size and the
//! difference is the shaper's positioning adjustment (GPOS kerning). Every glyph therefore
//! advances a whole number of pixels, so text widths, line-fit decisions, alignment and every
//! glyph position follow from these advances. Its vertical font metrics are likewise whole
//! pixels: ascent and descent are each rounded, `line-height: normal` is the sum of the rounded
//! ascent, descent and line gap, and the half-leading above the baseline is floored (Parley's
//! `quantize` mode implements exactly that and is enabled for paragraph shaping).
//!
//! The rule is applied in CSS px at device scale 1. Chromium enables subpixel
//! positioning at device scale > 1, where this policy does not apply. On Windows, Chromium
//! uses subpixel positioning even at device scale 1; Windows uses the unrounded shaped advance.
//! A glyph of a face at variation coordinates takes the instance's own nominal advance
//! (skrifa: `hmtx` with `HVAR`, or the `gvar` phantom points) in place of the `hmtx` one (015 TXT-2).

use parley::{Layout, PositionedLayoutItem};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use ui4me_assets::fonts::FontBlob;

use super::source::{
    InlineItem, InlineItemKind, SourceSpan, SpanBrush, SpanCollapse, TabSpec, WrapOverflow,
};

/// Big-endian sfnt table lookup (face `index` inside a collection).
fn sfnt_table<'a>(data: &'a [u8], index: u32, tag: &[u8; 4]) -> Option<&'a [u8]> {
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

/// A face's horizontal metrics: `hmtx` advance widths (font units) and `head.unitsPerEm`.
#[derive(Debug)]
pub(crate) struct Hmtx {
    upem: f32,
    advances: Vec<u16>,
}

impl Hmtx {
    fn parse(data: &[u8], index: u32) -> Option<Hmtx> {
        let head = sfnt_table(data, index, b"head")?;
        let hhea = sfnt_table(data, index, b"hhea")?;
        let hmtx = sfnt_table(data, index, b"hmtx")?;
        let upem = u16::from_be_bytes([*head.get(18)?, *head.get(19)?]);
        let n = u16::from_be_bytes([*hhea.get(34)?, *hhea.get(35)?]) as usize;
        if upem == 0 || n == 0 {
            return None;
        }
        let advances = (0..n)
            .map(|i| {
                hmtx.get(4 * i..4 * i + 2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Hmtx {
            upem: upem as f32,
            advances,
        })
    }

    /// Nominal advance of `glyph` at `size` px (glyphs past `numberOfHMetrics` repeat the last).
    fn advance(&self, glyph: u32, size: f32) -> f32 {
        let units = self
            .advances
            .get(glyph as usize)
            .or(self.advances.last())
            .copied()
            .unwrap_or(0);
        units as f32 * size / self.upem
    }
}

/// The nominal advances (font units) of the glyphs a paragraph uses from one face at one instance.
#[derive(Debug, Default)]
struct InstanceAdvances {
    upem: f32,
    units: HashMap<u32, f32>,
}

/// The policy's per-face tables, shared by a shaped paragraph and its frozen layouts.
#[derive(Debug, Default)]
pub(crate) struct AdvancePolicy {
    faces: HashMap<(u64, u32), Option<Arc<Hmtx>>>,
    /// Per (face, instance coordinates): the nominal advances of the paragraph's glyphs there.
    instances: HashMap<(u64, u32, u64), InstanceAdvances>,
    /// Subpixel glyph positioning (device scale above 1): glyphs advance by their shaped (fractional)
    /// advance, unrounded.
    subpixel: bool,
    /// Vertical metrics of the faces of a vertical paragraph with upright clusters (015 WM-1b).
    vertical: HashMap<(u64, u32), Arc<super::vertical::VerticalMetrics>>,
}

/// The font identity and size a glyph is snapped with.
#[derive(Clone, Copy)]
pub(crate) struct FaceKey {
    pub blob: u64,
    pub index: u32,
    pub size: f32,
    /// Variation coordinates in use (the nominal advance is the instance's, `instances`).
    pub varied: bool,
    /// Hash of the normalised coordinates (0 when not varied).
    pub coords: u64,
}

impl FaceKey {
    /// The key of a glyph run.
    pub(crate) fn of<B: parley::Brush>(run: &parley::layout::Run<'_, B>) -> FaceKey {
        let font = run.font();
        let coords = run.normalized_coords();
        let varied = coords.iter().any(|c| c.to_bits() != 0);
        FaceKey {
            blob: font.data.id(),
            index: font.index,
            size: run.font_size(),
            varied,
            coords: if varied {
                coords_hash(coords.iter().map(|c| c.to_bits()))
            } else {
                0
            },
        }
    }
}

fn coords_hash(coords: impl Iterator<Item = i16>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in coords {
        c.hash(&mut h);
    }
    h.finish()
}

impl AdvancePolicy {
    /// Load the tables of every face the layout uses.
    pub(crate) fn for_layout(
        layout: &Layout<SpanBrush>,
        blobs: &[FontBlob],
        subpixel: bool,
    ) -> AdvancePolicy {
        let mut p = AdvancePolicy {
            subpixel,
            ..AdvancePolicy::default()
        };
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let font = gr.run().font();
                let key = (font.data.id(), font.index);
                let published = crate::text::fonts::environment::published_blob_id(key.0);
                p.faces.entry(key).or_insert_with(|| {
                    blobs
                        .iter()
                        .find(|b| b.id.0 == published)
                        .and_then(|b| Hmtx::parse(&b.bytes, key.1))
                        .map(Arc::new)
                });
                // A face at variation coordinates: the nominal advance of each glyph at that instance.
                let face = FaceKey::of(gr.run());
                if face.varied
                    && let Some(blob) = blobs.iter().find(|b| b.id.0 == published)
                {
                    use skrifa::MetadataProvider;
                    use skrifa::raw::TableProvider;
                    if let Ok(font_ref) = skrifa::FontRef::from_index(&blob.bytes, key.1) {
                        let coords: Vec<skrifa::instance::NormalizedCoord> = gr
                            .run()
                            .normalized_coords()
                            .iter()
                            .map(|c| skrifa::instance::NormalizedCoord::from_bits(c.to_bits()))
                            .collect();
                        let entry = p
                            .instances
                            .entry((key.0, key.1, face.coords))
                            .or_insert_with(|| InstanceAdvances {
                                upem: font_ref.head().map_or(1000, |h| h.units_per_em()) as f32,
                                units: HashMap::new(),
                            });
                        let metrics = font_ref.glyph_metrics(
                            skrifa::instance::Size::unscaled(),
                            skrifa::instance::LocationRef::new(&coords),
                        );
                        for g in gr.glyphs() {
                            entry.units.entry(g.id).or_insert_with(|| {
                                metrics
                                    .advance_width(skrifa::GlyphId::new(g.id))
                                    .unwrap_or(0.0)
                            });
                        }
                    }
                }
            }
        }
        p
    }

    /// Load the vertical metrics of every face the layout uses (`vertical.rs`).
    pub(crate) fn load_vertical(&mut self, layout: &Layout<SpanBrush>, blobs: &[FontBlob]) {
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let font = gr.run().font();
                let key = (font.data.id(), font.index);
                if self.vertical.contains_key(&key) {
                    continue;
                }
                let published = crate::text::fonts::environment::published_blob_id(key.0);
                let m = blobs
                    .iter()
                    .find(|b| b.id.0 == published)
                    .map(|b| {
                        super::vertical::VerticalMetrics::parse(&b.bytes, key.1, |d, i, t| {
                            sfnt_table(d, i, t).map(<[u8]>::to_vec)
                        })
                    })
                    .unwrap_or_default();
                self.vertical.insert(key, Arc::new(m));
            }
        }
    }

    /// The advance of an upright glyph along a vertical line, whole pixels (`vertical.rs`); the run's
    /// font ascent and descent (px) are the fallback.
    pub(crate) fn upright_advance(
        &self,
        face: FaceKey,
        glyph: u32,
        ascent: f32,
        descent: f32,
    ) -> f32 {
        match self.vertical.get(&(face.blob, face.index)) {
            Some(m) => m.advance(glyph, face.size, ascent, descent),
            None => ascent.round() + descent.round(),
        }
    }

    /// The vertical origin of an upright glyph (px below the top of its vertical em box).
    pub(crate) fn upright_origin(&self, face: FaceKey, glyph: u32, ascent: f32) -> f32 {
        match self.vertical.get(&(face.blob, face.index)) {
            Some(m) => m.origin(glyph, face.size, ascent),
            None => ascent.round(),
        }
    }

    /// The whole-pixel advance of one shaped glyph (module docs). Nominal advances
    /// are first quantized to the reference font engine’s 26.6 fixed-point precision.
    pub(crate) fn snap(&self, face: FaceKey, glyph: u32, shaped: f32) -> f32 {
        if self.subpixel {
            return shaped;
        }
        if face.varied {
            let nominal = self
                .instances
                .get(&(face.blob, face.index, face.coords))
                .and_then(|i| i.units.get(&glyph).map(|u| u * face.size / i.upem));
            return match nominal {
                Some(n) => ((n * 64.0).round() / 64.0).round() + (shaped - n).round(),
                None => shaped.round(),
            };
        }
        let table = self
            .faces
            .get(&(face.blob, face.index))
            .and_then(Option::as_ref);
        match table {
            Some(t) => {
                let nominal = t.advance(glyph, face.size);
                ((nominal * 64.0).round() / 64.0).round() + (shaped - nominal).round()
            }
            None => shaped.round(),
        }
    }
}

/// What one unit of the table is. The table's unit sequence is the paragraph's logical order of
/// text clusters, atomic inlines and inline-box edge spacers; a line is a range of units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum UnitKind {
    Text,
    /// An atomic inline, tagged with the caller's item tag. Its width comes from the scratch
    /// layout (`with_atomic_widths`); in a shaped paragraph it is zero.
    Atomic(u64),
    /// Edge spacer (padding + border + margin) of an inline box. Parley does not see it: it adds
    /// to the whole-pixel width only.
    Edge {
        start: bool,
    },
}

/// One logical unit of the paragraph: a cluster (grapheme) with Parley's fractional advance and
/// the policy's whole-pixel advance (a ligature's whole advance sits on its first cluster), an
/// atomic inline, or an edge spacer. Non-text units have an empty byte range at their position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClusterAdvance {
    pub start: usize,
    pub end: usize,
    pub fractional: f32,
    pub snapped: f32,
    /// A space that hangs at a soft wrap (and at the end of the paragraph).
    pub hangs: bool,
    pub kind: UnitKind,
    /// A tab character: its width is the distance to the next tab stop (module `tab_stops`).
    pub tab: bool,
}

impl ClusterAdvance {
    /// Order among units at one byte position: the edges and atomics in source order (an inline
    /// box around only an atomic has its start edge, the atomic and its end edge at one position),
    /// then the text that starts there.
    fn rank(&self) -> u8 {
        match self.kind {
            UnitKind::Edge { .. } | UnitKind::Atomic(_) => 1,
            UnitKind::Text => 2,
        }
    }
}

/// The paragraph's units in logical order and its break opportunities (as unit indices), from
/// which the breaker decides each line with whole-pixel widths.
#[derive(Clone, Debug, Default)]
pub(crate) struct SnapTable {
    pub clusters: Vec<ClusterAdvance>,
    /// Unit indices before which a line may end (soft wrap opportunities), ascending; the
    /// paragraph end (`clusters.len()`) is always last.
    pub opportunities: Vec<usize>,
    /// Unit indices before which a line must end (forced breaks), ascending.
    pub forced: Vec<usize>,
    /// Emergency break positions (`overflow-wrap: anywhere | break-word`): between the clusters
    /// of a word, ascending, each with whether `anywhere` made it a soft wrap opportunity for
    /// min-content sizing. A line breaks at one only when no opportunity fits.
    pub emergency: Vec<(usize, bool)>,
    /// Where a line may end with a hyphen, ascending by unit (015 TXT-3). The soft ones are also in
    /// `opportunities`; the automatic ones are not (they are no break opportunity for min-content).
    pub hyphens: Vec<HyphenPoint>,
    /// The break positions the breaker chooses from: `opportunities` and the automatic hyphenation
    /// points, ascending (the paragraph end is last).
    pub breaks: Vec<usize>,
}

/// A unit boundary where a line may end with a hyphen (015 TXT-3): after a soft hyphen (`soft`) or
/// inside a word at a hyphenation point of `hyphens: auto`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HyphenPoint {
    /// The unit index the line ends before.
    pub unit: usize,
    /// The whole-pixel width of the hyphen drawn there.
    pub width: f32,
    /// The span whose shaped hyphen it is (`ShapedParagraph::hyphen_runs`).
    pub span: u32,
    pub soft: bool,
    /// A point of a word that no break opportunity follows (the last word of the paragraph): the unit the word
    /// starts at. A line ends at such a point only when it starts at or after that unit (in the word).
    pub word_start: Option<usize>,
}

/// What hyphenation gives `SnapTable::build`: the automatic break points (bytes, ascending) and the
/// shaped hyphen of each span.
pub(crate) struct HyphenInput<'a> {
    pub auto: &'a [usize],
    pub auto_final: &'a [(usize, usize)],
    pub runs: &'a [Option<super::source::SynthRun>],
}

/// Widths of a unit range: whole-pixel and fractional, without and with hanging spaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpanWidths {
    pub snapped: f32,
    pub snapped_full: f32,
    pub fractional: f32,
    pub fractional_full: f32,
}

/// The most lines a paragraph may have for `text-wrap-style: balance` to act (the reference stops at six).
pub(crate) const BALANCE_MAX_LINES: usize = 6;

impl SnapTable {
    /// Glyph-level snapping summed per cluster, from an unbounded break of `layout`; break
    /// opportunities from a zero-width break of a copy (every soft wrap opportunity ends a line).
    /// Atomic inlines and edge spacers of `items` are merged in at their byte positions; there is
    /// a soft wrap opportunity before and after every atomic inline (CSS Text 3 §5.1: an atomic
    /// inline behaves as U+FFFC).
    pub(crate) fn build(
        layout: &Layout<SpanBrush>,
        policy: &AdvancePolicy,
        text: &str,
        items: &[InlineItem],
        spans: &[SourceSpan],
        hy: &HyphenInput<'_>,
    ) -> SnapTable {
        let mut unbounded = layout.clone();
        unbounded.break_all_lines(None);
        let mut clusters = Vec::new();
        for line in unbounded.lines() {
            for run in line.runs() {
                let face = FaceKey::of(&run);
                let fm = run.font_metrics();
                for c in run.clusters() {
                    let r = c.text_range();
                    // `text-combine-upright: all`: the span is one em long, on its first cluster (015 WM-1b).
                    let combined = spans
                        .iter()
                        .find(|s| s.range.contains(&r.start))
                        .filter(|s| {
                            s.props.combine_upright
                                && s.props.orientation
                                    != super::vertical::GlyphOrientation::Horizontal
                        });
                    if let Some(s) = combined {
                        clusters.push(ClusterAdvance {
                            start: r.start,
                            end: r.end,
                            fractional: c.advance(),
                            snapped: if r.start == s.range.start {
                                run.font_size()
                            } else {
                                0.0
                            },
                            hangs: false,
                            kind: UnitKind::Text,
                            tab: false,
                        });
                        continue;
                    }
                    // An upright cluster of a vertical line advances by its vertical advance (015 WM-1b).
                    let upright = spans
                        .iter()
                        .find(|s| s.range.contains(&r.start))
                        .is_some_and(|s| {
                            text.get(r.clone())
                                .and_then(|t| t.chars().next())
                                .is_some_and(|ch| s.props.orientation.upright(ch))
                        });
                    let snapped: f32 = if upright {
                        c.glyphs()
                            .map(|g| policy.upright_advance(face, g.id, fm.ascent, fm.descent))
                            .sum()
                    } else {
                        c.glyphs().map(|g| policy.snap(face, g.id, g.advance)).sum()
                    };
                    clusters.push(ClusterAdvance {
                        start: r.start,
                        end: r.end,
                        fractional: c.advance(),
                        snapped,
                        hangs: hangs(text.get(r.clone()).unwrap_or(""))
                            && span_hangs(spans, r.start),
                        kind: UnitKind::Text,
                        tab: text.get(r.clone()) == Some("\t"),
                    });
                }
            }
        }
        clusters.sort_by_key(|c| c.start);
        // Merge the non-text units: stable by (byte position, rank, source order).
        let mut units = clusters;
        let text_units = units.len();
        for it in items {
            let (kind, snapped) = match it.kind {
                InlineItemKind::AtomicInline => (UnitKind::Atomic(it.owner), 0.0),
                InlineItemKind::EdgeSpacer { start } => (UnitKind::Edge { start }, it.extent),
                _ => continue,
            };
            let at = it.range.start;
            units.push(ClusterAdvance {
                start: at,
                end: at,
                fractional: 0.0,
                snapped,
                hangs: false,
                kind,
                tab: false,
            });
        }
        if units.len() > text_units {
            // `sort_by_key` is stable: text units keep their order, items keep source order.
            units.sort_by_key(|u| (u.start, u.rank()));
        }
        tab_stops(&mut units, text, spans);
        // The unit index where a line that ends its text at byte `at` ends: after the end edges
        // that follow that text directly (they close boxes whose content ended there), before the
        // start edges and atomics at `at`.
        let boundary = |at: usize| {
            let mut i = units.partition_point(|u| u.start < at);
            while units
                .get(i)
                .is_some_and(|u| u.start == at && u.kind == (UnitKind::Edge { start: false }))
            {
                i += 1;
            }
            i
        };

        let mut narrow = layout.clone();
        narrow.break_all_lines(Some(0.0));
        let (mut opportunities, mut forced, mut emergency) = (Vec::new(), Vec::new(), Vec::new());
        let mut hyphens: Vec<HyphenPoint> = Vec::new();
        let mut auto_units: Vec<usize> = Vec::new();
        for line in narrow.lines() {
            let end = boundary(line.text_range().end);
            match line.break_reason() {
                parley::BreakReason::Regular => {
                    // A soft hyphen or a hyphenation point ends the line with a hyphen.
                    let byte = line.text_range().end;
                    let soft = text
                        .get(..byte)
                        .is_some_and(|t| t.ends_with(super::hyphen::SOFT_HYPHEN));
                    let final_pos = hy
                        .auto_final
                        .binary_search_by_key(&byte, |p| p.0)
                        .ok()
                        .map(|i| hy.auto_final[i].1);
                    let auto = hy.auto.binary_search(&byte).is_ok() || final_pos.is_some();
                    if soft || auto {
                        let span = spans
                            .iter()
                            .position(|s| s.range.contains(&byte.saturating_sub(1)));
                        let run = span.and_then(|i| hy.runs.get(i)).and_then(|r| r.as_ref());
                        if let (Some(span), Some(run)) = (span, run) {
                            let word_start = if soft {
                                None
                            } else {
                                final_pos.map(|b| units.partition_point(|c| c.start < b))
                            };
                            hyphens.push(HyphenPoint {
                                unit: end,
                                width: run.width,
                                span: span as u32,
                                soft,
                                word_start,
                            });
                            if soft {
                                opportunities.push(end);
                            } else {
                                auto_units.push(end);
                            }
                        } else if soft {
                            opportunities.push(end);
                        }
                    } else {
                        opportunities.push(end);
                    }
                }
                parley::BreakReason::Explicit => {
                    opportunities.push(end);
                    forced.push(end);
                }
                parley::BreakReason::Emergency => {
                    let last = line.text_range().end.saturating_sub(1);
                    let anywhere = spans
                        .iter()
                        .find(|s| s.range.contains(&last))
                        .is_some_and(|s| s.props.overflow_wrap == WrapOverflow::Anywhere);
                    emergency.push((end, anywhere));
                }
                parley::BreakReason::None => {}
            }
        }
        for (i, u) in units.iter().enumerate() {
            if matches!(u.kind, UnitKind::Atomic(_)) {
                // An atomic inline in a non-wrapping context (`white-space: nowrap | pre`) is no wrap
                // opportunity: the text next to it decides (CSS Text 4 `text-wrap-mode`). A list
                // marker leading a `nowrap` item is the case that matters.
                let nowrap_at = |at: usize| {
                    spans
                        .iter()
                        .find(|s| s.range.contains(&at))
                        .is_some_and(|s| s.props.nowrap)
                };
                // The start edges of the boxes around the atomic go with it, and so do the end
                // edges that close them: edges are no wrap opportunities.
                let mut before = i;
                while before > 0
                    && units[before - 1].start == u.start
                    && units[before - 1].kind == (UnitKind::Edge { start: true })
                {
                    before -= 1;
                }
                let mut after = i + 1;
                while units.get(after).is_some_and(|n| {
                    n.start == u.start && n.kind == (UnitKind::Edge { start: false })
                }) {
                    after += 1;
                }
                if !(u.start > 0 && nowrap_at(u.start - 1)) {
                    opportunities.push(before);
                }
                if !nowrap_at(u.start) {
                    opportunities.push(after);
                }
            }
        }
        opportunities.push(units.len());
        opportunities.retain(|&o| o > 0);
        opportunities.sort_unstable();
        opportunities.dedup();
        forced.retain(|&o| o > 0);
        forced.sort_unstable();
        forced.dedup();
        emergency.retain(|&(o, _)| o > 0 && o < units.len());
        emergency.sort_unstable();
        emergency.dedup_by_key(|e| e.0);
        hyphens.sort_by_key(|h| h.unit);
        hyphens.dedup_by_key(|h| h.unit);
        auto_units.retain(|&o| o > 0 && o < units.len());
        let mut breaks: Vec<usize> = opportunities.iter().copied().chain(auto_units).collect();
        breaks.sort_unstable();
        breaks.dedup();
        SnapTable {
            clusters: units,
            opportunities,
            forced,
            emergency,
            hyphens,
            breaks,
        }
    }

    /// The hyphen a line that ends before unit `unit` shows, if any.
    pub(crate) fn hyphen_at(&self, unit: usize) -> Option<&HyphenPoint> {
        self.hyphens
            .binary_search_by_key(&unit, |h| h.unit)
            .ok()
            .map(|i| &self.hyphens[i])
    }

    /// The whole-pixel width of the line made of units `r`, hanging spaces excluded, with the hyphen
    /// when it ends at a hyphenation point.
    pub(crate) fn line_width(&self, r: Range<usize>) -> f32 {
        let end = r.end;
        self.widths(r).snapped + self.hyphen_at(end).map_or(0.0, |h| h.width)
    }

    /// A copy with every atomic inline's width set to `width(item)` (fractional and whole-pixel).
    pub(crate) fn with_atomic_widths(&self, width: impl Fn(u64) -> f32) -> SnapTable {
        let mut t = self.clone();
        for u in &mut t.clusters {
            if let UnitKind::Atomic(item) = u.kind {
                let w = width(item);
                u.fractional = w;
                u.snapped = w;
            }
        }
        t
    }

    /// Index of the atomic inline `item`.
    pub(crate) fn atomic_unit(&self, item: u64) -> Option<usize> {
        self.clusters
            .iter()
            .position(|u| u.kind == UnitKind::Atomic(item))
    }

    /// One past the last text unit that starts before byte `at`: the unit index right after the
    /// text that ends at `at` (edge spacers and atomics at `at` come after it).
    pub(crate) fn text_units_end(&self, at: usize) -> usize {
        self.clusters
            .iter()
            .rposition(|u| u.kind == UnitKind::Text && u.start < at)
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// Index of the first text unit that starts at or after byte `at` (`unit_count` when none).
    pub(crate) fn first_text_unit_at(&self, at: usize) -> usize {
        self.clusters
            .iter()
            .position(|u| u.kind == UnitKind::Text && u.start >= at)
            .unwrap_or(self.clusters.len())
    }

    pub(crate) fn widths(&self, r: Range<usize>) -> SpanWidths {
        let cs = &self.clusters[r.start.min(self.clusters.len())..r.end.min(self.clusters.len())];
        let trailing = cs.iter().rev().take_while(|c| c.hangs).count();
        let body = &cs[..cs.len() - trailing];
        SpanWidths {
            snapped: body.iter().map(|c| c.snapped).sum(),
            snapped_full: cs.iter().map(|c| c.snapped).sum(),
            fractional: body.iter().map(|c| c.fractional).sum(),
            fractional_full: cs.iter().map(|c| c.fractional).sum(),
        }
    }

    /// Whole-pixel min-content and max-content widths (hanging spaces excluded).
    pub(crate) fn content_widths(&self) -> (f32, f32) {
        let (mut min, mut max) = (0.0f32, 0.0f32);
        let mut start = 0;
        // `overflow-wrap: anywhere` makes every position inside a word a soft wrap opportunity
        // for min-content (`break-word` does not).
        // Hyphenation points are break opportunities of min-content too, with the hyphen's width (measured:
        // an inline-block of `width: min-content` and `hyphens: auto`).
        let mut ops: Vec<usize> = self
            .breaks
            .iter()
            .copied()
            .chain(self.emergency.iter().filter(|e| e.1).map(|e| e.0))
            .collect();
        ops.sort_unstable();
        ops.dedup();
        for &o in &ops {
            min = min.max(self.line_width(start..o));
            start = o;
        }
        let mut start = 0;
        let ends = self.forced.iter().copied().chain(std::iter::once(
            self.opportunities.last().copied().unwrap_or(0),
        ));
        for f in ends {
            max = max.max(self.widths(start..f).snapped);
            start = f;
        }
        (min, max)
    }

    /// The line ending the policy chooses for a line starting at unit `start` in `available` px:
    /// the furthest opportunity whose whole-pixel width fits, else the first one (overflow),
    /// never past a forced break. Returns the end unit and the max advance that makes Parley's
    /// fractional breaker end the line there.
    pub(crate) fn choose(&self, start: usize, available: f32) -> Option<(usize, f32)> {
        self.choose_with(start, available, true)
    }

    /// [`Self::choose`], with or without the automatic hyphenation points as break candidates (balancing leaves
    /// them out, as the reference does: a balanced paragraph with `hyphens: auto` shows no automatic hyphen).
    pub(crate) fn choose_with(
        &self,
        start: usize,
        available: f32,
        auto_hyphens: bool,
    ) -> Option<(usize, f32)> {
        let limit = self
            .forced
            .iter()
            .copied()
            .find(|&f| f > start)
            .unwrap_or(usize::MAX);
        let all = if auto_hyphens {
            &self.breaks
        } else {
            &self.opportunities
        };
        let candidates: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&o| o > start && o <= limit)
            // the points of the last word of the paragraph: only for a line that starts in the word
            .filter(|&o| {
                self.hyphen_at(o)
                    .and_then(|h| h.word_start)
                    .is_none_or(|w| start >= w)
            })
            .collect();
        let first = *candidates.first()?;
        // Chromium compares LayoutUnit widths; whole-pixel widths need only a float tolerance.
        let fits = |e: usize| self.line_width(start..e) <= available + 1e-3;
        let idx = candidates.iter().rposition(|&e| fits(e)).unwrap_or(0);
        if !fits(candidates[idx]) {
            // Nothing fits: an emergency break inside the first word, when `overflow-wrap` allows
            // one, at the last position that fits (at least one cluster per line).
            let inside: Vec<usize> = self
                .emergency
                .iter()
                .map(|e| e.0)
                .filter(|&e| e > start && e < first)
                .collect();
            if !inside.is_empty() {
                let k = inside.iter().rposition(|&e| fits(e)).unwrap_or(0);
                let end = inside[k];
                let here = self.widths(start..end).fractional;
                let next = inside.get(k + 1).copied().unwrap_or(first);
                let max = here + (self.widths(start..next).fractional - here) / 2.0;
                return Some((end, max.max(here)));
            }
        }
        let end = candidates[idx];
        let here = self.widths(start..end).fractional;
        // (the next opportunity Parley knows, whether this line may use it or not)
        let next_known = all.iter().copied().find(|&o| o > end && o <= limit);
        let max = match next_known {
            // Half-way to the next opportunity's width: robust against summation order.
            Some(next) => here + (self.widths(start..next).fractional - here) / 2.0,
            None => here.max(available) + 1.0,
        };
        debug_assert!(first <= end);
        Some((end, max.max(here)))
    }

    /// The line a line starting at unit `start` makes when it must end before unit `end` (the first line of a
    /// paragraph whose first formatted line was decided elsewhere, 015 TXT-5): the end and the max advance that makes
    /// Parley's fractional breaker end the line there (as [`Self::choose`]).
    pub(crate) fn choose_to(&self, start: usize, end: usize, available: f32) -> (usize, f32) {
        let limit = self
            .forced
            .iter()
            .copied()
            .find(|&f| f > start)
            .unwrap_or(usize::MAX);
        let end = end.max(start + 1).min(self.clusters.len()).min(limit);
        let here = self.widths(start..end).fractional;
        let next_known = self
            .breaks
            .iter()
            .chain(self.emergency.iter().map(|e| &e.0))
            .copied()
            .filter(|&o| o > end && o <= limit)
            .min();
        let max = match next_known {
            Some(next) => here + (self.widths(start..next).fractional - here) / 2.0,
            None => {
                here.max(if available.is_finite() {
                    available
                } else {
                    0.0
                }) + 1.0
            }
        };
        (end, max.max(here))
    }

    /// The position of the boundary before unit `unit`: its byte offset and how many edge spacers and atomic
    /// inlines come before it. Two paragraphs of the same box subtree have the same positions even when their
    /// clusters differ.
    pub(crate) fn unit_position(&self, unit: usize) -> (usize, usize) {
        let nontext = self.clusters[..unit.min(self.clusters.len())]
            .iter()
            .filter(|c| c.kind != UnitKind::Text)
            .count();
        let byte = match self.clusters.get(unit) {
            Some(c) => c.start,
            None => self.clusters.last().map_or(0, |c| c.end),
        };
        (byte, nontext)
    }

    /// The boundary with byte offset `byte` and `nontext` non-text units before it (see [`Self::unit_position`]).
    pub(crate) fn unit_at(&self, byte: usize, nontext: usize) -> Option<usize> {
        let mut nt = 0;
        for (i, c) in self.clusters.iter().enumerate() {
            if nt == nontext && c.start == byte {
                return Some(i);
            }
            if c.kind != UnitKind::Text {
                nt += 1;
            }
        }
        (nt == nontext && self.clusters.last().is_none_or(|c| c.end == byte))
            .then_some(self.clusters.len())
    }

    /// How many lines the paragraph takes broken greedily into `first` (the first line) and `room` (the others)
    /// whole pixels; `None` when a line is wider than its room (an unbreakable word) or nothing can break.
    fn greedy_lines(&self, first: f32, room: f32) -> Option<usize> {
        let (end, mut start, mut n) = (self.clusters.len(), 0usize, 0usize);
        while start < end {
            let avail = if n == 0 { first } else { room };
            let (e, _) = self.choose_with(start, avail, false)?;
            if e <= start || self.line_width(start..e) > avail + 1e-3 || n >= 64 {
                return None;
            }
            start = e;
            n += 1;
        }
        Some(n)
    }

    /// `text-wrap-style: balance` as the reference does it (015 TXT-4): for a paragraph of two to
    /// `BALANCE_MAX_LINES` lines without forced breaks, the narrowest whole-pixel width `w` that keeps the
    /// number of lines (binary search), then the breaks into that many lines no wider than `w` that minimise
    /// the sum of the squares of the unused room of every line, the last included. Returns the width of each
    /// line, to be given to the breaker as the line's `max_advance`. `first_room` is the room of the first
    /// line (`room` less `text-indent`).
    pub(crate) fn balance_plan(&self, first_room: f32, room: f32) -> Option<Vec<f32>> {
        if !self.forced.is_empty() || !room.is_finite() || !first_room.is_finite() {
            return None;
        }
        let indent = (room - first_room).max(0.0);
        let full = self.greedy_lines(first_room, room)?;
        if !(2..=BALANCE_MAX_LINES).contains(&full) {
            return None;
        }
        let feasible = |w: f32| {
            self.greedy_lines((w - indent).max(0.0), w)
                .is_some_and(|n| n <= full)
        };
        let top = room.floor();
        if top < 1.0 || !feasible(top) {
            return None;
        }
        let (mut lo, mut hi) = (1.0f32, top);
        while lo < hi {
            let mid = ((lo + hi) / 2.0).floor();
            if feasible(mid) {
                hi = mid;
            } else {
                lo = mid + 1.0;
            }
        }
        let w = hi;
        let mut pos = vec![0usize];
        pos.extend(self.opportunities.iter().copied());
        let m = pos.len();
        let mut cost = vec![vec![f64::INFINITY; m]; full + 1];
        let mut from = vec![vec![usize::MAX; m]; full + 1];
        cost[0][0] = 0.0;
        for k in 1..=full {
            let avail = if k == 1 { (w - indent).max(0.0) } else { w };
            for j in 1..m {
                for i in 0..j {
                    if !cost[k - 1][i].is_finite() {
                        continue;
                    }
                    let width = self.line_width(pos[i]..pos[j]);
                    if width > avail + 1e-3 {
                        continue;
                    }
                    let slack = f64::from(avail - width);
                    let c = cost[k - 1][i] + slack * slack;
                    if c < cost[k][j] - 1e-9 {
                        cost[k][j] = c;
                        from[k][j] = i;
                    }
                }
            }
        }
        let mut j = m - 1;
        if !cost[full][j].is_finite() {
            return None;
        }
        let mut widths = vec![0.0f32; full];
        for k in (1..=full).rev() {
            let i = from[k][j];
            widths[k - 1] = self.line_width(pos[i]..pos[j]);
            j = i;
        }
        Some(widths)
    }

    /// The line end whose fractional full advance is nearest `advance` (re-synchronises the
    /// cursor with the line Parley committed).
    pub(crate) fn resync(&self, start: usize, advance: f32, hint: usize) -> usize {
        let mut best = (
            hint,
            (self.widths(start..hint).fractional_full - advance).abs(),
        );
        for &o in self
            .breaks
            .iter()
            .chain(self.emergency.iter().map(|e| &e.0))
            .filter(|&&o| o > start)
        {
            let d = (self.widths(start..o).fractional_full - advance).abs();
            if d + 1e-4 < best.1 {
                best = (o, d);
            }
        }
        best.0
    }
}

/// Tab stops (CSS Text 3 section 4.2): a tab advances to the next multiple of the `tab-size`
/// interval from the start of the line; a stop closer than half a space is skipped. The interval of
/// a number is that many advances of the shaped space (the tab is shaped as a space, so its own
/// width is that advance, spacing included). Lines are the paragraph's forced lines: the pen
/// restarts after each preserved newline. A soft wrap before a tab is not known here, so the
/// tabs of a wrapped line are measured from the start of their forced line (the caller reports
/// that case).
fn tab_stops(units: &mut [ClusterAdvance], text: &str, spans: &[SourceSpan]) {
    if !units.iter().any(|u| u.tab) {
        return;
    }
    let mut pen = 0.0f32;
    for u in units.iter_mut() {
        if u.tab {
            let space = u.snapped.max(1.0);
            let spec = spans
                .iter()
                .find(|s| s.range.contains(&u.start))
                .map(|s| s.props.tab_size)
                .unwrap_or_default();
            let interval = match spec {
                TabSpec::Spaces(n) => n * space,
                TabSpec::Px(p) => p,
            };
            let width = if interval > 0.0 && interval.is_finite() {
                let mut next = ((pen / interval).floor() + 1.0) * interval;
                if next - pen < space / 2.0 {
                    next += interval;
                }
                (next - pen).round()
            } else {
                0.0
            };
            u.snapped = width;
            u.fractional = width;
        }
        pen += u.snapped;
        if text.get(u.start..u.end).is_some_and(|t| t.ends_with('\n')) {
            pen = 0.0;
        }
    }
}

/// Whether a space of the span at byte `at` may hang at the end of a line (CSS Text 4 section 4.3):
/// not under `break-spaces`, and not under `pre` (preserved spaces of a non-wrapping span).
fn span_hangs(spans: &[SourceSpan], at: usize) -> bool {
    match spans.iter().find(|s| s.range.contains(&at)) {
        Some(s) => match s.props.collapse {
            SpanCollapse::Collapse => true,
            SpanCollapse::Preserve => !s.props.nowrap,
            SpanCollapse::BreakSpaces => false,
        },
        None => true,
    }
}

/// Spaces and tabs hang at the end of a line (CSS Text 3 §4.1.2); no-break spaces do not.
fn hangs(cluster_text: &str) -> bool {
    !cluster_text.is_empty() && cluster_text.chars().all(|c| c == ' ' || c == '\t')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::fonts::TextServices;
    use crate::text::paragraph::source::{LineHeightSpec, ParagraphSource, TextStyle};
    use std::path::Path;
    use ui4me_assets::fonts::{FontRegistry, PinnedFontConfig};
    use ui4me_engine_types::ids::{DocumentId, Slots};
    use ui4me_engine_types::text_metrics::{FamilyName, FontSpec};

    /// Kerned glyphs round the nominal advance and the kerning adjustment separately (the
    /// reference platform's rule), which differs from rounding the kerned advance.
    #[test]
    fn kerning_is_rounded_separately_from_the_nominal_advance() {
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
        let reg = FontRegistry::pinned(&PinnedFontConfig::with_files([font])).unwrap();
        let mut ts = TextServices::new(&reg.font_sources()).unwrap();
        // Exercise the whole-pixel policy independently of the host platform.
        ts.subpixel = false;
        let style = TextStyle {
            font: FontSpec {
                families: vec![FamilyName::Named("Noto Sans".into())].into(),
                ..FontSpec::default()
            },
            font_size: 16.0,
            line_height: LineHeightSpec::Px(20.0),
        };
        let mut s = Slots::new();
        let mut src = ParagraphSource::new(DocumentId::from_raw(s.alloc()), style.clone());
        src.push("AVAV", &style, 1);
        let shaped = ts.shape(&src);
        let mut layout = shaped.layout.clone();
        layout.break_all_lines(None);
        let line = layout.lines().next().unwrap();
        let mut checked = 0;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else {
                continue;
            };
            let font = gr.run().font();
            let face = FaceKey {
                blob: font.data.id(),
                index: font.index,
                size: 16.0,
                varied: false,
                coords: 0,
            };
            let table = shaped.policy.faces[&(face.blob, face.index)]
                .clone()
                .unwrap();
            for g in gr.glyphs() {
                let nominal = table.advance(g.id, 16.0);
                let snapped = shaped.policy.snap(face, g.id, g.advance);
                assert_eq!(snapped, nominal.round() + (g.advance - nominal).round());
                assert_eq!(snapped.fract(), 0.0);
                checked += 1;
            }
        }
        assert_eq!(checked, 4);
        // At least one kerned pair in "AVAV" takes a whole pixel off its rounded nominal advance.
        assert!(
            shaped
                .snap
                .clusters
                .iter()
                .any(|c| c.snapped < c.fractional.round()),
            "{:?}",
            shaped.snap.clusters
        );
    }
}
