//! owner: 003-document-model
//!
//! T020 / U4T-140 (FR-001, SC-001): html5lib tree-construction conformance over the pinned corpus
//! in verification/fixtures/dom-parser/tree-construction/. Runs the `#script-off` and unmarked
//! cases, whole-document and fragment. Script execution and declarative shadow DOM
//! are outside the application foundation. Dependency differences are printed explicitly.

mod common;

use std::path::PathBuf;
use ui4me_document::document::Document;

#[derive(Debug, Default, Clone)]
struct Case {
    file: String,
    index: usize,
    data: String,
    fragment: Option<String>,
    script_on: bool,
    document: String,
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dom-parser/tree-construction")
}

fn parse_dat(file: &str, text: &str) -> Vec<Case> {
    let mut cases = vec![];
    let mut cur: Option<Case> = None;
    let mut section = String::new();
    let mut idx = 0;
    let finish = |c: Option<Case>, cases: &mut Vec<Case>| {
        if let Some(mut c) = c {
            // the document section ends with a blank separator line
            while c.document.ends_with("\n\n") {
                c.document.pop();
            }
            if c.data.ends_with('\n') {
                c.data.pop();
            }
            cases.push(c);
        }
    };
    for line in text.split_inclusive('\n') {
        let l = line.trim_end_matches('\n');
        if l == "#data" {
            finish(cur.take(), &mut cases);
            idx += 1;
            cur = Some(Case {
                file: file.to_owned(),
                index: idx,
                ..Default::default()
            });
            section = "data".into();
            continue;
        }
        if let Some(rest) = l.strip_prefix('#')
            && matches!(
                rest,
                "errors"
                    | "new-errors"
                    | "document-fragment"
                    | "script-on"
                    | "script-off"
                    | "document"
            )
        {
            section = rest.to_owned();
            if rest == "script-on"
                && let Some(c) = cur.as_mut()
            {
                c.script_on = true;
            }
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        match section.as_str() {
            "data" => c.data.push_str(line),
            "document-fragment" => c.fragment = Some(l.to_owned()),
            "document" => c.document.push_str(line),
            _ => {}
        }
    }
    finish(cur.take(), &mut cases);
    cases
}

fn load() -> Vec<Case> {
    let mut files: Vec<_> = std::fs::read_dir(corpus_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    files
        .iter()
        .flat_map(|p| {
            parse_dat(
                p.file_name().unwrap().to_str().unwrap(),
                &std::fs::read_to_string(p).unwrap(),
            )
        })
        .collect()
}

/// html5ever 0.40.1 differs from the newer corpus on two identified cases.
/// These are dependency limitations, not successful conformance results. The
/// foundation separately rejects application documents deeper than 256 nodes.
fn parser_dependency_limit(case: &str) -> bool {
    matches!(case, "custom-template.dat#1" | "nesting-limit.dat#1")
}

fn normalise(s: &str) -> String {
    s.trim_end_matches('\n').to_owned()
}

/// Run one document case; `None` when the expectation cannot apply.
fn run_document(c: &Case) -> String {
    let doc = Document::parse_html(&c.data).expect("parse");
    common::dump_children(&doc, doc.document_node())
}

fn run_fragment(c: &Case) -> Result<String, String> {
    let ctx = c.fragment.as_deref().unwrap();
    let (nsname, local) = match ctx.split_once(' ') {
        Some((n, l)) => (n, l),
        None => ("html", ctx),
    };
    let mut doc = Document::new_empty(ui4me_engine_types::ids::DocumentId::from_raw(
        ui4me_engine_types::ids::RawId {
            index: 77,
            generation: std::num::NonZeroU32::MIN,
        },
    ));
    let uri = match nsname {
        "svg" => "http://www.w3.org/2000/svg",
        "math" => "http://www.w3.org/1998/Math/MathML",
        _ => "http://www.w3.org/1999/xhtml",
    };
    let ctx_el = doc
        .create_element_ns(Some(uri), local)
        .map_err(|e| format!("{e:?}"))?;
    let frag = doc
        .parse_fragment_for_element(ctx_el, &c.data)
        .map_err(|e| format!("{e:?}"))?;
    Ok(common::dump_children(&doc, frag))
}

#[test]
fn tree_construction_corpus_document_cases() {
    let cases = load();
    let (mut run, mut pass, mut excluded, mut fragments) = (0, 0, 0, 0);
    let mut failures = vec![];
    for c in &cases {
        if c.script_on {
            excluded += 1;
            continue;
        }
        if c.fragment.is_some() {
            fragments += 1;
            continue;
        }
        run += 1;
        let got = run_document(c);
        if normalise(&got) == normalise(&c.document) {
            pass += 1;
        } else {
            let short: String = c.data.chars().take(80).collect();
            failures.push(format!(
                "{}#{}: data={:?}\n--- expected\n{}\n--- got\n{}",
                c.file,
                c.index,
                short,
                normalise(&c.document)
                    .lines()
                    .take(40)
                    .collect::<Vec<_>>()
                    .join("\n"),
                normalise(&got)
                    .lines()
                    .take(40)
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    let ids: Vec<String> = failures
        .iter()
        .map(|f| f.split(':').next().unwrap().to_owned())
        .collect();
    let (known, unexpected): (Vec<_>, Vec<_>) =
        ids.iter().partition(|id| parser_dependency_limit(id));
    println!(
        "tree-construction document cases: {pass}/{run} pass; {excluded} script-on excluded; {fragments} fragment cases"
    );
    println!("known dependency differences: {known:?}; unexpected failures: {unexpected:?}");
    assert!(run > 1000, "corpus unexpectedly small: {run}");
    assert!(
        unexpected.is_empty(),
        "unexpected parser failures {unexpected:?}:\n{}",
        failures.join("\n=====\n")
    );
}

#[test]
fn tree_construction_corpus_fragment_cases() {
    let cases = load();
    let (mut run, mut pass, mut excluded) = (0, 0, 0);
    let mut failures = vec![];
    for c in cases.iter().filter(|c| c.fragment.is_some()) {
        if c.script_on {
            excluded += 1;
            continue;
        }
        run += 1;
        match run_fragment(c) {
            Ok(got) if normalise(&got) == normalise(&c.document) => pass += 1,
            Ok(got) => {
                if failures.len() < 12 {
                    failures.push(format!(
                        "{}#{}: ctx={:?} data={:?}\n--- expected\n{}\n--- got\n{}",
                        c.file,
                        c.index,
                        c.fragment,
                        c.data,
                        normalise(&c.document),
                        normalise(&got)
                    ));
                }
            }
            Err(e) => {
                if failures.len() < 12 {
                    failures.push(format!("{}#{}: error {e}", c.file, c.index));
                }
            }
        }
    }
    println!("tree-construction fragment cases: {pass}/{run} pass; {excluded} script-on excluded");
    assert!(run > 100, "fragment corpus unexpectedly small: {run}");
    assert_eq!(
        pass,
        run,
        "fragment-case failures:\n{}",
        failures.join("\n=====\n")
    );
}
