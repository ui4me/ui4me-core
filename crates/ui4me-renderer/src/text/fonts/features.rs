//! owner: 009-text-inline (015 TXT-2)
//!
//! The OpenType settings of a span: `font-variation-settings`, `font-optical-sizing` and the
//! features `font-kerning`, `font-variant-*` and `font-feature-settings` ask for. The style
//! adapter resolves the keywords to tags (`ui4me-style` `FontOpenType`); here they become Parley
//! style properties.

use parley::setting::{FontFeature, FontVariation, Tag};
use parley::{FontFeatures, FontVariations, StyleProperty};
use std::borrow::Cow;
use std::sync::Arc;

use crate::text::paragraph::source::SpanBrush;

/// An OpenType tag as its four bytes.
pub type FontTag = [u8; 4];

/// Per-span OpenType settings (the initial value changes nothing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontSettings {
    /// `font-variation-settings`: axis tag and user-space value, in declaration order.
    pub variations: Arc<[(FontTag, f32)]>,
    /// `font-optical-sizing: none` (the initial `auto` sets the `opsz` axis to the font size in
    /// CSS px, for fonts that have one).
    pub optical_sizing_none: bool,
    /// Features in application order; a later entry for a tag wins.
    pub features: Arc<[(FontTag, u32)]>,
    /// `font-synthesis-weight: none` (no faux bold) and `font-synthesis-style: none` (no faux oblique).
    pub no_synthesis_weight: bool,
    pub no_synthesis_style: bool,
    /// `font-synthesis-small-caps: none` and `font-synthesis-position: none`: the reference browser
    /// does not synthesize, so a missing feature is no gap.
    pub no_synthesis_small_caps: bool,
    pub no_synthesis_position: bool,
}

impl FontSettings {
    /// CSS Fonts §7.2: face defaults precede element properties; letter spacing
    /// disables optional ligatures after face defaults and before explicit properties.
    pub(crate) fn with_face_defaults(
        &self,
        face: &ui4me_assets::fonts::FaceSettings,
        letter_spacing: f32,
    ) -> Self {
        let mut features = face.features.clone();
        if letter_spacing != 0.0 {
            features.extend([*b"liga", *b"clig", *b"dlig", *b"hlig"].map(|tag| (tag, 0)));
        }
        features.extend(self.features.iter().copied());
        let mut variations = face.variations.clone();
        variations.extend(self.variations.iter().copied());
        Self {
            features: features.into(),
            variations: variations.into(),
            ..self.clone()
        }
    }

    /// Whether nothing differs from the initial values.
    pub fn is_initial(&self) -> bool {
        self.variations.is_empty()
            && !self.optical_sizing_none
            && self.features.is_empty()
            && !self.no_synthesis_weight
            && !self.no_synthesis_style
            && !self.no_synthesis_small_caps
            && !self.no_synthesis_position
    }
}

/// Parley properties of `settings` for text of `font_size` px. `opsz_available` says whether any
/// font the environment can use has an `opsz` axis: only then `font-optical-sizing: auto` adds the
/// axis (so that a page without such a font shapes exactly as before).
pub(crate) fn style_properties<'a>(
    settings: &FontSettings,
    font_size: f32,
    opsz_available: bool,
    slnt: Option<f32>,
) -> Vec<StyleProperty<'a, SpanBrush>> {
    let mut out = Vec::new();
    // The axes the style implies come first; `font-variation-settings` after them wins (CSS Fonts 4
    // section 7: the low-level setting overrides the high-level properties). A repeated axis keeps its
    // last value.
    let mut all: Vec<(FontTag, f32)> = Vec::new();
    if opsz_available && !settings.optical_sizing_none && font_size > 0.0 {
        all.push((*b"opsz", font_size));
    }
    if let Some(a) = slnt {
        all.push((*b"slnt", a));
    }
    all.extend(settings.variations.iter().copied());
    let mut vars: Vec<FontVariation> = Vec::new();
    for (i, (t, v)) in all.iter().enumerate() {
        if all[i + 1..].iter().any(|(t2, _)| t2 == t) {
            continue;
        }
        vars.push(FontVariation::new(Tag::new(t), *v));
    }
    if !vars.is_empty() {
        out.push(StyleProperty::FontVariations(FontVariations::List(
            Cow::Owned(vars),
        )));
    }
    if !settings.features.is_empty() {
        let feats: Vec<FontFeature> = settings
            .features
            .iter()
            .map(|(t, v)| FontFeature::new(Tag::new(t), (*v).min(u32::from(u16::MAX)) as u16))
            .collect();
        out.push(StyleProperty::FontFeatures(FontFeatures::List(Cow::Owned(
            feats,
        ))));
    }
    out
}

