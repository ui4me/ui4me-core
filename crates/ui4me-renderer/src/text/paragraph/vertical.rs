//! owner: 009-text-inline (015 WM-1b, lane L-WM; plan §A5)
//!
//! Glyph orientation in vertical lines (CSS Writing Modes 4 section 5.1). A vertical paragraph is still
//! shaped and broken along its line as a horizontal one (the layout frame is logical); what differs is
//! per cluster:
//!
//! * **sideways** clusters (`text-orientation: sideways`, and in `mixed` the characters whose Unicode
//!   `Vertical_Orientation` (UAX #50) is `R`) advance by their horizontal advance and are drawn turned
//!   (the scene turns the run);
//! * **upright** clusters (all of `upright`, and in `mixed` the `U`, `Tu` and `Tr` characters, CJK among
//!   them) advance by the font's **vertical** advance (`vmtx`; without one, ascent + descent as the
//!   reference browser), are shaped with the `vert` feature (vertical alternates of punctuation) and are
//!   drawn upright: horizontally centred on the line's central baseline, their vertical origin (`VORG`;
//!   else the ascent) at the pen position.
//!
//! [`GlyphOrientation`] is a span property set by the inline layout for vertical roots.

use std::collections::HashMap;

/// The glyph orientation of a span (`text-orientation` in a vertical writing mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlyphOrientation {
    /// Horizontal text (the line is horizontal).
    #[default]
    Horizontal,
    Mixed,
    Upright,
    Sideways,
}

impl GlyphOrientation {
    /// Whether the cluster that starts with `c` is set upright.
    pub fn upright(self, c: char) -> bool {
        use icu_properties::props::VerticalOrientation as V;
        match self {
            GlyphOrientation::Horizontal | GlyphOrientation::Sideways => false,
            // Every character, spaces included (the reference browser advances an upright space by its vertical advance).
            GlyphOrientation::Upright => true,
            GlyphOrientation::Mixed => {
                let v = icu_properties::CodePointMapData::<V>::new().get(c);
                v == V::Upright || v == V::TransformedUpright || v == V::TransformedRotated
            }
        }
    }
}

/// The vertical metrics of a face: `vmtx` advances, `VORG` origins and the horizontal ascent and descent
/// (font units).
#[derive(Debug, Default)]
pub(crate) struct VerticalMetrics {
    upem: f32,
    advances: Vec<u16>,
    vorg_default: Option<i16>,
    vorg: HashMap<u16, i16>,
    ascender: i16,
    descender: i16,
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

impl VerticalMetrics {
    pub(crate) fn parse(
        data: &[u8],
        index: u32,
        table: impl Fn(&[u8], u32, &[u8; 4]) -> Option<Vec<u8>>,
    ) -> VerticalMetrics {
        let mut m = VerticalMetrics {
            upem: 1000.0,
            ..Default::default()
        };
        if let Some(head) = table(data, index, b"head") {
            m.upem = be16(&head, 18).filter(|u| *u > 0).unwrap_or(1000) as f32;
        }
        if let Some(hhea) = table(data, index, b"hhea") {
            m.ascender = be16(&hhea, 4).unwrap_or(0) as i16;
            m.descender = be16(&hhea, 6).unwrap_or(0) as i16;
        }
        if let (Some(vhea), Some(vmtx)) = (table(data, index, b"vhea"), table(data, index, b"vmtx"))
        {
            let n = be16(&vhea, 34).unwrap_or(0) as usize;
            m.advances = (0..n).filter_map(|i| be16(&vmtx, 4 * i)).collect();
        }
        if let Some(vorg) = table(data, index, b"VORG") {
            m.vorg_default = be16(&vorg, 4).map(|v| v as i16);
            let n = be16(&vorg, 6).unwrap_or(0) as usize;
            for i in 0..n {
                if let (Some(g), Some(y)) = (be16(&vorg, 8 + 4 * i), be16(&vorg, 10 + 4 * i)) {
                    m.vorg.insert(g, y as i16);
                }
            }
        }
        m
    }

    /// The whole-pixel vertical advance of `glyph` at `size` px; without `vmtx`, the rounded ascent plus
    /// the rounded descent of the run's font (`ascent`, `descent` in px).
    pub(crate) fn advance(&self, glyph: u32, size: f32, ascent: f32, descent: f32) -> f32 {
        if self.advances.is_empty() {
            return ascent.round() + descent.round();
        }
        let units = self
            .advances
            .get(glyph as usize)
            .or(self.advances.last())
            .copied()
            .unwrap_or(0);
        (units as f32 * size / self.upem).round()
    }

    /// The vertical origin of `glyph` (the distance from the top of its vertical em box down to its
    /// alphabetic baseline) at `size` px: `VORG`, else the font's ascent (`ascent` px).
    pub(crate) fn origin(&self, glyph: u32, size: f32, ascent: f32) -> f32 {
        match self
            .vorg
            .get(&(glyph as u16))
            .copied()
            .or(self.vorg_default)
        {
            Some(y) => y as f32 * size / self.upem,
            None if !self.advances.is_empty() && self.ascender > 0 => {
                self.ascender as f32 * size / self.upem
            }
            None => ascent.round(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn descender(&self) -> i16 {
        self.descender
    }
}

/// How far above the alphabetic baseline the central baseline of a font lies (the reference browser's
/// rule; `ui4me-layout` `writing_mode::central_offset`).
pub fn central_offset(ascent: f32, descent: f32) -> f32 {
    let (a, d) = (ascent.round(), descent.round());
    let h = (a + d) as i32;
    a - (h - h / 2) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_sets_cjk_upright_and_latin_sideways() {
        let m = GlyphOrientation::Mixed;
        assert!(m.upright('日'));
        assert!(m.upright('あ'));
        assert!(m.upright('。'));
        assert!(!m.upright('a'));
        assert!(!m.upright('1'));
        assert!(GlyphOrientation::Upright.upright('a'));
        assert!(!GlyphOrientation::Sideways.upright('日'));
    }
}
