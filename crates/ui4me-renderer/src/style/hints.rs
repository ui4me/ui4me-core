//! owner: 006-style-engine — T034, T035
//!
//! Table-driven presentational hints (FR-010; research R-07; HTML Rendering 15.3, 15.4.3): the
//! legacy presentational attributes become declarations at the presentational-hint cascade
//! level (author origin, specificity zero, before every other author rule). The mapping is
//! computed per element from its attributes and parsed with the engine's declaration parser,
//! so values go through the same profile and parsing rules as any style.
//!
//! Covered (FR-010): `body` bgcolor/text; `table` width/height/border/cellspacing/cellpadding/
//! bgcolor/align; `tr`/`td`/`th`/`thead`/`tbody`/`tfoot` align/valign/bgcolor/width/height/nowrap;
//! `col`/`colgroup` width; `img` width/height (+ `aspect-ratio`); `svg`, `canvas`, `iframe`, `embed`,
//! `object` and `video` width/height, border, align, hspace, vspace;
//! `hr` align/width/size/color/noshade; `div`/`p`/`h1`-`h6`/`caption`/`center` align; `font`
//! color/face/size; `ol`/`ul`/`li` type; `pre` wrap; global `dir`. `ol` start/reversed and `li`
//! value are counter hints and belong to US7 (T082). Unlisted legacy attributes have no effect
//! and are reported once per document ([`unlisted_attribute`]).

use crate::style::adapter::handle::StyleNode;
use std::fmt::Write as _;

/// Attribute local names that can change a hint (restyle on change).
pub(crate) fn affects_hints(local: &str) -> bool {
    matches!(
        local,
        "bgcolor"
            | "text"
            | "width"
            | "height"
            | "border"
            | "cellspacing"
            | "cellpadding"
            | "align"
            | "valign"
            | "nowrap"
            | "hspace"
            | "vspace"
            | "size"
            | "color"
            | "noshade"
            | "face"
            | "type"
            | "wrap"
            | "dir"
            | "reversed"
            | "start"
            | "value"
            | "clear"
    )
}

/// Unlisted legacy attributes that carry a defined presentational meaning in the HTML Rendering
/// section but have no effect here: the report id (a registered `html.presentational-hint.*`
/// feature) per `(element, attribute)`.
pub(crate) fn unlisted_attribute(element: &str, attr: &str) -> Option<&'static str> {
    Some(match (element, attr) {
        ("body", "background")
        | ("table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th", "background") => {
            "html.presentational-hint.background"
        }
        ("body", "link") => "html.presentational-hint.link",
        ("body", "vlink") => "html.presentational-hint.vlink",
        ("body", "alink") => "html.presentational-hint.alink",
        ("body" | "iframe" | "frame", "marginwidth") => "html.presentational-hint.marginwidth",
        ("body" | "iframe" | "frame", "marginheight") => "html.presentational-hint.marginheight",
        ("body", "topmargin") => "html.presentational-hint.topmargin",
        ("body", "leftmargin") => "html.presentational-hint.leftmargin",
        ("table", "bordercolor") => "html.presentational-hint.bordercolor",
        ("table", "rules") => "html.presentational-hint.rules",
        ("table", "frame") => "html.presentational-hint.frame",
        ("iframe", "frameborder") => "html.presentational-hint.frameborder",
        ("iframe", "scrolling") => "html.presentational-hint.scrolling",
        ("embed" | "object", "hspace") => "html.presentational-hint.hspace",
        ("embed" | "object", "vspace") => "html.presentational-hint.vspace",
        ("embed" | "object" | "iframe" | "input", "align") => "html.presentational-hint.align",
        _ => return None,
    })
}

