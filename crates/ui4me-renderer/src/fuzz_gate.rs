//! 1.0 F1104: deterministic seeded fuzz harnesses for the document engine (HTML parser, full
//! pipeline render, CSS, DOM mutation sequences interleaved with input) and the tests that pin
//! every fixed finding. The harnesses are `#[ignore]`d; the toolchain is pinned stable, so this
//! is a self-contained mutation fuzzer (xorshift, no dependencies) instead of cargo-fuzz.
//!
//!     UI4ME_FUZZ_SECS=300 UI4ME_FUZZ_SEED=1 nice cargo test -p ui4me-renderer --lib --release \
//!         fuzz_gate::harness -- --ignored --nocapture --test-threads=1
//!
//! Environment: `UI4ME_FUZZ_SEED` (u64, default 1), `UI4ME_FUZZ_ITERS` (cases, default 2000),
//! `UI4ME_FUZZ_SECS` (wall budget, 0 = iterations only), `UI4ME_FUZZ_TIMEOUT_MS` (per-case
//! watchdog, default 30000), `UI4ME_FUZZ_OUT` (failing-case directory, default the temp dir).
//! Every case runs on a worker thread with an 8 MiB stack; a panic or a watchdog timeout is a
//! finding, the case is saved to `UI4ME_FUZZ_OUT` and the harness fails at the end.
use super::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Snippets spliced into mutated markup/CSS: tokens that stress the parser and resource paths.
const DICT: &[&str] = &[
    "<",
    ">",
    "</",
    "<!--",
    "-->",
    "<![CDATA[",
    "&#x",
    "&#0;",
    "&#xD800;",
    "&amp;",
    "&nbsp;",
    "<table>",
    "<tr><td>",
    "</table>",
    "<svg>",
    "<svg viewBox='0 0 1e9 1e9'>",
    "<math>",
    "<template>",
    "<select>",
    "<textarea>",
    "<input type=file>",
    "<dialog open>",
    "<details open><summary>",
    "<img src='data:image/png;base64,AAAA'>",
    "<img src='http://127.0.0.1:9/x.png'>",
    "<link rel=stylesheet href='//example.invalid/a.css'>",
    "<style>@import url(http://127.0.0.1:9/a.css);</style>",
    "<a href='javascript:alert(1)'>",
    "<form><input name=a>",
    "<div style='",
    "style=\"",
    "{",
    "}",
    ";",
    ":",
    "!important",
    "@media (min-width:1px){",
    "@supports (display:grid){",
    "@font-face{src:url(file:///etc/passwd)}",
    "@import 'x.css';",
    "@keyframes k{from{left:0}to{left:1e9px}}",
    "calc(",
    "calc(1px * 1e308)",
    "var(--a)",
    "--a:var(--a);",
    "url(",
    "url(//example.invalid/x)",
    "display:grid;grid-template-columns:repeat(99999,1fr)",
    "display:flex;flex-wrap:wrap",
    "position:fixed;top:-1e9px",
    "width:1e38px",
    "height:calc(100% - 1e9px)",
    "font-size:1e9px",
    "transform:scale(1e30) rotate(1e30deg)",
    "column-count:99999",
    "border-width:1e9px",
    "content:'\\0'",
    "::before",
    ":not(:not(:not(:is(a,b))))",
    ":nth-child(2n+1 of a)",
    "*",
    "\u{0}",
    "\u{202e}",
    "\u{200d}",
    "\u{fffd}",
    "𝒜",
    "👨‍👩‍👧",
    "\r\n",
    "0",
    "-1",
    "9999999999",
    "NaN",
];

