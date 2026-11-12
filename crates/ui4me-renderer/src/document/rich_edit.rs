//! The content model of rich `contenteditable` hosts (1.0-track F1003), kept free of layout and
//! of the live document so it can be tested alone.
//!
//! A rich host is read into a [`Rich`] tree: **leaf blocks** (`p`, `div`, `h1`-`h6`, `li`,
//! `blockquote`, `pre` without block children; containers such as `ul` or a block-holding `div`
//! only nest them), whose inline content is a list of [`Run`]s. A run is a piece of text, a
//! `<br>` or an opaque inline element (an `img`, an empty element), plus its stack of ancestor
//! inline elements (`b`, `i`, `a href`, any unknown `span`...). Formatting adds or removes
//! entries of that stack, so nested markup is split and merged without touching the DOM; the
//! tree is then serialized back with `innerHTML` (children of the host are replaced, element
//! identities inside it are not kept). Elements that are neither blocks nor inline text
//! (`table`, `hr`, `figure`, scripts...) are kept verbatim as [`Item::Other`] and never edited.
//!
//! Positions are **flat byte offsets** into the host's text: the leaf blocks' texts joined by
//! one `\n` (a `<br>` also counts as one `\n`). A block holding only a `<br>` is the empty
//! placeholder (length 0). Editing only ever creates `p` (split paragraphs copy the block name
//! and attributes except `id`), `b`, `i`, `u`, `s`, `code`, `a href` (http/https/mailto) and `br`;
//! no `style`, event-handler or script markup is created.

use super::*;
use ui4me_document::node::NodeKind;

const BLOCKS: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "ul",
    "ol",
    "blockquote",
    "pre",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
];
const OTHER_BLOCKS: &[&str] = &[
    "table", "hr", "figure", "dl", "details", "fieldset", "form", "video", "audio", "canvas",
    "iframe", "object", "svg", "math", "script", "style", "template", "noscript", "textarea",
    "select", "button", "input", "menu",
];
const MAX_RICH_HTML: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Wrap {
    pub name: String,
    pub attrs: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Text(String),
    Br,
    /// A serialized inline element that is kept as it is (zero length).
    Opaque(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Run {
    pub stack: Vec<Wrap>,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub(super) struct Block {
    pub id: Option<NodeId>,
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug)]
pub(super) enum Item {
    Block(Block),
    Container {
        name: String,
        attrs: Vec<(String, String)>,
        kids: Vec<Item>,
    },
    Other(String),
}

#[derive(Clone, Debug, Default)]
pub(super) struct Rich {
    pub items: Vec<Item>,
}

/// The formats Core edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fmt {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Link,
}

impl Fmt {
    fn of(name: &str) -> Option<Fmt> {
        Some(match name {
            "b" | "strong" => Fmt::Bold,
            "i" | "em" => Fmt::Italic,
            "u" => Fmt::Underline,
            "s" | "strike" | "del" => Fmt::Strike,
            "code" => Fmt::Code,
            "a" => Fmt::Link,
            _ => return None,
        })
    }
    fn tag(self) -> &'static str {
        match self {
            Fmt::Bold => "b",
            Fmt::Italic => "i",
            Fmt::Underline => "u",
            Fmt::Strike => "s",
            Fmt::Code => "code",
            Fmt::Link => "a",
        }
    }
    pub(super) fn input_type(self) -> &'static str {
        match self {
            Fmt::Bold => "formatBold",
            Fmt::Italic => "formatItalic",
            Fmt::Underline => "formatUnderline",
            Fmt::Strike => "formatStrikeThrough",
            Fmt::Code => "formatCode",
            Fmt::Link => "insertLink",
        }
    }
}

/// The formats in force at a selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct EditState {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// Only http, https and mailto links are created or kept.
pub(super) fn safe_href(v: &str) -> bool {
    let v = v.trim().to_ascii_lowercase();
    !v.contains(char::is_control)
        && (v.starts_with("https://") || v.starts_with("http://") || v.starts_with("mailto:"))
}