impl<'a> StyleNode<'a> {
    /// The element's local name ("" for a non-element).
    pub(crate) fn local(&self) -> &'a str {
        self.elem().map_or("", |e| e.local_name())
    }

    /// An HTML attribute value (null namespace).
    pub(crate) fn attr(&self, name: &str) -> Option<&'a str> {
        self.elem()?.attr(name)
    }

    /// The parent node when it is an element.
    pub(crate) fn parent_node_element(&self) -> Option<StyleNode<'a>> {
        let p = self.view().arena().node(self.id()).parent()?;
        let n = self.with(p);
        n.is_elem().then_some(n)
    }

    /// The parent when it is the HTML element `tag`.
    pub(crate) fn parent_html(&self, tag: &str) -> Option<StyleNode<'a>> {
        self.parent_node_element().filter(|p| p.local() == tag)
    }
}

// ------------------------------------------------------------------ attribute value parsers

/// HTML "rules for parsing non-negative integers" (None on error).
pub(crate) fn parse_non_negative_integer(s: &str) -> Option<u64> {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    let t = t.strip_prefix('+').unwrap_or(t);
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// HTML "rules for parsing integers" (None on error).
pub(crate) fn parse_integer(s: &str) -> Option<i64> {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    let (neg, rest) = match t.chars().next()? {
        '-' => (true, &t[1..]),
        '+' => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dim {
    Px(f64),
    Percent(f64),
}

/// HTML "rules for parsing dimension values" (None on error).
pub(crate) fn parse_dimension(s: &str) -> Option<Dim> {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    let t = t.strip_prefix('+').unwrap_or(t);
    let b = t.as_bytes();
    if b.first().is_none_or(|c| !c.is_ascii_digit()) {
        return None;
    }
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut end = i;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        end = j;
    }
    let v: f64 = t[..end].parse().ok()?;
    if b.get(end) == Some(&b'%') {
        Some(Dim::Percent(v))
    } else {
        Some(Dim::Px(v))
    }
}

fn dim_css(d: Dim) -> String {
    match d {
        Dim::Px(v) => format!("{}px", trim_num(v)),
        Dim::Percent(v) => format!("{}%", trim_num(v)),
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v}");
    s
}

/// HTML "rules for parsing a legacy colour value" (None on failure); the result is an
/// `rgb(r, g, b)` function text.
pub fn parse_legacy_color(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let mut s = input
        .trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'))
        .to_owned();
    if s.is_empty() || s.eq_ignore_ascii_case("transparent") {
        return None;
    }
    if let Ok((r, g, b)) = cssparser::color::parse_named_color(&s.to_ascii_lowercase()) {
        return Some(format!("rgb({r}, {g}, {b})"));
    }
    let hex = |c: char| c.is_ascii_hexdigit();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() == 4 && chars[0] == '#' && chars[1..].iter().all(|&c| hex(c)) {
        let v = |c: char| c.to_digit(16).unwrap_or(0) * 17;
        return Some(format!(
            "rgb({}, {}, {})",
            v(chars[1]),
            v(chars[2]),
            v(chars[3])
        ));
    }
    // Replace any code point above U+FFFF with "00", then the legacy digit rules.
    let mut t: Vec<char> = Vec::new();
    for c in s.drain(..) {
        if (c as u32) > 0xFFFF {
            t.extend(['0', '0']);
        } else {
            t.push(c);
        }
    }
    if t.len() > 128 {
        t.truncate(128);
    }
    if t.first() == Some(&'#') {
        t.remove(0);
    }
    let mut t: Vec<char> = t
        .into_iter()
        .map(|c| if hex(c) { c } else { '0' })
        .collect();
    while t.is_empty() || !t.len().is_multiple_of(3) {
        t.push('0');
    }
    let len = t.len() / 3;
    let mut comps: Vec<Vec<char>> = (0..3).map(|i| t[i * len..(i + 1) * len].to_vec()).collect();
    let mut len = len;
    if len > 8 {
        for c in &mut comps {
            *c = c[len - 8..].to_vec();
        }
        len = 8;
    }
    while len > 2 && comps.iter().all(|c| c[0] == '0') {
        for c in &mut comps {
            c.remove(0);
        }
        len -= 1;
    }
    if len > 2 {
        for c in &mut comps {
            c.truncate(2);
        }
    }
    let val = |c: &Vec<char>| u32::from_str_radix(&c.iter().collect::<String>(), 16).unwrap_or(0);
    Some(format!(
        "rgb({}, {}, {})",
        val(&comps[0]),
        val(&comps[1]),
        val(&comps[2])
    ))
}

/// HTML "rules for parsing a legacy font size" -> a `font-size` keyword.
pub(crate) fn parse_legacy_font_size(input: &str) -> Option<&'static str> {
    let t = input.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    if t.is_empty() {
        return None;
    }
    let (mode, rest) = match t.chars().next()? {
        '+' => (1, &t[1..]),
        '-' => (-1, &t[1..]),
        _ => (0, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = digits.parse().ok()?;
    if mode == 1 {
        value += 3;
    } else if mode == -1 {
        value = 3 - value;
    }
    Some(match value.clamp(1, 7) {
        1 => "x-small",
        2 => "small",
        3 => "medium",
        4 => "large",
        5 => "x-large",
        6 => "xx-large",
        _ => "xxx-large",
    })
}

// ------------------------------------------------------------------ the table

/// The `content` of the document's first `<meta name=color-scheme>` child of `<head>`, with commas
/// treated as separators.
fn meta_color_scheme(doc: &ui4me_document::document::Document) -> Option<String> {
    let head = doc.head()?;
    for &c in doc.children(head).ok()? {
        let Some(el) = doc.arena().node(c).as_element() else {
            continue;
        };
        if !el.is_html_named("meta")
            || !el
                .attr("name")
                .is_some_and(|n| n.eq_ignore_ascii_case("color-scheme"))
        {
            continue;
        }
        let v = el.attr("content")?.replace(',', " ");
        let v = v.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        return (!v.is_empty()).then_some(v);
    }
    None
}

struct Decls(String);

impl Decls {
    fn add(&mut self, prop: &str, value: &str) {
        let _ = write!(self.0, "{prop}: {value}; ");
    }
}

fn dim_attr(e: &StyleNode<'_>, name: &str, ignore_zero: bool) -> Option<String> {
    let d = parse_dimension(e.attr(name)?)?;
    if ignore_zero && matches!(d, Dim::Px(v) | Dim::Percent(v) if v == 0.0) {
        return None;
    }
    Some(dim_css(d))
}

fn px_attr(e: &StyleNode<'_>, name: &str) -> Option<String> {
    Some(format!("{}px", parse_non_negative_integer(e.attr(name)?)?))
}

/// The `text-align` value an `align` attribute of a block or table part maps to (reference
/// browser, FR-013): `center`/`middle` -> `-webkit-center`, `left` -> `-webkit-left`, `right` ->
/// `-webkit-right` (the legacy keywords that also align block-level children), `justify` as is.
fn legacy_text_align(v: Option<&str>) -> Option<&'static str> {
    let v = v?;
    Some(match () {
        _ if v.eq_ignore_ascii_case("center") || v.eq_ignore_ascii_case("middle") => {
            "-webkit-center"
        }
        _ if v.eq_ignore_ascii_case("left") => "-webkit-left",
        _ if v.eq_ignore_ascii_case("right") => "-webkit-right",
        _ if v.eq_ignore_ascii_case("justify") => "justify",
        _ => return None,
    })
}

fn is_ci(v: Option<&str>, s: &str) -> bool {
    v.is_some_and(|v| v.eq_ignore_ascii_case(s))
}

fn table_of<'a>(e: &StyleNode<'a>) -> Option<StyleNode<'a>> {
    // td/th > tr > (thead|tbody|tfoot)? > table
    let tr = e.parent_html("tr")?;
    let p = tr.parent_node_element()?;
    if p.local() == "table" {
        return Some(p);
    }
    if matches!(p.local(), "thead" | "tbody" | "tfoot") {
        let t = p.parent_node_element()?;
        if t.local() == "table" {
            return Some(t);
        }
    }
    None
}