pub fn mutate(rng: &mut Rng, base: &str, others: &[String], max_len: usize) -> String {
    let mut s: Vec<char> = base.chars().collect();
    let rounds = 1 + rng.below(8);
    for _ in 0..rounds {
        let len = s.len();
        match rng.below(9) {
            0 if len > 0 => {
                let i = rng.below(len);
                s.remove(i);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                let n = (1 + rng.below(64)).min(len - i);
                s.drain(i..i + n);
            }
            2 => {
                let i = rng.below(len + 1);
                let tok: Vec<char> = rng.pick(DICT).chars().collect();
                s.splice(i..i, tok);
            }
            3 if len > 1 => {
                let a = rng.below(len);
                let n = (1 + rng.below(200)).min(len - a);
                let chunk: Vec<char> = s[a..a + n].to_vec();
                let times = 1 + rng.below(20);
                let at = rng.below(len + 1);
                let rep: Vec<char> = (0..times).flat_map(|_| chunk.iter().copied()).collect();
                s.splice(at..at, rep);
            }
            4 if !others.is_empty() => {
                let other: Vec<char> = rng.pick(others).chars().collect();
                if !other.is_empty() {
                    let a = rng.below(other.len());
                    let n = (1 + rng.below(400)).min(other.len() - a);
                    let at = rng.below(len + 1);
                    s.splice(at..at, other[a..a + n].iter().copied());
                }
            }
            5 if len > 0 => {
                let i = rng.below(len);
                s[i] = char::from_u32((rng.next() % 0x2FFF) as u32 + 1).unwrap_or('?');
            }
            6 => {
                // nesting bomb
                let tag = *rng.pick(&["div", "span", "b", "table", "ul", "svg", "a", "p", "i"]);
                let depth = 1 + rng.below(600);
                let at = rng.below(len + 1);
                let open: Vec<char> = format!("<{tag}>").repeat(depth).chars().collect();
                s.splice(at..at, open);
            }
            7 if len > 0 => {
                let i = rng.below(len);
                s.truncate(i);
            }
            _ if len > 0 => {
                let i = rng.below(len);
                let j = rng.below(len);
                s.swap(i, j);
            }
            _ => {}
        }
        if s.len() > max_len {
            s.truncate(max_len);
        }
    }
    s.into_iter().collect()
}

/// Seed corpus: every `.html`/`.htm`/`.svg` (and the style blocks of those) under the repo's
/// example, fixture and test-data directories, plus built-in documents.
pub fn corpus(exts: &[&str]) -> Vec<String> {
    fn walk(dir: &Path, exts: &[&str], out: &mut Vec<String>, depth: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if depth < 6 && !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    walk(&p, exts, out, depth + 1);
                }
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.contains(&x))
                && p.metadata().is_ok_and(|m| m.len() < 48 * 1024)
                && let Ok(t) = std::fs::read_to_string(&p)
            {
                out.push(t);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in ["examples", "crates", "tests", "docs"] {
        walk(&root.join(d), exts, &mut out, 0);
    }
    out.truncate(600);
    out
}

pub fn builtin_docs() -> Vec<String> {
    [
        "<!doctype html><title>t</title><p id=p1>Hello <b>world</b> <a href='#x'>link</a><br>line<hr><ul><li>a<li>b</ul>",
        "<table id=t border=1><caption>c</caption><colgroup><col span=2></colgroup><thead><tr><th>a<th>b<tbody><tr><td rowspan=2>1<td colspan=3>2<tr><td>3</table>",
        "<div style='display:grid;grid-template-columns:1fr 2fr;gap:4px'><div style='grid-column:1/-1'>x</div><div>y</div><div>z</div></div>",
        "<div style='display:flex;flex-wrap:wrap;gap:3px'><span style='flex:1 1 40px'>a</span><span style='flex:2'>bb</span></div>",
        "<form><input id=i value=v><textarea id=ta>t</textarea><select id=s><optgroup label=g><option>1<option selected>2</optgroup></select><input type=checkbox checked><input type=range><input type=number min=1><button>b</button></form>",
        "<svg width=100 height=100 viewBox='0 0 100 100'><defs><linearGradient id=g><stop offset=0 stop-color=red /><stop offset=1 stop-color=blue /></linearGradient><clipPath id=c><circle r=30 cx=50 cy=50 /></clipPath></defs><rect width=100 height=100 fill='url(#g)' clip-path='url(#c)' /><path d='M0 0 L100 100 A30 30 0 0 1 10 10 Z' stroke=black /><text x=5 y=50>hi</text></svg>",
        "<style>body{font:14px serif}.a{position:absolute;top:5px;left:5px}.b{float:left;width:30%}.c{columns:3;column-gap:8px}.d{transform:rotate(3deg)}@media(max-width:300px){.a{display:none}}@keyframes k{from{opacity:0}to{opacity:1}}.e{animation:k 1s infinite}</style><div class=a>a</div><div class=b>b</div><div class=c>cccc cccc cccc cccc</div><div class=d>d</div><div class=e>e</div>",
        "<div contenteditable id=ce>edit <b>me</b></div><dialog id=dlg><p>d</p></dialog><div popover id=pop>p</div><details open><summary>s</summary>body</details>",
        "<p style='writing-mode:vertical-rl;direction:rtl'>עברית مرحبا abc 日本語 👨‍👩‍👧</p><ruby>漢<rt>kan</rt></ruby><bdi>x</bdi>",
        "<img src='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='><img src='missing.png' width=10><picture><source srcset='a.png 1x, b.png 2x'><img src='c.png'></picture>",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

pub struct Failure {
    pub kind: &'static str,
    pub detail: String,
}

type CaseFn = Box<dyn FnOnce() + Send>;
struct Job {
    run: CaseFn,
    done: mpsc::Sender<std::result::Result<(), String>>,
}

struct Worker {
    tx: mpsc::Sender<Job>,
}
impl Worker {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.run));
                    let _ = job.done.send(r.map_err(|p| {
                        p.downcast_ref::<String>()
                            .cloned()
                            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                            .unwrap_or_else(|| "panic".into())
                    }));
                }
            })
            .expect("spawn fuzz worker");
        Worker { tx }
    }
}

