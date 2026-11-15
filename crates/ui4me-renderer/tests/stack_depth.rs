//! T1 (F-01, F-06): content at the documented maximum depth must not exhaust the engine stack.
//!
//! The host runs the whole engine (parse, style, box tree, layout, paint, accessibility) on one
//! thread with an explicit stack of [`ui4me_renderer::ENGINE_STACK_BYTES`]. A stack overflow is a
//! process abort that `catch_unwind` cannot intercept, so every case runs in a child process (this
//! test binary, re-entered through the ignored `stack_probe` test) on a thread with **half** that
//! stack: an overflow fails the test cleanly instead of aborting the suite, and the 2x headroom is
//! what the invariant (audit 03-architecture section 3.3 B) demands.
//!
//! `inline-block` is deliberately absent: its shrink-to-fit layout is exponential in nesting depth
//! (INPUTSEC-06, roadmap 2.5). T1 is about aborts, not time.
//!
//! Measuring: `UI4ME_STACK_MEASURE=div,fieldset cargo test --release -p ui4me-renderer --test
//! stack_depth -- --ignored --nocapture stack_measure` binary-searches the smallest thread stack
//! that survives two depths per kind and prints the stack cost per nesting level.

use std::process::Command;
use ui4me_renderer::Document;

/// Deepest node the document validator accepts (`MAX_DEPTH` in the renderer; the document node
/// is depth 0, `html` 1, `body` 2).
const MAX_DEPTH: usize = 256;

const KINDS: &[&str] = &[
    "div", "fieldset", "table", "flex", "contents", "grid", "mixed",
];

/// A document whose deepest node, the text of `#leaf`, sits at DOM depth `depth`.
fn document(kind: &str, depth: usize) -> String {
    assert!(depth >= 4);
    // Element levels between `body` (2) and the `<span id=leaf>` (depth - 1).
    let mut levels = depth - 4;
    let mut open = String::from("<!doctype html><html><body>");
    let mut close = Vec::new();
    let mut unit = 0usize;
    while levels > 0 {
        let k = if kind == "mixed" {
            ["div", "fieldset", "table", "flex", "contents", "grid", "ul"][unit % 7]
        } else {
            kind
        };
        unit += 1;
        match k {
            "table" if levels >= 4 => {
                open.push_str("<table><tbody><tr><td>");
                close.push("</td></tr></tbody></table>");
                levels -= 4;
            }
            "ul" if levels >= 2 => {
                open.push_str("<ul><li>");
                close.push("</li></ul>");
                levels -= 2;
            }
            "fieldset" => {
                open.push_str("<fieldset>");
                close.push("</fieldset>");
                levels -= 1;
            }
            "flex" => {
                open.push_str("<div style=\"display:flex\">");
                close.push("</div>");
                levels -= 1;
            }
            "grid" => {
                open.push_str("<div style=\"display:grid\">");
                close.push("</div>");
                levels -= 1;
            }
            "contents" => {
                open.push_str("<div style=\"display:contents\">");
                close.push("</div>");
                levels -= 1;
            }
            _ => {
                open.push_str("<div>");
                close.push("</div>");
                levels -= 1;
            }
        }
    }
    open.push_str("<span id=leaf>deep</span>");
    for c in close.iter().rev() {
        open.push_str(c);
    }
    open.push_str("</body></html>");
    open
}

/// What the host does with a document: cold render, accessibility tree, geometry, then an
/// incremental update and a second frame.
fn exercise(kind: &str, depth: usize) -> Result<(), String> {
    let html = document(kind, depth);
    let mut d = Document::parse_hosted(&html, None, 800, 600).map_err(|e| e.message)?;
    d.render(800, 600, 1.0).map_err(|e| e.message)?;
    d.accessibility_tree().map_err(|e| e.message)?;
    let rect = d.element_rect("leaf").map_err(|e| e.message)?;
    if rect.is_none() {
        return Err("the leaf has no box".into());
    }
    d.set_text("leaf", "deeper").map_err(|e| e.message)?;
    d.render(800, 600, 1.0).map_err(|e| e.message)?;
    d.accessibility_tree().map_err(|e| e.message)?;
    Ok(())
}

