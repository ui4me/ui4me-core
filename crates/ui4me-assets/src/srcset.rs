//! owner: 007-resources — lane L-PNT-r8 (resources-images `srcset` / `srcset-width`)
//!
//! Image source selection of an `<img>` element (HTML Standard "parse a srcset attribute",
//! "parse a sizes attribute", "select an image source from a source set"): density (`x`) and width
//! (`w`) descriptors, `sizes` with media conditions, and the document's device pixel ratio. Pure
//! functions over the attribute text and the environment; the element's `src` is the fallback.
//!
//! Supported in `sizes`: lengths in `px`, `em`/`rem` (16 px, the initial font size), `vw`, `vh`,
//! `vmin`, `vmax`, `cm`, `mm`, `in`, `pt`, `pc`, `q`, unitless `0` and `calc()` over those; media
//! conditions over `width`, `height`, `aspect-ratio`, `orientation`, `resolution` and
//! `prefers-color-scheme` (`light`), combined with `and`, `or`, `not` and parentheses. A feature or
//! value the evaluator does not know makes the condition false, and an entry whose length does not
//! parse is dropped (the HTML parsing rules).

/// The environment a selection depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceEnv {
    /// Viewport size in CSS px.
    pub viewport: (f32, f32),
    /// The document's device pixel ratio.
    pub dpr: f32,
}

/// One parsed `srcset` image candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub url: String,
    /// `w` descriptor.
    pub width: Option<u32>,
    /// `x` descriptor.
    pub density: Option<f32>,
}

/// The selected source: its URL and the density the image is to be treated at (its natural size is
/// the pixel size divided by it).
#[derive(Clone, Debug, PartialEq)]
pub struct Selected {
    pub url: String,
    pub density: f32,
}

/// HTML "parse a srcset attribute": the candidates with valid descriptors, in order.
pub fn parse_srcset_candidates(srcset: &str) -> Vec<Candidate> {
    let b = srcset.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut url = &srcset[start..i];
        let mut descriptors = "";
        let stripped = url.trim_end_matches(',');
        if stripped.len() != url.len() {
            url = stripped;
        } else {
            let dstart = i;
            let mut depth = 0;
            while i < b.len() && !(b[i] == b',' && depth == 0) {
                match b[i] {
                    b'(' => depth += 1,
                    b')' if depth > 0 => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            descriptors = &srcset[dstart..i];
        }
        if url.is_empty() {
            continue;
        }
        let (mut width, mut density, mut error) = (None, None, false);
        for d in descriptors.split_whitespace() {
            if let Some(n) = d.strip_suffix('w') {
                match n.parse::<u32>() {
                    Ok(v)
                        if v > 0
                            && width.is_none()
                            && density.is_none()
                            && n.bytes().all(|c| c.is_ascii_digit()) =>
                    {
                        width = Some(v)
                    }
                    _ => error = true,
                }
            } else if let Some(n) = d.strip_suffix('x') {
                match n.parse::<f32>() {
                    Ok(v) if v >= 0.0 && v.is_finite() && density.is_none() && width.is_none() => {
                        density = Some(v)
                    }
                    _ => error = true,
                }
            } else {
                error = true;
            }
        }
        if !error {
            out.push(Candidate {
                url: url.to_string(),
                width,
                density,
            });
        }
    }
    out
}

// ------------------------------------------------------------------ sizes

/// HTML "parse a sizes attribute": the used source size in CSS px (`100vw` when no entry applies).
pub fn source_size(sizes: Option<&str>, env: &SourceEnv) -> f32 {
    let default = env.viewport.0;
    let Some(sizes) = sizes else { return default };
    for entry in split_top_level(sizes, ',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (cond, len) = split_size_entry(entry);
        let Some(len) = len else { continue };
        let Some(px) = eval_length(len, env) else {
            continue;
        };
        if px < 0.0 || !px.is_finite() {
            continue;
        }
        if cond.trim().is_empty() || eval_media(cond.trim(), env) {
            return px;
        }
    }
    default
}

/// Split at `sep` outside parentheses.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// `<media-condition>? <source-size-value>`: the value is the last component (a token or a
/// function call).
fn split_size_entry(entry: &str) -> (&str, Option<&str>) {
    let e = entry.trim_end();
    if e.ends_with(')') {
        // The matching `(` and the function name in front of it.
        let mut depth = 0i32;
        for (i, c) in e.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        let name_start = e[..i]
                            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                            .map(|p| p + 1)
                            .unwrap_or(0);
                        // A bare parenthesised group with no function name is a media condition,
                        // not a length.
                        if name_start == i {
                            return (e, None);
                        }
                        return (&e[..name_start], Some(&e[name_start..]));
                    }
                }
                _ => {}
            }
        }
        (e, None)
    } else {
        match e.rfind(|c: char| c.is_ascii_whitespace()) {
            Some(p) => (&e[..p], Some(&e[p + 1..])),
            None => ("", Some(e)),
        }
    }
}