/// Run `cases` generated cases (or until the time budget). `make(rng, n)` returns the saved
/// description of the case and its body; the body runs under the watchdog.
pub fn run_cases(name: &str, mut make: impl FnMut(&mut Rng, u64) -> (String, CaseFn)) {
    let seed = env_u64("UI4ME_FUZZ_SEED", 1);
    let iters = env_u64("UI4ME_FUZZ_ITERS", 2000);
    let secs = env_u64("UI4ME_FUZZ_SECS", 0);
    let timeout = Duration::from_millis(env_u64("UI4ME_FUZZ_TIMEOUT_MS", 30_000));
    let out = std::env::var_os("UI4ME_FUZZ_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ui4me-fuzz"));
    std::fs::create_dir_all(&out).ok();
    let mut rng = Rng::new(seed ^ name.bytes().fold(7u64, |a, b| a * 31 + u64::from(b)));
    let started = Instant::now();
    let mut worker = Worker::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut n = 0u64;
    let mut longest = Duration::ZERO;
    loop {
        if secs > 0 {
            if started.elapsed() >= Duration::from_secs(secs) {
                break;
            }
        } else if n >= iters {
            break;
        }
        if failures.len() >= 20 {
            break;
        }
        let (desc, body) = make(&mut rng, n);
        let (dtx, drx) = mpsc::channel();
        let t0 = Instant::now();
        worker
            .tx
            .send(Job {
                run: body,
                done: dtx,
            })
            .expect("worker alive");
        let outcome = drx.recv_timeout(timeout);
        longest = longest.max(t0.elapsed());
        let fail = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(msg)) => Some(("panic", msg)),
            Err(_) => Some((
                "hang",
                format!("no result after {} ms", timeout.as_millis()),
            )),
        };
        if let Some((kind, detail)) = fail {
            let path = out.join(format!("{name}-seed{seed}-case{n}.txt"));
            std::fs::write(&path, &desc).ok();
            eprintln!("FINDING {kind} in {name} case {n}: {detail}\n  saved {path:?}");
            failures.push(Failure { kind, detail });
            if kind == "hang" {
                worker = Worker::new(); // the stuck thread is abandoned
            }
        }
        n += 1;
    }
    eprintln!(
        "fuzz {name}: seed {seed}, {n} cases in {:.1}s, slowest {:?}, {} finding(s)",
        started.elapsed().as_secs_f64(),
        longest,
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{name}: {} finding(s), first: {} {}",
        failures.len(),
        failures[0].kind,
        failures[0].detail
    );
}

fn known_code(code: i32) -> bool {
    (1..=16).contains(&code)
}

/// Run one document operation: an error must carry a registered code and a message.
fn ok<T>(r: Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            assert!(
                known_code(e.code),
                "unregistered error code {} ({})",
                e.code,
                e.message
            );
            assert!(!e.message.is_empty(), "error {} without a message", e.code);
            None
        }
    }
}

fn exercise(d: &mut Document, rng: &mut Rng) {
    let w = 1 + rng.below(1400) as u32;
    let h = 1 + rng.below(900) as u32;
    let scale = *rng.pick(&[0.5, 1.0, 1.0, 1.5, 2.0]);
    if let Some(px) = ok(d.render(w, h, scale)) {
        assert_eq!(px.len(), (w as usize) * (h as usize), "framebuffer size");
    }
    let _ = ok(d.accessibility_tree());
    let _ = d.diagnostics();
    for ev in d.drain_events() {
        let _ = d.event_is_current(&ev);
    }
}