/// The presentational-hint declarations of an element as CSS text (`None` when it has none).
pub(crate) fn declarations(e: &StyleNode<'_>, quirks_mode: bool) -> Option<String> {
    let name = e.local();
    let mut d = Decls(String::new());

    if let Some(dir) = e.attr("dir") {
        if dir.eq_ignore_ascii_case("ltr") {
            d.add("direction", "ltr");
        } else if dir.eq_ignore_ascii_case("rtl") {
            d.add("direction", "rtl");
        }
    }

    match name {
        // `<meta name=color-scheme content="...">` in the head (CSS Color Adjust 1 section 2.2): the first
        // one in tree order acts as a presentational hint for the root's `color-scheme`; an invalid value
        // is dropped by the declaration parser.
        "html" if e.view().document_element() == Some(e.id()) => {
            if let Some(v) = meta_color_scheme(e.view()) {
                d.add("color-scheme", &v);
            }
        }
        "body" => {
            if let Some(c) = e.attr("bgcolor").and_then(parse_legacy_color) {
                d.add("background-color", &c);
            }
            if let Some(c) = e.attr("text").and_then(parse_legacy_color) {
                d.add("color", &c);
            }
        }
        "table" => {
            if let Some(w) = dim_attr(e, "width", true) {
                d.add("width", &w);
            }
            if let Some(h) = dim_attr(e, "height", false) {
                d.add("height", &h);
            }
            if let Some(b) = e.attr("border") {
                let n = parse_non_negative_integer(b);
                let w = n.unwrap_or(1);
                for side in ["top", "right", "bottom", "left"] {
                    d.add(&format!("border-{side}-width"), &format!("{w}px"));
                }
                if n != Some(0) {
                    d.add("border-style", "outset");
                }
            }
            if let Some(s) = px_attr(e, "cellspacing") {
                d.add("border-spacing", &s);
            }
            if let Some(c) = e.attr("bgcolor").and_then(parse_legacy_color) {
                d.add("background-color", &c);
            }
            match e.attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("left") => d.add("float", "left"),
                Some("right") => d.add("float", "right"),
                Some("center") => d.add("margin-inline", "auto"),
                _ => {}
            }
        }
        "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" => {
            let cell = matches!(name, "td" | "th");
            if let Some(v) = legacy_text_align(e.attr("align")) {
                d.add("text-align", v);
            }
            if let Some(v @ ("top" | "middle" | "bottom" | "baseline")) =
                e.attr("valign").map(str::to_ascii_lowercase).as_deref()
            {
                d.add("vertical-align", v);
            }
            if let Some(c) = e.attr("bgcolor").and_then(parse_legacy_color) {
                d.add("background-color", &c);
            }
            if cell && let Some(w) = dim_attr(e, "width", true) {
                d.add("width", &w);
            }
            if let Some(h) = dim_attr(e, "height", cell) {
                d.add("height", &h);
            }
            if cell && e.attr("nowrap").is_some() {
                // Quirks mode: nowrap with a length width does not apply (rendering quirk).
                let length_width = e
                    .attr("width")
                    .and_then(parse_dimension)
                    .is_some_and(|w| matches!(w, Dim::Px(v) if v != 0.0));
                d.add(
                    "white-space",
                    if quirks_mode && length_width {
                        "normal"
                    } else {
                        "nowrap"
                    },
                );
            }
            if cell && let Some(table) = table_of(e) {
                if let Some(b) = table.attr("border") {
                    let n = parse_non_negative_integer(b);
                    if n != Some(0) {
                        d.add("border-width", "1px");
                        d.add("border-style", "inset");
                    }
                }
                if let Some(p) = px_attr(&table, "cellpadding") {
                    d.add("padding", &p);
                }
            }
        }
        "col" | "colgroup" => {
            if let Some(w) = dim_attr(e, "width", false) {
                d.add("width", &w);
            }
        }
        "img" => {
            let w = dim_attr(e, "width", false);
            let h = dim_attr(e, "height", false);
            if let Some(w) = &w {
                d.add("width", w);
            }
            if let Some(h) = &h {
                d.add("height", h);
            }
            if let (Some(Dim::Px(wv)), Some(Dim::Px(hv))) = (
                e.attr("width").and_then(parse_dimension),
                e.attr("height").and_then(parse_dimension),
            ) && wv > 0.0
                && hv > 0.0
            {
                d.add(
                    "aspect-ratio",
                    &format!("auto {} / {}", trim_num(wv), trim_num(hv)),
                );
            }
            if let Some(b) = e.attr("border").and_then(parse_non_negative_integer)
                && b > 0
            {
                for side in ["top", "right", "bottom", "left"] {
                    d.add(&format!("border-{side}-width"), &format!("{b}px"));
                    d.add(&format!("border-{side}-style"), "solid");
                }
            }
            match e.attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("left") => d.add("float", "left"),
                Some("right") => d.add("float", "right"),
                Some("top") => d.add("vertical-align", "top"),
                Some("baseline") => d.add("vertical-align", "baseline"),
                Some("texttop") => d.add("vertical-align", "text-top"),
                Some("absmiddle" | "abscenter" | "center" | "middle") => {
                    d.add("vertical-align", "middle")
                }
                Some("bottom") => d.add("vertical-align", "bottom"),
                _ => {}
            }
            if let Some(h) = dim_attr(e, "hspace", false) {
                d.add("margin-left", &h);
                d.add("margin-right", &h);
            }
            if let Some(v) = dim_attr(e, "vspace", false) {
                d.add("margin-top", &v);
                d.add("margin-bottom", &v);
            }
        }
        // HTML Rendering 15.5 (iframe, embed, object, video) and 15.4.2 (canvas), SVG 2 (the `width`
        // and `height` attributes of the outermost `svg` element are presentation attributes): the
        // dimension attributes are the element's width and height.
        "svg" | "canvas" | "iframe" | "embed" | "object" | "video" => {
            let w = dim_attr(e, "width", false);
            let h = dim_attr(e, "height", false);
            if let Some(w) = &w {
                d.add("width", w);
            }
            if let Some(h) = &h {
                d.add("height", h);
            }
            if let (Some(Dim::Px(wv)), Some(Dim::Px(hv))) = (
                e.attr("width").and_then(parse_dimension),
                e.attr("height").and_then(parse_dimension),
            ) && wv > 0.0
                && hv > 0.0
                && matches!(name, "svg" | "canvas")
            {
                d.add(
                    "aspect-ratio",
                    &format!("auto {} / {}", trim_num(wv), trim_num(hv)),
                );
            }
        }
        // HTML Rendering section 15.3 (`br[clear]`, flow content): `left` / `right` clear that side, `all` and `both` both (015 STAB-1).
        "br" => match e.attr("clear").map(str::to_ascii_lowercase).as_deref() {
            Some("left") => d.add("clear", "left"),
            Some("right") => d.add("clear", "right"),
            Some("all" | "both") => d.add("clear", "both"),
            _ => {}
        },
        "hr" => {
            // HTML 15.4.3 `align`: the horizontal margins only (the vertical ones stay the UA's).
            match e.attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("left") => {
                    d.add("margin-left", "0");
                    d.add("margin-right", "auto");
                }
                Some("right") => {
                    d.add("margin-left", "auto");
                    d.add("margin-right", "0");
                }
                Some("center") => {
                    d.add("margin-left", "auto");
                    d.add("margin-right", "auto");
                }
                _ => {}
            }
            // `color` and `noshade` draw the rule as a solid bar: border style `solid` plus the
            // border and background colour. The text `color` property stays the UA's gray; `noshade`
            // without `color` uses gray (D-LSTYFIXr7-01: reference browser, T2 hints-img-hr-font).
            if e.attr("color").is_some() || e.attr("noshade").is_some() {
                d.add("border-style", "solid");
            }
            if let Some(c) = e.attr("color").and_then(parse_legacy_color) {
                d.add("border-color", &c);
                d.add("background-color", &c);
            } else if e.attr("noshade").is_some() && e.attr("color").is_none() {
                d.add("border-color", "rgb(128, 128, 128)");
                d.add("background-color", "rgb(128, 128, 128)");
            }
            // `size`: one pixel or less drops the bottom border, otherwise it is the height minus
            // the two border pixels; the border width stays the UA's 1px in every case.
            if let Some(size) = e.attr("size").and_then(parse_non_negative_integer) {
                if size <= 1 {
                    d.add("border-bottom-width", "0");
                } else {
                    d.add("height", &format!("{}px", size - 2));
                }
            }
            if let Some(w) = dim_attr(e, "width", false) {
                d.add("width", &w);
            }
        }
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "caption" | "center" | "legend" => {
            let align = e.attr("align").map(str::to_ascii_lowercase);
            if name == "center" {
                d.add("text-align", "-webkit-center");
            } else if name == "caption" && align.as_deref() == Some("bottom") {
                d.add("caption-side", "bottom");
            } else if name == "legend" {
                // The reference browser maps a legend's `align` to `text-align` too (015 HTML-3); the
                // placement over the fieldset border reads the attribute itself.
                if let Some(v @ ("left" | "right" | "center" | "justify")) = align.as_deref() {
                    d.add("text-align", v);
                }
            } else if name != "caption"
                && let Some(v) = legacy_text_align(align.as_deref())
            {
                d.add("text-align", v);
            }
        }
        "font" => {
            if let Some(c) = e.attr("color").and_then(parse_legacy_color) {
                d.add("color", &c);
            }
            if let Some(f) = e.attr("face").filter(|f| !f.trim().is_empty()) {
                // The attribute value is a font-family list; unusable text is dropped by the parser.
                d.add("font-family", f.trim());
            }
            if let Some(s) = e.attr("size").and_then(parse_legacy_font_size) {
                d.add("font-size", s);
            }
        }
        "ol" | "ul" | "li" => {
            if name == "ol" && (e.attr("start").is_some() || e.attr("reversed").is_some()) {
                // HTML "ol start / reversed" -> counter-reset (the algorithm of 15.3.8).
                let start = e.attr("start").and_then(parse_integer);
                if e.attr("reversed").is_some() {
                    match start {
                        Some(v) => {
                            d.add("counter-reset", &format!("reversed(list-item) {}", v + 1))
                        }
                        None => d.add("counter-reset", "reversed(list-item)"),
                    }
                } else if let Some(v) = start {
                    d.add("counter-reset", &format!("list-item {}", v - 1));
                }
            }
            if name == "li"
                && let Some(v) = e.attr("value").and_then(parse_integer)
            {
                // M15: `counter-set` is engine-unavailable (profile), so the ordinal value is set
                // with a counter-reset one below the value; the implicit list-item increment then
                // lands on `value` and following siblings continue from it.
                d.add("counter-reset", &format!("list-item {}", v - 1));
            }
            let v = e.attr("type");
            let ty = match name {
                "ol" | "li" => match v {
                    Some("1") => Some("decimal"),
                    Some("a") => Some("lower-alpha"),
                    Some("A") => Some("upper-alpha"),
                    Some("i") => Some("lower-roman"),
                    Some("I") => Some("upper-roman"),
                    _ => None,
                },
                _ => None,
            };
            let ty = ty.or_else(|| {
                if name == "ol" {
                    return None;
                }
                ["none", "disc", "circle", "square"]
                    .into_iter()
                    .find(|t| is_ci(v, t))
            });
            if let Some(t) = ty {
                d.add("list-style-type", t);
            }
        }
        "pre" if e.attr("wrap").is_some() => d.add("white-space", "pre-wrap"),
        _ => {}
    }

    if d.0.is_empty() { None } else { Some(d.0) }
}