fn unit_px(unit: &str, env: &SourceEnv) -> Option<f32> {
    let (vw, vh) = (env.viewport.0 / 100.0, env.viewport.1 / 100.0);
    Some(match unit.to_ascii_lowercase().as_str() {
        "px" => 1.0,
        "em" | "rem" => 16.0,
        "vw" => vw,
        "vh" => vh,
        "vmin" => vw.min(vh),
        "vmax" => vw.max(vh),
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "in" => 96.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    })
}

/// A `<length>` or `calc()` of lengths, in px. Percentages are not valid in `sizes`.
fn eval_length(s: &str, env: &SourceEnv) -> Option<f32> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("calc(").and_then(|r| r.strip_suffix(')')) {
        let mut p = Calc {
            s: inner.as_bytes(),
            i: 0,
            env,
        };
        let v = p.sum()?;
        p.ws();
        return (p.i == p.s.len() && v.is_finite()).then_some(v.px);
    }
    let (num, unit) = split_number(s)?;
    if unit.is_empty() {
        return (num == 0.0).then_some(0.0);
    }
    Some(num * unit_px(unit, env)?)
}

fn split_number(s: &str) -> Option<(f32, &str)> {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E')))
        .unwrap_or(s.len());
    // `e` may start a unit (`em`): back off when the exponent is not followed by a digit.
    let mut end = end;
    while end > 0 {
        if let Ok(v) = s[..end].parse::<f32>() {
            let rest = &s[end..];
            if rest.chars().all(|c| c.is_ascii_alphabetic()) {
                return Some((v, rest));
            }
        }
        end -= 1;
    }
    None
}

/// A calc value: a length in px, or a bare number (`is_number`).
#[derive(Clone, Copy)]
struct CVal {
    px: f32,
    number: bool,
}

impl CVal {
    fn is_finite(&self) -> bool {
        self.px.is_finite()
    }
}

struct Calc<'a> {
    s: &'a [u8],
    i: usize,
    env: &'a SourceEnv,
}

impl Calc<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn sum(&mut self) -> Option<CVal> {
        let mut v = self.product()?;
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b'+') | Some(b'-') => {
                    let neg = self.s[self.i] == b'-';
                    self.i += 1;
                    let r = self.product()?;
                    if v.number != r.number {
                        return None;
                    }
                    v.px += if neg { -r.px } else { r.px };
                }
                _ => return Some(v),
            }
        }
    }

    fn product(&mut self) -> Option<CVal> {
        let mut v = self.atom()?;
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b'*') => {
                    self.i += 1;
                    let r = self.atom()?;
                    if !v.number && !r.number {
                        return None;
                    }
                    v = CVal {
                        px: v.px * r.px,
                        number: v.number && r.number,
                    };
                }
                Some(b'/') => {
                    self.i += 1;
                    let r = self.atom()?;
                    if !r.number || r.px == 0.0 {
                        return None;
                    }
                    v.px /= r.px;
                }
                _ => return Some(v),
            }
        }
    }

    fn atom(&mut self) -> Option<CVal> {
        self.ws();
        if self.s.get(self.i) == Some(&b'(') {
            self.i += 1;
            let v = self.sum()?;
            self.ws();
            if self.s.get(self.i) != Some(&b')') {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric()
                || matches!(self.s[self.i], b'.' | b'%')
                || (self.i == start && matches!(self.s[self.i], b'+' | b'-')))
        {
            self.i += 1;
        }
        let tok = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        let (num, unit) = split_number(tok)?;
        if unit.is_empty() {
            Some(CVal {
                px: num,
                number: true,
            })
        } else {
            Some(CVal {
                px: num * unit_px(unit, self.env)?,
                number: false,
            })
        }
    }
}

