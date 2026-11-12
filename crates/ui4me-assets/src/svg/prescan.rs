//! owner: 007-resources (T048) — the owned SVG pre-scan (plan, SVG pipeline; FR-037, FR-039, FR-041).
//!
//! usvg invents a 100x100 size when the root has neither `width`/`height` nor `viewBox`, and it
//! silently drops `<text>` when its `text` feature is off. Both would violate FR-037/FR-039 and
//! constitution III, so the pre-scan (roxmltree, the parser usvg itself uses) decides the natural
//! size and ratio, detects `<text>`, counts elements for the early exit and rejects gzip.
//! DTDs are not accepted (entity expansion is an attack surface and no supported SVG needs one).

use crate::decode::SvgNatural;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// What the pre-scan learned. `elements` counts every element of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Prescan {
    pub natural: SvgNatural,
    pub has_text: bool,
    pub elements: u32,
    /// The root says `preserveAspectRatio="none"`: the content is stretched to any box it is drawn into.
    pub stretch: bool,
    /// The root's `viewBox` has a zero width or height: rendering is disabled (SVG 2 section 8.2), the
    /// image draws nothing.
    pub disabled: bool,
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

/// An absolute CSS length in px; relative lengths (`%`, `em`, `ex`) and garbage are `None`.
pub fn absolute_length(s: &str) -> Option<f32> {
    let s = s.trim();
    // The number ends at the first character that cannot belong to it; `e`/`E` are exponent
    // markers only when a digit or sign follows ("em"/"ex" are units).
    let b = s.as_bytes();
    let cut = s
        .char_indices()
        .find(|&(i, c)| match c {
            '0'..='9' | '.' | '-' | '+' => false,
            'e' | 'E' => !b
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+'),
            _ => true,
        })
        .map_or(s.len(), |(i, _)| i);
    let (num, unit) = s.split_at(cut);
    let v: f32 = num.parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    let k = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        _ => return None,
    };
    Some(v * k)
}

fn view_box_ratio(s: &str) -> Option<(f32, f32)> {
    let v: Vec<f32> = s
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<f32>().ok())
        .collect::<Option<_>>()?;
    match v.as_slice() {
        [_, _, w, h] if *w > 0.0 && *h > 0.0 && w.is_finite() && h.is_finite() => Some((*w, *h)),
        _ => None,
    }
}

/// Natural dimensions from the root attributes; nothing is invented.
pub fn natural_of(width: Option<&str>, height: Option<&str>, view_box: Option<&str>) -> SvgNatural {
    let w = width.and_then(absolute_length).filter(|v| *v > 0.0);
    let h = height.and_then(absolute_length).filter(|v| *v > 0.0);
    let vb = view_box.and_then(view_box_ratio);
    match (w, h) {
        (Some(w), Some(h)) => SvgNatural {
            width: Some(w),
            height: Some(h),
            ratio: Some((w, h)),
        },
        (Some(w), None) => SvgNatural {
            width: Some(w),
            height: vb.map(|(a, b)| w * b / a),
            ratio: vb,
        },
        (None, Some(h)) => SvgNatural {
            width: vb.map(|(a, b)| h * a / b),
            height: Some(h),
            ratio: vb,
        },
        (None, None) => SvgNatural {
            width: None,
            height: None,
            ratio: vb,
        },
    }
}