fn esc(t: &str, attr: bool) -> String {
    let mut o = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' if !attr => o.push_str("&lt;"),
            '>' if !attr => o.push_str("&gt;"),
            '"' if attr => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

fn open_tag(name: &str, attrs: &[(String, String)]) -> String {
    let mut o = format!("<{name}");
    for (k, v) in attrs {
        o.push_str(&format!(" {k}=\"{}\"", esc(v, true)));
    }
    o.push('>');
    o
}

fn run_len(r: &Run) -> usize {
    match &r.kind {
        Kind::Text(t) => t.len(),
        Kind::Br => 1,
        Kind::Opaque(_) => 0,
    }
}

fn is_ws_text(runs: &[Run]) -> bool {
    runs.iter()
        .all(|r| matches!(&r.kind, Kind::Text(t) if t.trim().is_empty()))
}

// ---- reading the DOM -------------------------------------------------------------------------

fn attrs_of(d: &Model, n: NodeId) -> Vec<(String, String)> {
    d.attributes(n)
        .map(|a| {
            a.iter()
                .map(|a| (a.name.local.to_string(), a.value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn read_runs(d: &Model, nodes: &[NodeId], stack: &mut Vec<Wrap>, out: &mut Vec<Run>) {
    for &n in nodes {
        match d.node_kind(n) {
            Ok(NodeKind::Text) => {
                let t = d.text_data(n).unwrap_or_default();
                if !t.is_empty() {
                    out.push(Run {
                        stack: stack.clone(),
                        kind: Kind::Text(t.to_owned()),
                    });
                }
            }
            Ok(NodeKind::Element) => {
                let name = d.local_name(n).unwrap_or_default().to_owned();
                let kids = d.children(n).map(<[NodeId]>::to_vec).unwrap_or_default();
                if name == "br" {
                    out.push(Run {
                        stack: stack.clone(),
                        kind: Kind::Br,
                    });
                } else if kids.is_empty() || OTHER_BLOCKS.contains(&name.as_str()) {
                    out.push(Run {
                        stack: stack.clone(),
                        kind: Kind::Opaque(d.outer_html(n).unwrap_or_default()),
                    });
                } else {
                    stack.push(Wrap {
                        name,
                        attrs: attrs_of(d, n),
                    });
                    read_runs(d, &kids, stack, out);
                    stack.pop();
                }
            }
            _ => {}
        }
    }
}

fn has_block_child(d: &Model, n: NodeId) -> bool {
    d.children(n).is_ok_and(|k| {
        k.iter().any(|&c| {
            d.node_kind(c).is_ok_and(|k| k == NodeKind::Element)
                && d.local_name(c)
                    .is_ok_and(|name| BLOCKS.contains(&name) || OTHER_BLOCKS.contains(&name))
        })
    })
}

/// The items for the children of `parent` (a host or a container). Loose inline content is
/// wrapped in a synthetic block named `synthetic`.
fn read_items(d: &Model, parent: NodeId, synthetic: &str) -> Vec<Item> {
    let kids = d
        .children(parent)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let any_block = has_block_child(d, parent);
    let mut items = Vec::new();
    let mut loose: Vec<NodeId> = Vec::new();
    let flush = |loose: &mut Vec<NodeId>, items: &mut Vec<Item>| {
        if loose.is_empty() {
            return;
        }
        let mut runs = Vec::new();
        read_runs(d, loose, &mut Vec::new(), &mut runs);
        loose.clear();
        if runs.is_empty() || (any_block && is_ws_text(&runs)) {
            return;
        }
        items.push(Item::Block(Block {
            id: None,
            name: synthetic.to_owned(),
            attrs: Vec::new(),
            runs,
        }));
    };
    for k in kids {
        if d.node_kind(k).is_ok_and(|x| x == NodeKind::Element) {
            let name = d.local_name(k).unwrap_or_default().to_owned();
            if BLOCKS.contains(&name.as_str()) {
                flush(&mut loose, &mut items);
                if has_block_child(d, k) {
                    items.push(Item::Container {
                        name,
                        attrs: attrs_of(d, k),
                        kids: read_items(d, k, "div"),
                    });
                } else {
                    let mut runs = Vec::new();
                    let nodes = d.children(k).map(<[NodeId]>::to_vec).unwrap_or_default();
                    read_runs(d, &nodes, &mut Vec::new(), &mut runs);
                    items.push(Item::Block(Block {
                        id: Some(k),
                        name,
                        attrs: attrs_of(d, k),
                        runs,
                    }));
                }
                continue;
            }
            if OTHER_BLOCKS.contains(&name.as_str()) && any_block {
                flush(&mut loose, &mut items);
                items.push(Item::Other(d.outer_html(k).unwrap_or_default()));
                continue;
            }
        }
        loose.push(k);
    }
    flush(&mut loose, &mut items);
    items
}

// ---- run operations --------------------------------------------------------------------------

/// Ensure a run boundary at byte `off` of the block; returns the index of the first run after it.
fn split_runs(runs: &mut Vec<Run>, off: usize) -> usize {
    let mut pos = 0;
    for i in 0..runs.len() {
        let l = run_len(&runs[i]);
        if off == pos {
            return i;
        }
        if off < pos + l {
            if let Kind::Text(t) = &mut runs[i].kind {
                let mut at = off - pos;
                while !t.is_char_boundary(at) {
                    at -= 1;
                }
                if at == 0 {
                    return i;
                }
                let tail = t.split_off(at);
                let stack = runs[i].stack.clone();
                runs.insert(
                    i + 1,
                    Run {
                        stack,
                        kind: Kind::Text(tail),
                    },
                );
                return i + 1;
            }
            return i + 1;
        }
        pos += l;
    }
    runs.len()
}

fn coalesce(runs: &mut Vec<Run>) {
    runs.retain(|r| !matches!(&r.kind, Kind::Text(t) if t.is_empty()));
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs.drain(..) {
        if let (Some(last), Kind::Text(t)) = (out.last_mut(), &r.kind)
            && let Kind::Text(lt) = &mut last.kind
            && last.stack == r.stack
        {
            lt.push_str(t);
            continue;
        }
        out.push(r);
    }
    *runs = out;
}

fn delete_in(runs: &mut Vec<Run>, s: usize, e: usize) {
    let a = split_runs(runs, s);
    let b = split_runs(runs, e);
    runs.drain(a..b.max(a));
    coalesce(runs);
}

fn set_fmt(stack: &mut Vec<Wrap>, fmt: Fmt, on: bool, href: Option<&str>) {
    stack.retain(|w| Fmt::of(&w.name) != Some(fmt));
    if on {
        let attrs = match (fmt, href) {
            (Fmt::Link, Some(h)) => vec![("href".to_owned(), h.to_owned())],
            _ => Vec::new(),
        };
        stack.push(Wrap {
            name: fmt.tag().to_owned(),
            attrs,
        });
    }
}

fn stack_at(runs: &[Run], off: usize) -> Vec<Wrap> {
    let mut pos = 0;
    let mut prev: Option<&Run> = None;
    let mut next: Option<&Run> = None;
    for r in runs {
        let l = run_len(r);
        if pos < off && off <= pos + l {
            prev = Some(r);
            if off < pos + l {
                next = Some(r);
            }
        } else if pos == off && next.is_none() {
            next = Some(r);
        }
        if pos > off {
            break;
        }
        pos += l;
    }
    fn text(r: Option<&Run>) -> Option<&Run> {
        r.filter(|r| matches!(r.kind, Kind::Text(_)))
    }
    let (p, n) = (text(prev), text(next));
    let mut st = p.or(n).map(|r| r.stack.clone()).unwrap_or_default();
    // A link continues only through its interior.
    let keep = |w: &Wrap| match (p, n) {
        (Some(p), Some(n)) => p.stack.contains(w) && n.stack.contains(w),
        _ => false,
    };
    st.retain(|w| Fmt::of(&w.name) != Some(Fmt::Link) || keep(w));
    st
}

fn runs_html(runs: &[Run]) -> String {
    let mut out = String::new();
    let mut open: Vec<&Wrap> = Vec::new();
    for r in runs {
        let common = open
            .iter()
            .zip(&r.stack)
            .take_while(|(a, b)| **a == *b)
            .count();
        while open.len() > common {
            let w = open.pop().expect("open");
            out.push_str(&format!("</{}>", w.name));
        }
        for w in &r.stack[common..] {
            out.push_str(&open_tag(&w.name, &w.attrs));
            open.push(w);
        }
        match &r.kind {
            Kind::Text(t) => out.push_str(&esc(t, false)),
            Kind::Br => out.push_str("<br>"),
            Kind::Opaque(raw) => out.push_str(raw),
        }
    }
    while let Some(w) = open.pop() {
        out.push_str(&format!("</{}>", w.name));
    }
    out
}

impl Block {
    fn placeholder(&self) -> bool {
        matches!(self.runs.as_slice(), [Run { kind: Kind::Br, .. }])
    }
    fn len(&self) -> usize {
        if self.placeholder() {
            0
        } else {
            self.runs.iter().map(run_len).sum()
        }
    }
    fn text(&self) -> String {
        if self.placeholder() {
            return String::new();
        }
        let mut s = String::new();
        for r in &self.runs {
            match &r.kind {
                Kind::Text(t) => s.push_str(t),
                Kind::Br => s.push('\n'),
                Kind::Opaque(_) => {}
            }
        }
        s
    }
}

fn collect<'a>(items: &'a [Item], out: &mut Vec<&'a Block>) {
    for it in items {
        match it {
            Item::Block(b) => out.push(b),
            Item::Container { kids, .. } => collect(kids, out),
            Item::Other(_) => {}
        }
    }
}
fn collect_mut<'a>(items: &'a mut [Item], out: &mut Vec<&'a mut Block>) {
    for it in items {
        match it {
            Item::Block(b) => out.push(b),
            Item::Container { kids, .. } => collect_mut(kids, out),
            Item::Other(_) => {}
        }
    }
}
fn insert_after_nth(items: &mut Vec<Item>, n: &mut usize, nb: &mut Option<Block>) -> bool {
    for i in 0..items.len() {
        match &mut items[i] {
            Item::Block(_) => {
                if *n == 0 {
                    items.insert(i + 1, Item::Block(nb.take().expect("block")));
                    return true;
                }
                *n -= 1;
            }
            Item::Container { kids, .. } => {
                if insert_after_nth(kids, n, nb) {
                    return true;
                }
            }
            Item::Other(_) => {}
        }
    }
    false
}
fn remove_nth(items: &mut Vec<Item>, n: &mut usize) -> bool {
    let mut i = 0;
    while i < items.len() {
        match &mut items[i] {
            Item::Block(_) => {
                if *n == 0 {
                    items.remove(i);
                    return true;
                }
                *n -= 1;
            }
            Item::Container { kids, .. } => {
                if remove_nth(kids, n) {
                    return true;
                }
            }
            Item::Other(_) => {}
        }
        i += 1;
    }
    false
}
fn items_html(items: &[Item], out: &mut String) {
    for it in items {
        match it {
            Item::Block(b) => {
                out.push_str(&open_tag(&b.name, &b.attrs));
                out.push_str(&runs_html(&b.runs));
                out.push_str(&format!("</{}>", b.name));
            }
            Item::Container { name, attrs, kids } => {
                out.push_str(&open_tag(name, attrs));
                items_html(kids, out);
                out.push_str(&format!("</{name}>"));
            }
            Item::Other(raw) => out.push_str(raw),
        }
    }
}

impl Rich {
    /// Read the content of `host` (normalized: loose inline content is wrapped in a block, an
    /// empty host gets one empty block).
    pub fn read(d: &Model, host: NodeId) -> Rich {
        let name = d.local_name(host).unwrap_or("div");
        let synthetic = if matches!(name, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            "div"
        } else {
            "p"
        };
        let mut r = Rich {
            items: read_items(d, host, synthetic),
        };
        r.normalize(synthetic);
        r
    }
    /// Parse a (sanitized) HTML fragment and reduce it to paragraphs of allowed inline runs.
    pub fn paste_paragraphs(html: &str) -> Result<Vec<Vec<Run>>> {
        if html.len() > MAX_RICH_HTML {
            return Err(limit_error("Pasted HTML is too large"));
        }
        let id = DocumentId::from_raw(RawId {
            index: u32::MAX - 8,
            generation: NonZeroU32::MIN,
        });
        let d = Model::create(id, html, "about:blank", DomLimits::default())?;
        let Some(body) = d.body() else {
            return Ok(Vec::new());
        };
        let items = read_items(&d, body, "p");
        let mut blocks = Vec::new();
        collect(&items, &mut blocks);
        let mut paras: Vec<Vec<Run>> = Vec::new();
        for b in blocks {
            let mut out: Vec<Run> = Vec::new();
            for r in &b.runs {
                let mut stack = Vec::new();
                for w in &r.stack {
                    match Fmt::of(&w.name) {
                        Some(Fmt::Link) => {
                            if let Some((_, h)) = w.attrs.iter().find(|(k, _)| k == "href")
                                && safe_href(h)
                            {
                                set_fmt(&mut stack, Fmt::Link, true, Some(h.trim()));
                            }
                        }
                        Some(f) => set_fmt(&mut stack, f, true, None),
                        None => {}
                    }
                }
                match &r.kind {
                    Kind::Text(t) => {
                        let mut s = String::new();
                        let mut ws = false;
                        for c in t.chars() {
                            if c.is_whitespace() && c != '\u{a0}' {
                                if !ws {
                                    s.push(' ');
                                }
                                ws = true;
                            } else if !c.is_control() {
                                s.push(c);
                                ws = false;
                            }
                        }
                        if !s.is_empty() {
                            out.push(Run {
                                stack,
                                kind: Kind::Text(s),
                            });
                        }
                    }
                    Kind::Br => out.push(Run {
                        stack,
                        kind: Kind::Br,
                    }),
                    Kind::Opaque(_) => {}
                }
            }
            coalesce(&mut out);
            paras.push(out);
        }
        while paras.len() > 1 && paras.last().is_some_and(Vec::is_empty) {
            paras.pop();
        }
        Ok(paras)
    }
    fn blocks(&self) -> Vec<&Block> {
        let mut v = Vec::new();
        collect(&self.items, &mut v);
        v
    }
    fn blocks_mut(&mut self) -> Vec<&mut Block> {
        let mut v = Vec::new();
        collect_mut(&mut self.items, &mut v);
        v
    }
    /// The ids of the leaf blocks in order (`None` for blocks not yet in the DOM).
    pub fn block_ids(&self) -> Vec<Option<NodeId>> {
        self.blocks().iter().map(|b| b.id).collect()
    }
    pub fn html(&self) -> String {
        let mut s = String::new();
        items_html(&self.items, &mut s);
        s
    }
    /// The flat text.
    pub fn text(&self) -> String {
        self.blocks()
            .iter()
            .map(|b| b.text())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn flat_len(&self) -> usize {
        let b = self.blocks();
        b.iter().map(|b| b.len()).sum::<usize>() + b.len().saturating_sub(1)
    }
    /// (block index, offset in block) of flat offset `flat` (clamped).
    pub fn locate(&self, flat: usize) -> (usize, usize) {
        let mut start = 0;
        let blocks = self.blocks();
        for (i, b) in blocks.iter().enumerate() {
            let l = b.len();
            if flat <= start + l {
                return (i, flat - start);
            }
            start += l + 1;
        }
        let last = blocks.len().saturating_sub(1);
        (last, blocks.get(last).map_or(0, |b| b.len()))
    }
    pub fn flat_of(&self, block: usize, off: usize) -> usize {
        let blocks = self.blocks();
        let before: usize = blocks.iter().take(block).map(|b| b.len() + 1).sum();
        before + off.min(blocks.get(block).map_or(0, |b| b.len()))
    }
    fn strip(&mut self) {
        for b in self.blocks_mut() {
            if b.placeholder() {
                b.runs.clear();
            }
        }
    }
    fn normalize(&mut self, synthetic: &str) {
        for b in self.blocks_mut() {
            coalesce(&mut b.runs);
            if b.runs.is_empty() {
                b.runs.push(Run {
                    stack: Vec::new(),
                    kind: Kind::Br,
                });
            }
        }
        if self.blocks().is_empty() {
            self.items.push(Item::Block(Block {
                id: None,
                name: synthetic.to_owned(),
                attrs: Vec::new(),
                runs: vec![Run {
                    stack: Vec::new(),
                    kind: Kind::Br,
                }],
            }));
        }
    }
    fn finish(&mut self) {
        self.normalize("p");
    }
    fn split_block(&mut self, bi: usize, off: usize) {
        let blocks = self.blocks_mut();
        let b = &mut *blocks.into_iter().nth(bi).expect("block");
        let at = split_runs(&mut b.runs, off);
        let tail = b.runs.split_off(at);
        let nb = Block {
            id: None,
            name: b.name.clone(),
            attrs: b.attrs.iter().filter(|(k, _)| k != "id").cloned().collect(),
            runs: tail,
        };
        let mut n = bi;
        self.insert_after(&mut n, nb);
    }
    fn insert_after(&mut self, n: &mut usize, nb: Block) {
        let mut slot = Some(nb);
        insert_after_nth(&mut self.items, n, &mut slot);
    }
    /// Delete the flat range.
    pub fn delete_range(&mut self, s: usize, e: usize) {
        if s >= e {
            return;
        }
        self.strip();
        let (b1, o1) = self.locate(s);
        let (b2, o2) = self.locate(e);
        if b1 == b2 {
            let mut bl = self.blocks_mut();
            delete_in(&mut bl[b1].runs, o1, o2);
        } else {
            let mut bl = self.blocks_mut();
            let tail = {
                let b = &mut *bl[b2];
                let at = split_runs(&mut b.runs, o2);
                b.runs.split_off(at)
            };
            let first = &mut *bl[b1];
            let l1 = first.len();
            delete_in(&mut first.runs, o1, l1);
            first.runs.extend(tail);
            coalesce(&mut first.runs);
            drop(bl);
            for k in (b1 + 1..=b2).rev() {
                let mut n = k;
                remove_nth(&mut self.items, &mut n);
            }
        }
        self.finish();
    }
    fn insert_in_block(&mut self, bi: usize, off: usize, kind: Kind, fmts: &[(Fmt, bool)]) {
        let mut bl = self.blocks_mut();
        let b = &mut *bl[bi];
        let mut stack = stack_at(&b.runs, off);
        for &(f, on) in fmts {
            if f != Fmt::Link {
                set_fmt(&mut stack, f, on, None);
            }
        }
        let at = split_runs(&mut b.runs, off);
        b.runs.insert(at, Run { stack, kind });
        coalesce(&mut b.runs);
    }
    /// Insert text at flat `at` (a `\n` starts a new paragraph); returns the caret after it.
    pub fn insert_text(&mut self, at: usize, text: &str, fmts: &[(Fmt, bool)]) -> usize {
        self.strip();
        let (mut bi, mut off) = self.locate(at);
        for (k, piece) in text.split('\n').enumerate() {
            if k > 0 {
                self.split_block(bi, off);
                bi += 1;
                off = 0;
            }
            if !piece.is_empty() {
                self.insert_in_block(bi, off, Kind::Text(piece.to_owned()), fmts);
                off += piece.len();
            }
        }
        self.finish();
        self.flat_of(bi, off)
    }
    /// Insert a `<br>` at flat `at`; returns the caret after it.
    pub fn insert_br(&mut self, at: usize) -> usize {
        self.strip();
        let (bi, off) = self.locate(at);
        self.insert_in_block(bi, off, Kind::Br, &[]);
        let mut bl = self.blocks_mut();
        let b = &mut *bl[bi];
        if b.runs.len() == 1 {
            // A lone `<br>` would read as the empty placeholder: keep a second one.
            let r = b.runs[0].clone();
            b.runs.push(r);
        }
        self.finish();
        self.flat_of(bi, off + 1)
    }
    /// Insert pasted paragraphs at `at` (after the range was deleted); returns the caret.
    pub fn insert_paragraphs(&mut self, at: usize, paras: &[Vec<Run>]) -> usize {
        if paras.is_empty() {
            return at;
        }
        self.strip();
        let (mut bi, off) = self.locate(at);
        let mut off = off;
        if paras.len() > 1 {
            self.split_block(bi, off);
        }
        let n = paras.len();
        for (k, p) in paras.iter().enumerate() {
            if k > 0 {
                bi += 1;
                off = 0;
                if k + 1 < n {
                    // A middle paragraph is a new block between the halves.
                    let bl = self.blocks_mut();
                    let name = bl[bi - 1].name.clone();
                    drop(bl);
                    let nb = Block {
                        id: None,
                        name,
                        attrs: Vec::new(),
                        runs: Vec::new(),
                    };
                    let mut at = bi - 1;
                    self.insert_after(&mut at, nb);
                }
            }
            let mut bl = self.blocks_mut();
            let b = &mut *bl[bi];
            let i = split_runs(&mut b.runs, off);
            let len: usize = p.iter().map(run_len).sum();
            for (j, r) in p.iter().enumerate() {
                b.runs.insert(i + j, r.clone());
            }
            coalesce(&mut b.runs);
            off += len;
        }
        self.finish();
        self.flat_of(bi, off)
    }
    fn apply(&mut self, s: usize, e: usize, f: impl Fn(&mut Vec<Wrap>)) {
        self.strip();
        let mut start = 0;
        for b in self.blocks_mut() {
            let l = b.len();
            let (lo, hi) = (s.max(start), e.min(start + l));
            if lo < hi {
                let i = split_runs(&mut b.runs, lo - start);
                let j = split_runs(&mut b.runs, hi - start);
                for r in &mut b.runs[i..j] {
                    if matches!(r.kind, Kind::Text(_)) {
                        f(&mut r.stack);
                    }
                }
                coalesce(&mut b.runs);
            }
            start += l + 1;
        }
        self.finish();
    }
    /// Turn `fmt` on or off over the flat range.
    pub fn set_format(&mut self, s: usize, e: usize, fmt: Fmt, on: bool) {
        self.apply(s, e, |st| set_fmt(st, fmt, on, None));
    }
    pub fn set_link(&mut self, s: usize, e: usize, href: &str) {
        self.apply(s, e, |st| set_fmt(st, Fmt::Link, true, Some(href)));
    }
    /// Remove the link around a collapsed caret (the whole link) or in a range.
    pub fn unlink(&mut self, s: usize, e: usize) {
        if s < e {
            self.apply(s, e, |st| set_fmt(st, Fmt::Link, false, None));
            return;
        }
        self.strip();
        let (bi, off) = self.locate(s);
        let mut bl = self.blocks_mut();
        let b = &mut *bl[bi];
        if let Some(w) = stack_at_any(&b.runs, off) {
            for r in &mut b.runs {
                if r.stack.contains(&w) {
                    r.stack.retain(|x| *x != w);
                }
            }
        }
        coalesce(&mut b.runs);
        self.finish();
    }
    pub fn remove_format(&mut self, s: usize, e: usize) {
        self.apply(s, e, |st| st.retain(|w| Fmt::of(&w.name).is_none()));
    }
    /// The formats at the flat range (all text of a range must have one; a caret takes the
    /// formats typing would continue).
    pub fn state(&self, s: usize, e: usize) -> EditState {
        let mut st = EditState::default();
        let has = |stack: &[Wrap], f: Fmt| stack.iter().any(|w| Fmt::of(&w.name) == Some(f));
        let href = |stack: &[Wrap]| {
            stack
                .iter()
                .find(|w| Fmt::of(&w.name) == Some(Fmt::Link))
                .and_then(|w| w.attrs.iter().find(|(k, _)| k == "href"))
                .map(|(_, v)| v.clone())
        };
        let mut stacks: Vec<Vec<Wrap>> = Vec::new();
        if s >= e {
            let (bi, off) = self.locate(s);
            if let Some(b) = self.blocks().get(bi) {
                stacks.push(stack_at(&b.runs, off));
            }
        } else {
            let mut start = 0;
            for b in self.blocks() {
                let l = b.len();
                let mut pos = start;
                for r in &b.runs {
                    let rl = run_len(r);
                    if matches!(r.kind, Kind::Text(_))
                        && pos < e
                        && pos + rl > s
                        && !b.placeholder()
                    {
                        stacks.push(r.stack.clone());
                    }
                    pos += rl;
                }
                start += l + 1;
            }
        }
        if stacks.is_empty() {
            return st;
        }
        let all = |f: Fmt| stacks.iter().all(|k| has(k, f));
        st.bold = all(Fmt::Bold);
        st.italic = all(Fmt::Italic);
        st.underline = all(Fmt::Underline);
        st.strikethrough = all(Fmt::Strike);
        st.code = all(Fmt::Code);
        if all(Fmt::Link) {
            st.link = href(&stacks[0]);
        }
        st
    }
    /// The selected content: paragraphs of runs (cloned and cut to the range).
    pub fn slice(&self, s: usize, e: usize) -> Vec<Vec<Run>> {
        let mut out = Vec::new();
        let mut start = 0;
        for b in self.blocks() {
            let l = b.len();
            if s <= start + l && e >= start {
                let mut runs = if b.placeholder() {
                    Vec::new()
                } else {
                    b.runs.clone()
                };
                let hi = e.min(start + l) - start;
                let lo = s.max(start) - start;
                let j = split_runs(&mut runs, hi);
                runs.truncate(j);
                let i = split_runs(&mut runs, lo);
                runs.drain(..i);
                out.push(runs);
            }
            start += l + 1;
        }
        out
    }
    /// The HTML of [`Rich::slice`] (inline when it is within one block).
    pub fn slice_html(&self, s: usize, e: usize) -> String {
        let p = self.slice(s, e);
        if p.len() == 1 {
            return runs_html(&p[0]);
        }
        p.iter()
            .map(|r| format!("<p>{}</p>", runs_html(r)))
            .collect()
    }
    pub fn runs_text(runs: &[Run]) -> String {
        runs.iter()
            .map(|r| match &r.kind {
                Kind::Text(t) => t.as_str(),
                Kind::Br => "\n",
                Kind::Opaque(_) => "",
            })
            .collect()
    }
}

/// The link wrap at (or just before) `off`, whatever the boundary rule.
fn stack_at_any(runs: &[Run], off: usize) -> Option<Wrap> {
    let mut pos = 0;
    for r in runs {
        let l = run_len(r);
        if off >= pos
            && off <= pos + l
            && matches!(r.kind, Kind::Text(_))
            && l > 0
            && let Some(w) = r.stack.iter().find(|w| Fmt::of(&w.name) == Some(Fmt::Link))
        {
            return Some(w.clone());
        }
        pos += l;
    }
    None
}