// ------------------------------------------------------------------ media conditions

/// Evaluate a media condition (`(min-width: 600px) and (orientation: landscape)`); `false` for
/// anything it cannot evaluate.
pub fn eval_media(cond: &str, env: &SourceEnv) -> bool {
    let c = cond.trim();
    // Optional media type prefix in a `sizes` entry is not allowed; only conditions are.
    eval_condition(c, env).unwrap_or(false)
}

/// `Some(bool)`, or `None` when the text is not a condition this evaluator understands.
fn eval_condition(c: &str, env: &SourceEnv) -> Option<bool> {
    let c = c.trim();
    if let Some(rest) = strip_keyword(c, "not") {
        return eval_in_parens(rest, env).map(|v| !v);
    }
    // `a and b and c` / `a or b or c` (no mixing without parentheses).
    for (kw, is_and) in [("and", true), ("or", false)] {
        let parts = split_keyword(c, kw);
        if parts.len() > 1 {
            let mut acc = is_and;
            for p in parts {
                let v = eval_in_parens(p, env)?;
                acc = if is_and { acc && v } else { acc || v };
            }
            return Some(acc);
        }
    }
    eval_in_parens(c, env)
}

fn strip_keyword<'a>(c: &'a str, kw: &str) -> Option<&'a str> {
    let r = c.strip_prefix(kw)?;
    (r.starts_with(|ch: char| ch.is_ascii_whitespace() || ch == '(')).then(|| r.trim_start())
}

/// Split at top-level whitespace-delimited keyword occurrences.
fn split_keyword<'a>(c: &'a str, kw: &str) -> Vec<&'a str> {
    let b = c.as_bytes();
    let (mut depth, mut start) = (0i32, 0usize);
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => depth = (depth - 1).max(0),
            ch if depth == 0 && ch.is_ascii_whitespace() => {
                let rest = c[i..].trim_start();
                let skipped = c.len() - i - rest.len();
                if let Some(after) = rest.strip_prefix(kw)
                    && after.starts_with(|x: char| x.is_ascii_whitespace() || x == '(')
                {
                    out.push(&c[start..i]);
                    start = i + skipped + kw.len();
                    i = start;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&c[start..]);
    out
}

fn eval_in_parens(c: &str, env: &SourceEnv) -> Option<bool> {
    let c = c.trim();
    let inner = c.strip_prefix('(')?.strip_suffix(')')?.trim();
    // A nested condition, or a feature test.
    if inner.starts_with('(') || strip_keyword(inner, "not").is_some() {
        return eval_condition(inner, env);
    }
    eval_feature(inner, env)
}

