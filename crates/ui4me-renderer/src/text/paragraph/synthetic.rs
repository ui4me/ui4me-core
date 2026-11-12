//! owner: 009-text-inline (T025; FR-043, FR-090; research R-05, R-12, R-21; 008
//! `ifc-interface.md` section 5.1 `shape_synthetic`)
//!
//! Synthesized runs without DOM offsets: the ellipsis, the soft-hyphen hyphen and the
//! `text-overflow` string. The text is shaped with the first available font of the root style
//! (ellipsis, string) or of the span at `at_cluster` (hyphen), with the paragraph's own
//! whole-pixel text metrics policy, so a synthesized run measures exactly as ordinary text of
//! that font does. The operation is pure: it reads the shaped paragraph and the font
//! environment and changes neither.

use parley::{PositionedLayoutItem, StyleProperty};

use super::snap::{AdvancePolicy, FaceKey};
use super::source::{
    ShapedParagraph, SpanBrush, SynthFont, SynthGlyph, SynthKind, SynthRequest, SynthRun, TextStyle,
};
use crate::text::fonts::TextServices;

/// U+2026 HORIZONTAL ELLIPSIS, and the fallback when the font lacks the glyph.
const ELLIPSIS: &str = "\u{2026}";
const ELLIPSIS_FALLBACK: &str = "...";
/// U+2010 HYPHEN, falling back to U+002D HYPHEN-MINUS when the font lacks it (CSS Text 3 5.4.1).
const HYPHEN: &str = "\u{2010}";
const HYPHEN_FALLBACK: &str = "-";

/// One shaped candidate: text, whole-pixel width, whole-pixel ascent and descent, and whether
/// every visible character has a glyph.
struct Shaped {
    width: f32,
    ascent: f32,
    descent: f32,
    complete: bool,
    glyphs: Vec<SynthGlyph>,
    font: Option<SynthFont>,
}

impl TextServices {
    /// `shape_synthetic` (section 5.1): shape an ellipsis, hyphen or `text-overflow` string with
    /// the root's first available font (or the font of the run at `at_cluster`, a byte offset of
    /// the processed text, for a hyphen). U+2026 falls back to "..." and U+2010 to "-" when the
    /// glyph is missing; a `String` is shaped as given.
    pub fn shape_synthetic(
        &mut self,
        shaped: &ShapedParagraph,
        request: &SynthRequest,
    ) -> SynthRun {
        let source = shaped.source();
        let style: TextStyle = match request.kind {
            SynthKind::Hyphen => source
                .spans
                .iter()
                .find(|s| {
                    s.range.contains(&request.at_cluster) || s.range.end == request.at_cluster
                })
                .map(|s| s.style.clone())
                .unwrap_or_else(|| source.root.clone()),
            SynthKind::Ellipsis | SynthKind::String(_) => source.root.clone(),
        };
        let candidates: Vec<String> = match &request.kind {
            SynthKind::Ellipsis => vec![ELLIPSIS.into(), ELLIPSIS_FALLBACK.into()],
            SynthKind::Hyphen => vec![HYPHEN.into(), HYPHEN_FALLBACK.into()],
            SynthKind::String(text) => vec![text.clone()],
        };
        self.shape_synth_candidates(request.kind.clone(), &style, candidates, request.at_cluster)
    }

    /// The first of `candidates` every visible character of which has a glyph in `style`'s font (the
    /// last when none is complete), shaped as a synthesized run.
    pub(crate) fn shape_synth_candidates(
        &mut self,
        kind: SynthKind,
        style: &TextStyle,
        candidates: Vec<String>,
        at_cluster: usize,
    ) -> SynthRun {
        let mut chosen: Option<(String, Shaped)> = None;
        for text in candidates {
            if text.is_empty() {
                chosen = Some((
                    text,
                    Shaped {
                        width: 0.0,
                        ascent: 0.0,
                        descent: 0.0,
                        complete: true,
                        glyphs: Vec::new(),
                        font: None,
                    },
                ));
                break;
            }
            let sh = self.shape_text(&text, style);
            let complete = sh.complete;
            chosen = Some((text, sh));
            if complete {
                break;
            }
        }
        // `candidates` is never empty.
        let (text, sh) = chosen.unwrap_or_else(|| {
            (
                String::new(),
                Shaped {
                    width: 0.0,
                    ascent: 0.0,
                    descent: 0.0,
                    complete: true,
                    glyphs: Vec::new(),
                    font: None,
                },
            )
        });
        SynthRun {
            kind,
            text,
            width: sh.width,
            ascent: sh.ascent,
            descent: sh.descent,
            anchor_cluster: at_cluster,
            glyphs: sh.glyphs,
            font: sh.font,
        }
    }

    fn shape_text(&mut self, text: &str, style: &TextStyle) -> Shaped {
        let props =
            super::source::text_properties(self, style, &super::source::SpanProps::default(), true);
        let mut b = self.lcx.ranged_builder(&mut self.env.fcx, text, 1.0, true);
        for p in props {
            b.push_default(p);
        }
        b.push_default(StyleProperty::Brush(SpanBrush(u32::MAX)));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        let blobs = self.env.blobs_for_layout(&layout);
        let policy = AdvancePolicy::for_layout(&layout, &blobs, self.subpixel);
        let (mut width, mut ascent, mut descent, mut complete) = (0.0f32, 0.0f32, 0.0f32, true);
        let (mut glyphs, mut synth_font) = (Vec::new(), None);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let run = gr.run();
                let font = run.font();
                let face = FaceKey::of(run);
                if synth_font.is_none() {
                    let syn = run.synthesis();
                    synth_font = Some(SynthFont {
                        blob: crate::text::fonts::environment::published_blob_id(font.data.id()),
                        index: font.index,
                        coords: run
                            .normalized_coords()
                            .iter()
                            .map(|c| c.to_bits())
                            .collect(),
                        embolden: syn.embolden(),
                        oblique_angle_deg: syn.skew(),
                        size_px: run.font_size(),
                    });
                }
                for g in gr.glyphs() {
                    let advance = policy.snap(face, g.id, g.advance);
                    glyphs.push(SynthGlyph {
                        id: g.id,
                        x: width + g.x,
                        y: g.y,
                        advance,
                    });
                    width += advance;
                    if g.id == 0 {
                        complete = false;
                    }
                }
                let m = run.font_metrics();
                ascent = ascent.max(m.ascent.round());
                descent = descent.max(m.descent.round());
            }
        }
        Shaped {
            width,
            ascent,
            descent,
            complete,
            glyphs,
            font: synth_font,
        }
    }
}