/// Child-process entry: `UI4ME_STACK_PROBE=<kind>,<depth>,<stack KiB>`.
#[test]
#[ignore = "child-process entry of the stack-depth tests"]
fn stack_probe() {
    let Ok(spec) = std::env::var("UI4ME_STACK_PROBE") else {
        return;
    };
    let mut parts = spec.split(',');
    let kind = parts.next().unwrap().to_owned();
    let depth: usize = parts.next().unwrap().parse().unwrap();
    let kib: usize = parts.next().unwrap().parse().unwrap();
    let result = std::thread::Builder::new()
        .name("stack-probe".into())
        .stack_size(kib * 1024)
        .spawn(move || exercise(&kind, depth))
        .unwrap()
        .join()
        .expect("probe thread panicked");
    match result {
        Ok(()) => println!("PROBE_OK"),
        Err(e) => println!("PROBE_ERR {e}"),
    }
}

enum Outcome {
    Ok,
    Rejected(String),
    Crashed(String),
}

fn probe(kind: &str, depth: usize, kib: usize) -> Outcome {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "stack_probe",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("UI4ME_STACK_PROBE", format!("{kind},{depth},{kib}"))
        .output()
        .expect("spawn probe");
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() && stdout.contains("PROBE_OK") {
        return Outcome::Ok;
    }
    if out.status.success()
        && let Some(line) = stdout.lines().find(|l| l.starts_with("PROBE_ERR"))
    {
        return Outcome::Rejected(line.to_owned());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Outcome::Crashed(format!(
        "{:?}: {}",
        out.status,
        stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ")
    ))
}

fn half_engine_stack_kib() -> usize {
    ui4me_renderer::ENGINE_STACK_BYTES / 2 / 1024
}

#[test]
fn documented_maximum_depth_survives_half_the_engine_stack() {
    let kib = half_engine_stack_kib();
    let mut failures = Vec::new();
    for kind in KINDS {
        match probe(kind, MAX_DEPTH, kib) {
            Outcome::Ok => {}
            Outcome::Rejected(e) => failures.push(format!("{kind}: rejected at {MAX_DEPTH}: {e}")),
            Outcome::Crashed(e) => {
                failures.push(format!("{kind}: aborted at {MAX_DEPTH} on {kib} KiB: {e}"))
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn one_level_beyond_the_maximum_is_rejected_not_rendered() {
    // The cases above run at the real cap: one more level is refused by the validator.
    for kind in ["div", "table"] {
        let html = document(kind, MAX_DEPTH + 1);
        let error = Document::parse_hosted(&html, None, 800, 600)
            .err()
            .unwrap_or_else(|| panic!("{kind} at depth {} was accepted", MAX_DEPTH + 1));
        assert!(error.message.starts_with("SIZE_LIMIT"), "{}", error.message);
        assert!(Document::parse_hosted(&document(kind, MAX_DEPTH), None, 800, 600).is_ok());
    }
}

/// Smallest stack (KiB, within 1 %) on which `kind` at `depth` survives.
fn min_stack_kib(kind: &str, depth: usize) -> Option<usize> {
    let (mut lo, mut hi) = (64usize, 256 * 1024usize);
    if !matches!(probe(kind, depth, hi), Outcome::Ok) {
        return None;
    }
    while hi - lo > (lo / 100).max(16) {
        let mid = (lo + hi) / 2;
        if matches!(probe(kind, depth, mid), Outcome::Ok) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

#[test]
#[ignore = "measurement tool; run explicitly (see the module documentation)"]
fn stack_measure() {
    let kinds = std::env::var("UI4ME_STACK_MEASURE").unwrap_or_else(|_| KINDS.join(","));
    let (a, b) = (64usize, MAX_DEPTH);
    println!("| kind | min stack @{a} (KiB) | min stack @{b} (KiB) | per DOM level (KiB) |");
    println!("|---|---|---|---|");
    for kind in kinds.split(',') {
        let sa = min_stack_kib(kind, a);
        let sb = min_stack_kib(kind, b);
        let per = match (sa, sb) {
            (Some(sa), Some(sb)) => format!("{:.1}", (sb as f64 - sa as f64) / (b - a) as f64),
            _ => "n/a".into(),
        };
        let show = |s: Option<usize>| s.map_or("> 262144".into(), |s| s.to_string());
        println!("| {kind} | {} | {} | {per} |", show(sa), show(sb));
    }
}