fn eval_feature(f: &str, env: &SourceEnv) -> Option<bool> {
    let (w, h) = env.viewport;
    // Range syntax: `width >= 600px`, `400px <= width < 800px`.
    if f.contains(['<', '>', '=']) && !f.contains(':') {
        return eval_range(f, env);
    }
    let (name, value) = match f.split_once(':') {
        Some((n, v)) => (n.trim().to_ascii_lowercase(), Some(v.trim())),
        None => (f.trim().to_ascii_lowercase(), None),
    };
    let len = |v: &str| eval_length(v, env);
    Some(match (name.as_str(), value) {
        ("width", Some(v)) => len(v).is_some_and(|x| (w - x).abs() < 1e-3),
        ("min-width", Some(v)) => len(v).is_some_and(|x| w >= x),
        ("max-width", Some(v)) => len(v).is_some_and(|x| w <= x),
        ("height", Some(v)) => len(v).is_some_and(|x| (h - x).abs() < 1e-3),
        ("min-height", Some(v)) => len(v).is_some_and(|x| h >= x),
        ("max-height", Some(v)) => len(v).is_some_and(|x| h <= x),
        ("orientation", Some(v)) => match v {
            "portrait" => h >= w,
            "landscape" => w > h,
            _ => return None,
        },
        ("aspect-ratio", Some(v)) => ratio(v).is_some_and(|r| (w / h - r).abs() < 1e-3),
        ("min-aspect-ratio", Some(v)) => ratio(v).is_some_and(|r| w / h >= r),
        ("max-aspect-ratio", Some(v)) => ratio(v).is_some_and(|r| w / h <= r),
        ("resolution", Some(v)) => resolution(v).is_some_and(|r| (env.dpr - r).abs() < 1e-3),
        ("min-resolution", Some(v)) => resolution(v).is_some_and(|r| env.dpr >= r),
        ("max-resolution", Some(v)) => resolution(v).is_some_and(|r| env.dpr <= r),
        ("prefers-color-scheme", Some(v)) => v == "light",
        ("width", None) | ("height", None) => true,
        _ => return None,
    })
}

fn ratio(v: &str) -> Option<f32> {
    match v.split_once('/') {
        Some((a, b)) => Some(a.trim().parse::<f32>().ok()? / b.trim().parse::<f32>().ok()?),
        None => v.trim().parse().ok(),
    }
}

fn resolution(v: &str) -> Option<f32> {
    let (n, u) = split_number(v.trim())?;
    Some(match u {
        "dppx" | "x" => n,
        "dpi" => n / 96.0,
        "dpcm" => n * 2.54 / 96.0,
        _ => return None,
    })
}

/// `width >= 600px`, `400px <= width < 800px` (also `height`).
fn eval_range(f: &str, env: &SourceEnv) -> Option<bool> {
    let toks = tokenize_range(f)?;
    let value_of = |t: &str| -> Option<f32> {
        match t.to_ascii_lowercase().as_str() {
            "width" => Some(env.viewport.0),
            "height" => Some(env.viewport.1),
            other => eval_length(other, env),
        }
    };
    let cmp = |a: f32, op: &str, b: f32| match op {
        "<" => a < b,
        "<=" => a <= b,
        ">" => a > b,
        ">=" => a >= b,
        "=" => (a - b).abs() < 1e-3,
        _ => false,
    };
    match toks.len() {
        3 => Some(cmp(value_of(&toks[0])?, &toks[1], value_of(&toks[2])?)),
        5 => Some(
            cmp(value_of(&toks[0])?, &toks[1], value_of(&toks[2])?)
                && cmp(value_of(&toks[2])?, &toks[3], value_of(&toks[4])?),
        ),
        _ => None,
    }
}

fn tokenize_range(f: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '<' | '>' | '=') {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
            let mut op = c.to_string();
            if c != '=' && chars.peek() == Some(&'=') {
                op.push('=');
                chars.next();
            }
            out.push(op);
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    Some(out)
}

// ------------------------------------------------------------------ selection

/// HTML "select an image source": from `src`, `srcset` and `sizes` for `env`. `None` when the
/// element has no usable source at all.
pub fn select_image_source(
    src: Option<&str>,
    srcset: Option<&str>,
    sizes: Option<&str>,
    env: &SourceEnv,
) -> Option<Selected> {
    let src = src.map(str::trim).filter(|s| !s.is_empty());
    let mut cands = srcset.map(parse_srcset_candidates).unwrap_or_default();
    let has_w = cands.iter().any(|c| c.width.is_some());
    // Width descriptors win: a candidate without one is not usable beside them.
    let mut list: Vec<(String, f32)> = Vec::new();
    if has_w {
        let size = source_size(sizes, env).max(0.0);
        cands.retain(|c| c.width.is_some());
        for c in &cands {
            let density = if size > 0.0 {
                c.width.unwrap_or(1) as f32 / size
            } else {
                f32::INFINITY
            };
            list.push((c.url.clone(), density));
        }
    } else {
        for c in &cands {
            list.push((c.url.clone(), c.density.unwrap_or(1.0)));
        }
        if let Some(s) = src
            && !list.iter().any(|(_, d)| *d == 1.0)
        {
            list.push((s.to_string(), 1.0));
        }
    }
    if list.is_empty() {
        return src.map(|s| Selected {
            url: s.to_string(),
            density: 1.0,
        });
    }
    // Ascending density; of equal densities the first listed stays.
    list.sort_by(|a, b| a.1.total_cmp(&b.1));
    list.dedup_by(|b, a| a.1 == b.1);
    let pick = list.iter().find(|(_, d)| *d >= env.dpr).or(list.last())?;
    Some(Selected {
        url: pick.0.clone(),
        density: if pick.1.is_finite() && pick.1 > 0.0 {
            pick.1
        } else {
            1.0
        },
    })
}

