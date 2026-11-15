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
    run_fragment_with(c, None)
}

fn run_fragment_with(
    c: &Case,
    limits: Option<ui4me_document::limits::DomLimits>,
) -> Result<String, String> {
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
    if let Some(l) = limits {
        doc = doc.with_limits(l);
    }
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

/// INPUTSEC-05: a parse with a depth cap is fed to html5ever in chunks so that it can stop early;
/// every corpus case builds the same tree whatever the chunk size (tokenizer states, character
/// references, CR LF and multi-byte characters split across chunks).
#[test]
fn chunked_parsing_builds_the_same_tree() {
    use ui4me_document::limits::DomLimits;
    let capped = DomLimits {
        parse_depth: 1 << 40,
        ..Default::default()
    };
    let id = |i| {
        ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
            index: i,
            generation: std::num::NonZeroU32::MIN,
        })
    };
    let cases: Vec<Case> = load().into_iter().filter(|c| !c.script_on).collect();
    let mut compared = 0;
    for chunk in [1, 2, 3, 7, 61] {
        ui4me_document::parse::set_parse_chunk_for_tests(chunk);
        for c in &cases {
            if c.fragment.is_some() {
                let whole = run_fragment(c);
                let chunked = run_fragment_with(c, Some(capped));
                assert_eq!(
                    whole, chunked,
                    "{}#{} in chunks of {chunk}",
                    c.file, c.index
                );
                compared += 1;
                continue;
            }
            let whole = run_document(c);
            let doc = Document::create(id(3), &c.data, "about:blank", capped).expect("parse");
            let chunked = common::dump_children(&doc, doc.document_node());
            assert_eq!(
                whole, chunked,
                "{}#{} in chunks of {chunk}",
                c.file, c.index
            );
            compared += 1;
        }
    }
    ui4me_document::parse::set_parse_chunk_for_tests(4096);
    assert!(compared > 5000, "{compared}");
}

/// INPUTSEC-05: deeply nested unclosed blocks stop the parse at the depth cap instead of costing
/// time quadratic in the input (40,000 nested `<div>` took 6.3 s before the node limit fired).
#[test]
fn a_parse_deeper_than_its_cap_stops_early() {
    use ui4me_document::limits::DomLimits;
    use ui4me_engine_types::codes_generated::Code;
    let limits = DomLimits {
        parse_depth: 512,
        ..Default::default()
    };
    let id = ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
        index: 5,
        generation: std::num::NonZeroU32::MIN,
    });
    let bound = if cfg!(debug_assertions) {
        std::time::Duration::from_secs(2)
    } else {
        std::time::Duration::from_millis(100)
    };
    for html in [
        format!("<!doctype html><body>{}x", "<div>".repeat(40_000)),
        format!("<!doctype html><body>{}x", "<div><span>".repeat(20_000)),
        format!("<!doctype html><table>{}", "<tr><td><table>".repeat(15_000)),
        format!("<!doctype html><body><template>{}", "<div>".repeat(40_000)),
    ] {
        let t0 = std::time::Instant::now();
        let e = Document::create(id, &html, "about:blank", limits).expect_err("too deep");
        let t = t0.elapsed();
        assert_eq!(e.code, Code::SizeLimit);
        assert!(e.message.contains("dom.parse_depth"), "{}", e.message);
        assert!(t < bound, "{t:?} for {}", &html[..60]);
    }
    // At the cap the document parses (the document node is depth 0, `html` 1, `body` 2).
    let ok = format!("<!doctype html><body>{}", "<div>".repeat(510));
    assert!(Document::create(id, &ok, "about:blank", limits).is_ok());
    let over = format!("<!doctype html><body>{}", "<div>".repeat(511));
    assert!(Document::create(id, &over, "about:blank", limits).is_err());
    // A fragment parse is refused the same way and leaves the document as it was.
    let mut d = Document::create(id, "<div id=a>keep</div>", "about:blank", limits).unwrap();
    let a = d.element_by_id("a").unwrap();
    let live = d.arena().live_count();
    let before = d.serialize_document().unwrap();
    let t0 = std::time::Instant::now();
    let e = d
        .set_inner_html(a, &"<div>".repeat(40_000))
        .expect_err("too deep");
    assert!(t0.elapsed() < bound);
    assert_eq!(e.code, Code::SizeLimit);
    assert_eq!(d.serialize_document().unwrap(), before);
    assert_eq!(d.arena().live_count(), live, "the staged nodes are freed");
}

/// INPUTSEC-05: a capped parse also stops once it holds more nodes than the node limit, unless a
/// `selectedcontent` element (whose contents are rebuilt after the parse) exists; either way the
/// result is the `SIZE_LIMIT` the finished parse would have failed with.
#[test]
fn a_capped_parse_stops_at_the_node_limit() {
    use ui4me_document::limits::DomLimits;
    use ui4me_engine_types::codes_generated::Code;
    let limits = DomLimits {
        parse_depth: 512,
        nodes: 1000,
        ..Default::default()
    };
    let id = ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
        index: 6,
        generation: std::num::NonZeroU32::MIN,
    });
    let deep_wide = format!(
        "<!doctype html><body>{}{}",
        "<div>".repeat(500),
        "<div></div>".repeat(20_000)
    );
    let t0 = std::time::Instant::now();
    let e = Document::create(id, &deep_wide, "about:blank", limits).expect_err("too many");
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        t0.elapsed()
    );
    assert_eq!(e.code, Code::SizeLimit);
    assert!(e.message.contains("dom.nodes"), "{}", e.message);
    // Same outcome as an uncapped parse, and the same as before for a document within the limit.
    let uncapped = DomLimits {
        nodes: 1000,
        ..Default::default()
    };
    let e2 = Document::create(id, &deep_wide, "about:blank", uncapped).expect_err("too many");
    assert_eq!(e2.code, Code::SizeLimit);
    let fits = format!("<!doctype html><body>{}", "<p>x</p>".repeat(300));
    assert!(Document::create(id, &fits, "about:blank", limits).is_ok());
    // With a `selectedcontent` element the stop is off; the finished parse decides.
    let sc = format!(
        "<!doctype html><select><button><selectedcontent>{}</selectedcontent></button><option>a</option></select>",
        "<span>x</span>".repeat(600)
    );
    let capped = Document::create(id, &sc, "about:blank", limits).map(|_| ());
    let whole = Document::create(id, &sc, "about:blank", uncapped).map(|_| ());
    assert_eq!(capped.map_err(|e| e.code), whole.map_err(|e| e.code));
}
