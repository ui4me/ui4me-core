//! owner: 008-layout-engine (L-INL-r2) — embedded content (FR-074; T086).
//!
//! `canvas`, `video`, `audio` (with `controls`), `iframe`, `embed` and `object` become replaced
//! boxes without natural dimensions: their CSS size, else 300×150. Each is reported as
//! `html.embedded.<name>` (stage `box`); inline `svg` is reported by 007, not here.

use ui4me_engine_types::diag::FeatureId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Embedded {
    pub name: &'static str,
    /// Intrinsic ratio (width / height) of inline `svg` from its `viewBox`.
    pub ratio: Option<f32>,
}

/// The embedded-content element names of FR-074.
pub fn classify(name: &str, has_controls: bool) -> Option<Embedded> {
    let name = match name {
        "canvas" => "canvas",
        "video" => "video",
        "audio" if has_controls => "audio",
        "iframe" => "iframe",
        "embed" => "embed",
        "object" => "object",
        "svg" => "svg",
        _ => return None,
    };
    Some(Embedded { name, ratio: None })
}

/// The ratio of a `viewBox="min-x min-y width height"` attribute.
pub fn view_box_ratio(view_box: &str) -> Option<f32> {
    let n: Vec<f32> = view_box
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    match n.as_slice() {
        [_, _, w, h] if *w > 0.0 && *h > 0.0 => Some(w / h),
        _ => None,
    }
}

impl Embedded {
    /// The `UNSUPPORTED_FEATURE` report of the element; inline `svg` is reported by 007 instead.
    pub fn feature(self) -> Option<FeatureId> {
        let id = match self.name {
            "svg" => return None,
            "canvas" => "html.embedded.canvas",
            "video" => "html.embedded.video",
            "audio" => "html.embedded.audio",
            "iframe" => "html.embedded.iframe",
            "embed" => "html.embedded.embed",
            _ => "html.embedded.object",
        };
        Some(FeatureId::new(id).expect("html is a registered root"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_without_controls_is_not_embedded_content() {
        assert!(classify("audio", false).is_none());
        assert_eq!(
            classify("audio", true).unwrap().feature().unwrap().as_str(),
            "html.embedded.audio"
        );
        assert_eq!(
            classify("canvas", false)
                .unwrap()
                .feature()
                .unwrap()
                .as_str(),
            "html.embedded.canvas"
        );
        assert!(classify("div", false).is_none());
        // Inline svg is a replaced box too, but 007 reports it.
        assert!(classify("svg", false).unwrap().feature().is_none());
        assert_eq!(view_box_ratio("0 0 20 10"), Some(2.0));
        assert_eq!(view_box_ratio("0,0,5,5"), Some(1.0));
        assert_eq!(view_box_ratio("0 0 0 5"), None);
    }
}