// ------------------------------------------------------------------ picture (015 HTML-4)

/// The attributes of one `source` child of a `picture` that precedes the `img` (HTML 4.8.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PictureSource<'a> {
    pub srcset: Option<&'a str>,
    pub sizes: Option<&'a str>,
    pub media: Option<&'a str>,
    pub mime: Option<&'a str>,
}

/// The image MIME types 007 decodes (a `source` whose `type` names another is skipped).
pub fn supported_image_type(mime: &str) -> bool {
    let t = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        t.as_str(),
        "image/png"
            | "image/apng"
            | "image/jpeg"
            | "image/jpg"
            | "image/pjpeg"
            | "image/gif"
            | "image/webp"
            | "image/svg+xml"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

/// Evaluate a media query list (`media` of a `source`): comma-separated queries, each an optional
/// `only` / `not`, a media type (`all` and `screen` match, other types do not) followed by `and` and a
/// condition, or a condition alone. An empty list matches; a query the evaluator cannot read does not.
pub fn eval_media_query_list(list: &str, env: &SourceEnv) -> bool {
    let list = list.trim();
    if list.is_empty() {
        return true;
    }
    let mut depth = 0i32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, ch) in list.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    parts.iter().any(|q| eval_media_query(q.trim(), env))
}

fn eval_media_query(q: &str, env: &SourceEnv) -> bool {
    if q.starts_with('(') {
        return eval_media(q, env);
    }
    let (negate, rest) = match strip_keyword(q, "not") {
        Some(r) => (true, r),
        None => (false, strip_keyword(q, "only").unwrap_or(q)),
    };
    let mut words = rest.splitn(2, char::is_whitespace);
    let ty = words.next().unwrap_or("").to_ascii_lowercase();
    let tail = words.next().unwrap_or("").trim();
    let type_ok = match ty.as_str() {
        "all" | "screen" => true,
        "print" | "tty" | "tv" | "projection" | "handheld" | "braille" | "embossed" | "aural"
        | "speech" => false,
        _ => return false,
    };
    let v = if tail.is_empty() {
        type_ok
    } else {
        match strip_keyword(tail, "and") {
            Some(cond) => type_ok && eval_condition(cond, env).unwrap_or(false),
            None => return false,
        }
    };
    v != negate
}

/// HTML "select an image source" for an `img` whose parent is a `picture`: the first `source` (in
/// order, all before the `img`) with a `srcset` holding candidates, whose `media` matches and whose
/// `type` is supported, provides the source set (with its `sizes`); otherwise the `img`'s own `src`,
/// `srcset` and `sizes` do.
pub fn select_picture_source(
    sources: &[PictureSource<'_>],
    src: Option<&str>,
    srcset: Option<&str>,
    sizes: Option<&str>,
    env: &SourceEnv,
) -> Option<Selected> {
    for s in sources {
        let Some(set) = s.srcset else { continue };
        if s.media.is_some_and(|m| !eval_media_query_list(m, env)) {
            continue;
        }
        if s.mime
            .is_some_and(|t| !t.trim().is_empty() && !supported_image_type(t))
        {
            continue;
        }
        if parse_srcset_candidates(set).is_empty() {
            continue;
        }
        return select_image_source(None, Some(set), s.sizes, env);
    }
    select_image_source(src, srcset, sizes, env)
}