fn pointer_sequence(d: &mut Document, rng: &mut Rng) {
    for _ in 0..(1 + rng.below(6)) {
        let x = rng.below(700) as f64;
        let y = rng.below(500) as f64;
        let input = match rng.below(9) {
            0 => Input::PointerMoved { x, y },
            1 => Input::PointerButton {
                pressed: true,
                button: 0,
                x,
                y,
            },
            2 => Input::PointerButton {
                pressed: false,
                button: *rng.pick(&[0, 1, 2]),
                x,
                y,
            },
            3 => Input::Wheel {
                dx: (rng.below(200) as f64) - 100.0,
                dy: (rng.below(600) as f64) - 300.0,
                x,
                y,
            },
            4 => Input::PointerLeft,
            5 => Input::Text(rng.pick(DICT).to_string()),
            6 => Input::Key {
                key: rng
                    .pick(&[
                        "Tab",
                        "Enter",
                        " ",
                        "ArrowDown",
                        "ArrowLeft",
                        "Backspace",
                        "Delete",
                        "Escape",
                        "Home",
                        "End",
                        "a",
                        "z",
                        "c",
                        "v",
                        "PageDown",
                    ])
                    .to_string(),
                code: String::new(),
                pressed: rng.chance(60),
                repeat: rng.chance(10),
                shift: rng.chance(25),
                ctrl: rng.chance(25),
                alt: rng.chance(10),
                meta: false,
            },
            7 => Input::ImePreedit {
                text: rng.pick(DICT).to_string(),
                cursor: Some((rng.below(5), rng.below(9))),
            },
            _ => Input::ImeCommit(rng.pick(DICT).to_string()),
        };
        if let Some(events) = ok(d.handle_input(input)) {
            assert!(events.len() <= 4096, "unbounded event batch");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Harness 1: HTML parser + full pipeline render
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "fuzz harness; see module docs"]
fn harness_html_pipeline() {
    let mut seeds = builtin_docs();
    seeds.extend(corpus(&["html", "htm", "svg"]));
    eprintln!("html corpus: {} seeds", seeds.len());
    let seeds = Arc::new(seeds);
    run_cases("html", move |rng, _| {
        let base = rng.pick(&seeds).clone();
        let input = if rng.chance(8) {
            // random bytes as lossy text
            let n = rng.below(2000);
            let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            mutate(rng, &base, &seeds, 120_000)
        };
        let case_seed = rng.next();
        let desc = input.clone();
        (
            desc,
            Box::new(move || {
                let mut r = Rng::new(case_seed);
                if let Some(mut d) = ok(Document::parse(&input)) {
                    exercise(&mut d, &mut r);
                    pointer_sequence(&mut d, &mut r);
                    exercise(&mut d, &mut r);
                    let _ = ok(d.get_html(None, None, true));
                    let _ = ok(d.paginate(None, None, r.chance(50)));
                }
            }),
        )
    });
}

// ---------------------------------------------------------------------------------------------
// Harness 2: CSS
// ---------------------------------------------------------------------------------------------

fn style_blocks(docs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for d in docs {
        let mut rest = d.as_str();
        while let Some(i) = rest.find("<style") {
            let Some(s) = rest[i..].find('>') else { break };
            let body = &rest[i + s + 1..];
            let Some(e) = body.find("</style>") else {
                break;
            };
            out.push(body[..e].to_owned());
            rest = &body[e..];
        }
    }
    out.extend(
        [
            "p{color:red;margin:1em 2em}",
            ".a:hover>b~i+u{background:linear-gradient(to right,red,blue)}",
            "@media(min-width:300px)and(max-width:900px){div{display:grid;grid-template-areas:'a b' 'c d'}}",
            ":root{--x:calc(1px*2)}div{width:var(--x);transition:width 1s;animation:a 2s}@keyframes a{to{width:9px}}",
            "table{border-collapse:collapse}td{border:1px solid;padding:2px}",
            "@font-face{font-family:f;src:url(f.woff2)}p{font:italic bold 12px/1.5 f,serif}",
        ]
        .map(String::from),
    );
    out
}

const CSS_DOC: &str = "<!doctype html><body><div id=a class='x y'><p id=b>Some <b>text</b> here</p><ul id=c><li>1<li>2</ul><table id=t><tr><td>1<td>2</table><div id=g style='display:grid'><i>g</i><i>h</i></div><input id=i><svg id=s width=40 height=40><rect width=30 height=30 /></svg></div>";

#[test]
#[ignore = "fuzz harness; see module docs"]
fn harness_css() {
    let mut docs = builtin_docs();
    docs.extend(corpus(&["html", "svg"]));
    let sheets = Arc::new(style_blocks(&docs));
    eprintln!("css corpus: {} sheets", sheets.len());
    run_cases("css", move |rng, _| {
        let n = 1 + rng.below(4);
        let css: Vec<String> = (0..n)
            .map(|_| {
                let base = rng.pick(&sheets).clone();
                mutate(rng, &base, &sheets, 60_000)
            })
            .collect();
        let styles: Vec<String> = (0..2)
            .map(|_| {
                let b = rng.pick(&sheets).clone();
                mutate(rng, &b, &sheets, 400)
            })
            .collect();
        let case_seed = rng.next();
        let desc = format!(
            "{}\n/*--style attrs--*/\n{}",
            css.join("\n/*--sheet--*/\n"),
            styles.join("\n/*--attr--*/\n")
        );
        (
            desc,
            Box::new(move || {
                let mut r = Rng::new(case_seed);
                let mut d = Document::parse(CSS_DOC).unwrap();
                let mut ids = Vec::new();
                for c in &css {
                    if let Some(v) = ok(d.add_stylesheet(c)) {
                        let diags = v["diagnostics"].as_array().map_or(0, Vec::len);
                        assert!(diags <= 50, "unbounded sheet diagnostics: {diags}");
                        ids.push(v["sheet"].as_u64().unwrap());
                    }
                }
                for (i, s) in styles.iter().enumerate() {
                    let target = ["a", "b", "c", "g", "i"][i % 5];
                    let _ = ok(d.set_attribute(target, "style", Some(s)));
                }
                exercise(&mut d, &mut r);
                if let Some(id) = ids.first() {
                    let _ = ok(d.replace_stylesheet(*id, "p{color:green}"));
                    exercise(&mut d, &mut r);
                    let _ = ok(d.remove_stylesheet(*id));
                    assert!(
                        d.remove_stylesheet(*id).is_err(),
                        "second removal must report a stale sheet"
                    );
                }
                let _ = ok(d.computed_style(
                    Some("b"),
                    None,
                    &["color".into(), "width".into(), "--x".into()],
                ));
                exercise(&mut d, &mut r);
                assert!(d.diagnostics().len() <= 128, "unbounded diagnostics");
            }),
        )
    });
}

// ---------------------------------------------------------------------------------------------
// Harness 3: DOM mutation sequences interleaved with input and renders
// ---------------------------------------------------------------------------------------------

const BASE_DOC: &str = "<!doctype html><body style='margin:0'><div id=root><p id=p1>Hello <b id=b1>world</b> text</p><input id=in1 value=x><textarea id=ta1>abc\ndef</textarea><div id=ce contenteditable>edit <i>me</i></div><select id=sel><option>1<option>2</select><button id=btn popovertarget=pop>b</button><div id=pop popover>pop</div><dialog id=dlg>dlg<button id=dbtn>x</button></dialog><ul id=list><li id=l1>1<li id=l2>2</ul><div id=scr style='height:80px;overflow:auto'><div style='height:500px'>tall</div></div><table id=tbl><tr><td id=c1>1<td>2</table></div>";
const FIXED_IDS: &[&str] = &[
    "root", "p1", "b1", "in1", "ta1", "ce", "sel", "btn", "pop", "dlg", "dbtn", "list", "l1", "l2",
    "scr", "tbl", "c1",
];

fn fragment(rng: &mut Rng, counter: &mut u32) -> String {
    let mut id = || {
        *counter += 1;
        format!("n{}", *counter)
    };
    match rng.below(10) {
        0 => format!("<div id={} class=k>{}</div>", id(), rng.pick(DICT)),
        1 => format!("<p id={}>text <span id={}>in</span></p>", id(), id()),
        2 => format!("<ul id={}><li id={}>a<li id={}>b</ul>", id(), id(), id()),
        3 => format!("<table id={}><tr><td id={}>1<td>2</table>", id(), id()),
        4 => format!("<input id={} value='v'>", id()),
        5 => format!("<div id={} style='{}'>x</div>", id(), rng.pick(DICT)),
        6 => format!(
            "<svg id={} width=20 height=20><rect width=10 height=10 /></svg>",
            id()
        ),
        7 => format!(
            "<dialog id={}>d</dialog><div id={} popover>p</div>",
            id(),
            id()
        ),
        8 => rng.pick(DICT).to_string(),
        _ => format!(
            "<div id={}>{}</div>",
            id(),
            "<b>x</b>".repeat(rng.below(40))
        ),
    }
}

#[test]
#[ignore = "fuzz harness; see module docs"]
fn harness_dom_sequences() {
    run_cases("dom", move |rng, _| {
        let case_seed = rng.next();
        let trace = Arc::new(Mutex::new(format!("seed {case_seed}\n")));
        let t2 = trace.clone();
        (
            format!("dom sequence seed {case_seed}; replay with this harness and UI4ME_FUZZ_SEED"),
            Box::new(move || {
                let mut r = Rng::new(case_seed);
                let _ = &t2;
                dom_sequence(&mut r, 40 + r_len(case_seed));
            }),
        )
    });
}

fn r_len(seed: u64) -> usize {
    (seed % 120) as usize
}

fn dom_sequence(rng: &mut Rng, steps: usize) {
    let mut d = Document::parse(BASE_DOC).unwrap();
    d.render(640, 480, 1.0).unwrap();
    let mut counter = 0u32;
    let mut pool: Vec<String> = FIXED_IDS.iter().map(|s| s.to_string()).collect();
    // reference -> original public id, to prove references never alias another node
    let mut refs: Vec<(String, String)> = Vec::new();
    let positions = [
        "beforebegin",
        "afterbegin",
        "beforeend",
        "afterend",
        "bogus",
    ];
    for step in 0..steps {
        let target = rng.pick(&pool).clone();
        let other = rng.pick(&pool).clone();
        match rng.below(26) {
            0 => {
                let f = fragment(rng, &mut counter);
                let pos = *rng.pick(&positions);
                if ok(d.insert_html(&target, pos, &f)).is_some() {
                    for i in 1..=counter {
                        let n = format!("n{i}");
                        if !pool.contains(&n) && d.has_element(&n) {
                            pool.push(n);
                        }
                    }
                }
            }
            1 => {
                let _ = ok(d.remove_element(&target));
            }
            2 => {
                let _ = ok(d.move_element(&target, &other, rng.pick(&positions)));
            }
            3 => {
                let _ = ok(d.set_text(&target, rng.pick(DICT)));
            }
            4 => {
                let name = *rng.pick(&[
                    "class",
                    "style",
                    "hidden",
                    "contenteditable",
                    "disabled",
                    "value",
                    "popover",
                    "open",
                    "tabindex",
                    "href",
                    "src",
                    "colspan",
                    "type",
                    "data-x",
                    "dir",
                ]);
                let v = if rng.chance(25) {
                    None
                } else {
                    Some(rng.pick(DICT).to_string())
                };
                let _ = ok(d.set_attribute(&target, name, v.as_deref()));
            }
            5 => {
                let _ = ok(d.append_text(&target, rng.pick(DICT)));
            }
            6 => {
                let _ = ok(d.trim_children(&target, rng.below(4), rng.chance(50)));
            }
            7 => {
                let f = fragment(rng, &mut counter);
                let _ = ok(d.replace_children_html(&target, &f));
            }
            8 => {
                let ops: Vec<DomOp> = (0..1 + rng.below(6))
                    .map(|_| match rng.below(6) {
                        0 => DomOp::RemoveElement {
                            element: rng.pick(&pool).clone(),
                        },
                        1 => DomOp::SetText {
                            element: rng.pick(&pool).clone(),
                            text: rng.pick(DICT).to_string(),
                        },
                        2 => DomOp::AddClass {
                            element: rng.pick(&pool).clone(),
                            name: "z".into(),
                        },
                        3 => DomOp::MoveElement {
                            element: rng.pick(&pool).clone(),
                            target: rng.pick(&pool).clone(),
                            position: "beforeend".into(),
                        },
                        4 => DomOp::AppendText {
                            element: rng.pick(&pool).clone(),
                            text: "t".into(),
                        },
                        _ => DomOp::TrimChildren {
                            element: rng.pick(&pool).clone(),
                            keep: 1,
                            from_start: true,
                        },
                    })
                    .collect();
                // A failed batch must leave the document unchanged (atomic).
                let before = d.get_html(None, None, true).ok();
                if ok(d.apply_batch(&ops)).is_none() {
                    assert_eq!(
                        before,
                        d.get_html(None, None, true).ok(),
                        "failed batch left a partial change"
                    );
                }
            }
            9 => {
                let _ = ok(d.show_popover(&target));
            }
            10 => {
                let _ = ok(d.toggle_popover(&target));
            }
            11 => {
                let _ = ok(d.hide_popover(&target));
            }
            12 => {
                let _ = ok(d.show_dialog(&target, rng.chance(60)));
            }
            13 => {
                let _ = ok(d.close_dialog(&target, Some("r")));
            }
            14 => {
                let _ = ok(d.select_text(if rng.chance(50) {
                    Some(target.as_str())
                } else {
                    None
                }));
                let _ = ok(d.selection_text());
                let _ = ok(d.copy_selection());
            }
            15 => d.clear_selection(),
            16 => {
                let _ = ok(d.focus(&target));
            }
            17 | 18 => pointer_sequence(&mut d, rng),
            19 => {
                let _ = ok(d.scroll_by(
                    Some(&target),
                    (rng.below(100) as f64) - 50.0,
                    (rng.below(2000) as f64) - 500.0,
                ));
                let _ = ok(d.scroll_into_view(&target, "center", "nearest"));
            }
            20 => {
                let _ =
                    ok(d.set_viewport(1 + rng.below(1200) as u32, 1 + rng.below(900) as u32, 1.0));
            }
            21 => {
                let size = if rng.chance(30) {
                    Some(PageSize::Px(
                        100.0 + rng.below(800) as f32,
                        100.0 + rng.below(800) as f32,
                    ))
                } else {
                    None
                };
                if ok(d.paginate(size, None, true)).is_some() {
                    let n = ok(d.page_count()).unwrap_or(0);
                    for i in 0..n.min(3) {
                        let _ = ok(d.render_page(i, 48.0));
                    }
                }
            }
            22 => {
                let _ = ok(d.set_value(&target, rng.pick(DICT)));
                let _ = ok(d.set_checked(&target, rng.chance(50)));
                let _ = ok(d.get_value(&target));
            }
            23 => {
                let _ = ok(d.add_stylesheet(&mutate(
                    rng,
                    "div{color:red;display:flex}p{margin:3px}",
                    &[],
                    2000,
                )));
            }
            24 => {
                let _ = ok(d.set_selected_values("sel", &["2".to_owned()]));
                let _ = ok(d.select_options("sel"));
            }
            _ => {
                let _ = ok(d.text_rects(Some(&target), None, None, None));
                let _ = ok(d.element_rect(&target));
                let _ = ok(d.text_at_point(rng.below(600) as f64, rng.below(400) as f64));
            }
        }
        // Identity: record references of live elements, then check no reference aliases.
        if rng.chance(30) {
            let id = rng.pick(&pool).clone();
            if let Some(r) = ok(d.node_of(Some(&id))) {
                refs.push((r, id));
                if refs.len() > 64 {
                    refs.remove(0);
                }
            }
        }
        for (r, id) in &refs {
            match d.get_node(r) {
                Ok(v) => {
                    assert_eq!(
                        v["node"].as_str(),
                        Some(r.as_str()),
                        "reference resolved to another node"
                    );
                    if v["type"] == "element" {
                        assert_eq!(
                            v["id"].as_str().unwrap_or(""),
                            id,
                            "reference {r} resolved to a node with another id"
                        );
                    }
                }
                Err(e) => assert_eq!(
                    e.code, 4,
                    "stale reference must report the stale code, got {} ({})",
                    e.code, e.message
                ),
            }
        }
        for ev in d.drain_events() {
            if d.event_is_current(&ev)
                && let Some(el) = ev.get("element").and_then(serde_json::Value::as_str)
            {
                assert!(
                    el.is_empty() || d.has_element(el),
                    "current event for missing element {el}"
                );
            }
        }
        if step % 7 == 0 {
            exercise(&mut d, rng);
        }
        let (ids, total) = d.query_ids("[id]", 100_000).unwrap();
        assert!(
            total <= 65_536 && ids.len() <= total,
            "node budget exceeded"
        );
    }
    exercise(&mut d, rng);
}

// ---------------------------------------------------------------------------------------------
// Regression tests for fixed findings (not ignored) are appended below.
// ---------------------------------------------------------------------------------------------

use std::path::{Path, PathBuf};