/// The axis tags of a font file's `fvar` table (first face of a collection).
pub(crate) fn font_axes(data: &[u8]) -> Vec<FontTag> {
    let Some(fvar) = crate::text::metrics::sfnt_table(data, 0, b"fvar") else {
        return Vec::new();
    };
    let be16 = |o: usize| {
        fvar.get(o..o + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let (Some(off), Some(count), Some(size)) = (be16(4), be16(8), be16(10)) else {
        return Vec::new();
    };
    (0..count)
        .filter_map(|i| {
            fvar.get(off + i * size..off + i * size + 4)
                .map(|t| [t[0], t[1], t[2], t[3]])
        })
        .collect()
}

/// The features the reference browser synthesizes when the font lacks them: small capitals
/// (`smcp`, `c2sc`, `pcap`, `c2pc`, `unic`) and super- and subscripts (`sups`, `subs`). This engine
/// does not synthesize them: a request the font cannot meet is reported.
pub const SYNTHESIZED_TAGS: &[FontTag] = &[
    *b"smcp", *b"c2sc", *b"pcap", *b"c2pc", *b"unic", *b"subs", *b"sups",
];

/// The feature tags of a font's `GSUB` table (face `index` of a collection).
pub(crate) fn gsub_features(data: &[u8], index: u32) -> Vec<FontTag> {
    let Some(gsub) = crate::text::metrics::sfnt_table(data, index, b"GSUB") else {
        return Vec::new();
    };
    let be16 = |o: usize| {
        gsub.get(o..o + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let Some(list) = be16(6) else {
        return Vec::new();
    };
    let Some(count) = be16(list) else {
        return Vec::new();
    };
    let mut out: Vec<FontTag> = (0..count)
        .filter_map(|i| {
            gsub.get(list + 2 + 6 * i..list + 6 + 6 * i)
                .map(|t| [t[0], t[1], t[2], t[3]])
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(vars: &[(&FontTag, f32)], feats: &[(&FontTag, u32)]) -> FontSettings {
        FontSettings {
            variations: vars.iter().map(|(t, v)| (**t, *v)).collect(),
            optical_sizing_none: false,
            features: feats.iter().map(|(t, v)| (**t, *v)).collect(),
            ..FontSettings::default()
        }
    }

    #[test]
    fn initial_settings_make_no_property() {
        assert!(FontSettings::default().is_initial());
        assert!(style_properties(&FontSettings::default(), 16.0, false, None).is_empty());
        // `auto` optical sizing adds the axis only when a font with the axis exists
        assert_eq!(
            style_properties(&FontSettings::default(), 16.0, true, None).len(),
            1
        );
    }

    #[test]
    fn explicit_opsz_replaces_auto_and_last_value_of_an_axis_wins() {
        let s = settings(&[(b"opsz", 30.0), (b"wght", 300.0), (b"wght", 700.0)], &[]);
        let props = style_properties(&s, 16.0, true, None);
        let [StyleProperty::FontVariations(FontVariations::List(list))] = props.as_slice() else {
            panic!("one variations property")
        };
        let got: Vec<_> = list.iter().map(|v| (v.tag.to_bytes(), v.value)).collect();
        assert_eq!(got, vec![(*b"opsz", 30.0), (*b"wght", 700.0)]);
    }

    #[test]
    fn style_slant_comes_before_the_declared_settings() {
        let s = settings(&[(b"slnt", -3.0)], &[]);
        let props = style_properties(&s, 16.0, false, Some(-10.0));
        let [StyleProperty::FontVariations(FontVariations::List(list))] = props.as_slice() else {
            panic!("one variations property")
        };
        assert_eq!(
            list.iter()
                .map(|v| (v.tag.to_bytes(), v.value))
                .collect::<Vec<_>>(),
            vec![(*b"slnt", -3.0)]
        );
        let props = style_properties(&FontSettings::default(), 16.0, false, Some(-10.0));
        let [StyleProperty::FontVariations(FontVariations::List(list))] = props.as_slice() else {
            panic!("one variations property")
        };
        assert_eq!(
            list.iter()
                .map(|v| (v.tag.to_bytes(), v.value))
                .collect::<Vec<_>>(),
            vec![(*b"slnt", -10.0)]
        );
    }

    #[test]
    fn optical_sizing_none_drops_the_auto_axis() {
        let s = FontSettings {
            optical_sizing_none: true,
            ..FontSettings::default()
        };
        assert!(style_properties(&s, 16.0, true, None).is_empty());
    }

    #[test]
    fn features_pass_in_order() {
        let s = settings(&[], &[(b"kern", 0), (b"smcp", 1)]);
        let props = style_properties(&s, 16.0, false, None);
        let [StyleProperty::FontFeatures(FontFeatures::List(list))] = props.as_slice() else {
            panic!("one features property")
        };
        assert_eq!(
            list.iter()
                .map(|f| (f.tag.to_bytes(), f.value))
                .collect::<Vec<_>>(),
            vec![(*b"kern", 0), (*b"smcp", 1)]
        );
    }
}