/// Scan one document. `max_nodes` bounds the element count (`SIZE_LIMIT`).
pub fn scan(bytes: &[u8], max_nodes: u32) -> Result<Prescan, Status> {
    if crate::decode::sniff::is_gzip(bytes) {
        return Err(err(
            Code::UnsupportedMediaType,
            "gzip-compressed content (SVGZ) is unsupported",
        ));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| err(Code::DecodeFailed, "SVG is not UTF-8"))?;
    let opt = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: max_nodes.saturating_mul(2).max(16),
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, opt).map_err(|e| match e {
        roxmltree::Error::NodesLimitReached => err(
            Code::SizeLimit,
            format!("SVG exceeds the {max_nodes}-element limit"),
        ),
        roxmltree::Error::DtdDetected => err(Code::DecodeFailed, "SVG with a DTD is not supported"),
        other => err(
            Code::DecodeFailed,
            format!("SVG is not well-formed: {other}"),
        ),
    })?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NS) {
        return Err(err(
            Code::DecodeFailed,
            "the root element is not an SVG <svg>",
        ));
    }
    let mut elements = 0u32;
    let mut has_text = false;
    for n in doc.descendants().filter(|n| n.is_element()) {
        elements += 1;
        if elements > max_nodes {
            return Err(err(
                Code::SizeLimit,
                format!("SVG exceeds the {max_nodes}-element limit"),
            ));
        }
        if n.tag_name().name() == "text" && n.tag_name().namespace() == Some(SVG_NS) {
            has_text = true;
        }
    }
    let natural = natural_of(
        root.attribute("width"),
        root.attribute("height"),
        root.attribute("viewBox"),
    );
    let stretch = root
        .attribute("preserveAspectRatio")
        .is_some_and(|v| v.split_whitespace().find(|t| *t != "defer") == Some("none"));
    let disabled = root.attribute("viewBox").is_some_and(|s| {
        let v: Option<Vec<f32>> = s.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|p| !p.is_empty()).map(|p| p.parse::<f32>().ok()).collect();
        matches!(v.as_deref(), Some([_, _, w, h]) if *w >= 0.0 && *h >= 0.0 && (*w == 0.0 || *h == 0.0))
    });
    Ok(Prescan {
        natural,
        has_text,
        elements,
        stretch,
        disabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(absolute_length("40"), Some(40.0));
        assert_eq!(absolute_length("40px"), Some(40.0));
        assert_eq!(absolute_length("1in"), Some(96.0));
        assert_eq!(absolute_length("12pt"), Some(16.0));
        assert_eq!(absolute_length("50%"), None);
        assert_eq!(absolute_length("2em"), None);
        assert_eq!(absolute_length("1e1"), Some(10.0));
        assert_eq!(absolute_length("abc"), None);
    }

    #[test]
    fn natural_sizes_invent_nothing() {
        assert_eq!(
            natural_of(None, None, None),
            SvgNatural {
                width: None,
                height: None,
                ratio: None
            }
        );
        assert_eq!(
            natural_of(None, None, Some("0 0 20 10")).ratio,
            Some((20.0, 10.0))
        );
        let n = natural_of(Some("40"), Some("20"), Some("0 0 1 1"));
        assert_eq!(
            (n.width, n.height, n.ratio),
            (Some(40.0), Some(20.0), Some((40.0, 20.0)))
        );
        let n = natural_of(Some("50%"), Some("20"), Some("0 0 10 5"));
        assert_eq!(
            (n.width, n.height),
            (Some(40.0), Some(20.0)),
            "the missing side follows the ratio"
        );
        assert_eq!(
            natural_of(Some("0"), Some("-3"), None),
            SvgNatural {
                width: None,
                height: None,
                ratio: None
            }
        );
    }

    #[test]
    fn root_preserve_aspect_ratio_none_and_a_zero_view_box() {
        let svg = |attrs: &str| format!("<svg xmlns='http://www.w3.org/2000/svg' {attrs}/>");
        let scan = |a: &str| scan(svg(a).as_bytes(), 100).unwrap();
        assert!(scan("viewBox='0 0 4 4' preserveAspectRatio='none'").stretch);
        assert!(scan("viewBox='0 0 4 4' preserveAspectRatio='defer none slice'").stretch);
        assert!(!scan("viewBox='0 0 4 4' preserveAspectRatio='xMidYMid meet'").stretch);
        assert!(!scan("viewBox='0 0 4 4'").stretch);
        // A zero width or height disables rendering (SVG 2 section 8.2); a negative one is an error, ignored.
        assert!(scan("viewBox='0 0 0 8'").disabled);
        assert!(scan("viewBox='0 0 8 0'").disabled);
        assert!(!scan("viewBox='0 0 8 8'").disabled);
        assert!(!scan("viewBox='0 0 -1 8'").disabled);
        assert!(!scan("width='8'").disabled);
    }
}
