// owner: F1004 — long-document scalability. The measurement harness (ignored; release build:
// `UI4ME_LD_ROWS=10000 cargo test --release -p ui4me-renderer --lib long_document_harness -- --ignored --nocapture`)
// times the update+render turn of local changes on a large chat/log/table document; the always-on tests
// run the same scenarios on a small document against the cold oracle and bound allocation growth.
use crate::document::{Document, DomOp};
use std::time::Instant;

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|pages| pages * 4)
        .unwrap_or(0)
}

fn row_html(i: usize) -> String {
    format!(
        "<div class=row id=r{i}><b>user{}</b> message number {i} with some wrapped words that make the row take \
         more than one line in a narrow chat pane, and some more</div>",
        i % 7
    )
}

fn page(rows: usize, table_rows: usize) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html><head><style>
body{margin:0;font-family:"Noto Sans";font-size:14px;display:flex;flex-direction:column;height:600px}
#msgs{flex:1;overflow-y:auto;border:1px solid #888}
.row{padding:4px 8px;border-bottom:1px solid #ddd}
.row.hot{background:#fe9}
.row.wide{padding:12px 8px}
.row.gap{margin:10px 0 14px}
.row.emp{padding:0;border:0;height:0;margin:9px 0}
.row.fl{float:left;width:40px;height:30px}
.row.pct{height:50%}
#side{height:120px;overflow:auto}
td{padding:2px 6px;border-bottom:1px solid #ccc}
input{margin:4px}
</style></head><body><input id=q value=x><button id=b>go</button><div id=msgs>"#,
    );
    for i in 0..rows {
        s.push_str(&row_html(i));
    }
    s.push_str("</div><div id=side><table id=tbl><tbody id=tb>");
    for i in 0..table_rows {
        s.push_str(&format!(
            "<tr><td>{i}</td><td>name {i}</td><td>{}.50</td></tr>",
            i * 3
        ));
    }
    s.push_str("</tbody></table></div></body></html>");
    s
}

#[derive(Default)]
struct Stat(Vec<f64>);
impl Stat {
    fn time<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let t = Instant::now();
        let r = f();
        self.0.push(t.elapsed().as_secs_f64() * 1000.0);
        r
    }
    fn line(&mut self, name: &str, rows: usize) -> String {
        self.0.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = self.0.len().max(1);
        let q = |p: f64| {
            self.0
                .get(((n as f64 * p) as usize).min(n - 1))
                .copied()
                .unwrap_or(0.0)
        };
        format!(
            "{name:<22} rows={rows:<6} n={n:<5} p50={:8.2}ms p95={:8.2}ms max={:8.2}ms",
            q(0.5),
            q(0.95),
            self.0.last().copied().unwrap_or(0.0)
        )
    }
}

fn render(d: &mut Document) {
    d.render(800, 600, 1.0).unwrap();
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "measurement harness"]
fn long_document_harness() {
    let rows = env_usize("UI4ME_LD_ROWS", 10_000);
    let iters = env_usize("UI4ME_LD_ITERS", 40);
    let t = Instant::now();
    let mut d = Document::parse(&page(0, 0)).unwrap();
    for chunk in 0..rows.div_ceil(200) {
        let html: String = (chunk * 200..((chunk + 1) * 200).min(rows))
            .map(row_html)
            .collect();
        d.insert_html("msgs", "beforeend", &html).unwrap();
        let trs: String = (chunk * 25..((chunk + 1) * 25).min(rows / 8))
            .map(|i| {
                format!(
                    "<tr><td>{i}</td><td>name {i}</td><td>{}.50</td></tr>",
                    i * 3
                )
            })
            .collect();
        if !trs.is_empty() {
            d.insert_html("tb", "beforeend", &trs).unwrap();
        }
    }
    render(&mut d);
    let rss0 = rss_kb();
    println!(
        "load+first render rows={rows}: {:.0} ms, rss={} MB",
        t.elapsed().as_secs_f64() * 1000.0,
        rss0 / 1024
    );
    if std::env::var_os("UI4ME_LD_REUSE").is_some() {
        d.pipeline_mut_for_testing().set_scene_reuse(true);
    }
    d.set_scroll_follow(Some("msgs"), true).unwrap();
    let only = std::env::var("UI4ME_LD_ONLY").unwrap_or_default();
    let mut out = vec![];
    let mut run = |name: &str, n: usize, f: &mut dyn FnMut(&mut Document, usize)| {
        if !only.is_empty() && !name.contains(only.as_str()) {
            return;
        }
        let mut s = Stat::default();
        let mut rss = vec![];
        for i in 0..iters {
            s.time(|| {
                f(&mut d, i);
                render(&mut d);
            });
            if i % 100 == 99 {
                rss.push(rss_kb() / 1024);
            }
        }
        out.push(s.line(name, n));
        if !rss.is_empty() {
            out.push(format!("  rss MB every 100 iterations: {rss:?}"));
        }
    };
    let tail = format!("r{}", rows - 1);
    let mid = format!("r{}", rows / 2 + 50);
    run("append (follow)", rows, &mut |d, i| {
        d.insert_html("msgs", "beforeend", &row_html(rows + i))
            .unwrap();
    });
    run("append+trim", rows, &mut |d, i| {
        d.apply_batch(&[
            DomOp::InsertHtml {
                element: "msgs".into(),
                position: "beforeend".into(),
                html: row_html(rows + 1000 + i),
            },
            DomOp::TrimChildren {
                element: "msgs".into(),
                keep: rows,
                from_start: true,
            },
        ])
        .unwrap();
    });
    run("stream text tail", rows, &mut |d, _| {
        d.append_text(&tail, "tok ").unwrap();
    });
    run("class toggle (paint)", rows, &mut |d, i| {
        if i % 2 == 0 {
            d.add_class(&mid, "hot")
        } else {
            d.remove_class(&mid, "hot")
        }
        .unwrap();
    });
    run("class toggle (layout)", rows, &mut |d, i| {
        if i % 2 == 0 {
            d.add_class(&mid, "wide")
        } else {
            d.remove_class(&mid, "wide")
        }
        .unwrap();
    });
    run("style change (colour)", rows, &mut |d, i| {
        d.set_attribute(&mid, "style", Some(&format!("color:#{:06x}", i * 977)))
            .unwrap();
    });
    run("style change (layout)", rows, &mut |d, i| {
        d.set_attribute(&mid, "style", Some(&format!("padding:{}px", 4 + i % 5)))
            .unwrap();
    });
    run("scroll top/bottom", rows, &mut |d, i| {
        d.scroll_to(
            Some("msgs"),
            None,
            Some(if i % 2 == 0 { 0.0 } else { rows as f64 * 20.0 }),
        )
        .unwrap();
    });
    run("scroll wheel step", rows, &mut |d, i| {
        d.scroll_by(Some("msgs"), 0.0, 120.0 + i as f64).unwrap();
    });
    run("focus move", rows, &mut |d, i| {
        d.focus(if i % 2 == 0 { "q" } else { "b" }).unwrap();
    });
    run("input value", rows, &mut |d, i| {
        d.set_value("q", &format!("typed {i}")).unwrap();
    });
    run("table row append", rows / 8, &mut |d, i| {
        d.insert_html(
            "tb",
            "beforeend",
            &format!("<tr><td>n</td><td>added {i}</td><td>1</td></tr>"),
        )
        .unwrap();
    });
    run("text append (mid row)", rows, &mut |d, _| {
        d.append_text(&mid, "x").unwrap();
    });
    for l in &out {
        println!("{l}");
    }
    println!("rss start={} MB end={} MB", rss0 / 1024, rss_kb() / 1024);
}

fn small_doc() -> Document {
    Document::parse(&page(60, 20)).unwrap()
}

fn oracle(d: &mut Document, what: &str) {
    render(d);
    let r = d.pipeline_mut_for_testing().cold_oracle(true).unwrap();
    // The scene stage differs only by font blob ids (a cold pipeline interns its own faces); the
    // cascade, box tree, layout and hit index must be equal and the frames byte-identical.
    for (stage, equal) in &r.oracle.stages {
        assert!(
            *equal || format!("{stage:?}") == "Scene",
            "{what}: {stage:?} differs from cold: {:?}",
            r.oracle.first_difference
        );
    }
    assert_eq!(
        r.frames_equal,
        Some(true),
        "{what}: frames differ from cold"
    );
}

#[test]
fn chat_scenarios_equal_cold() {
    let mut d = small_doc();
    oracle(&mut d, "initial");
    d.set_scroll_follow(Some("msgs"), true).unwrap();
    for i in 0..12 {
        d.apply_batch(&[
            DomOp::InsertHtml {
                element: "msgs".into(),
                position: "beforeend".into(),
                html: row_html(100 + i),
            },
            DomOp::TrimChildren {
                element: "msgs".into(),
                keep: 60,
                from_start: i % 2 == 0,
            },
        ])
        .unwrap();
        oracle(&mut d, &format!("append+trim {i}"));
    }
    d.append_text("r110", " streamed token").unwrap();
    oracle(&mut d, "stream text");
    d.add_class("r40", "hot").unwrap();
    oracle(&mut d, "paint class");
    d.add_class("r41", "wide").unwrap();
    oracle(&mut d, "layout class");
    d.set_attribute("r42", "style", Some("color:red")).unwrap();
    oracle(&mut d, "style colour");
    d.set_attribute("r42", "style", Some("padding:20px"))
        .unwrap();
    oracle(&mut d, "style padding");
    d.scroll_to(Some("msgs"), None, Some(0.0)).unwrap();
    oracle(&mut d, "scroll top");
    d.scroll_by(Some("msgs"), 0.0, 300.0).unwrap();
    oracle(&mut d, "scroll middle");
    d.focus("q").unwrap();
    oracle(&mut d, "focus q");
    d.focus("b").unwrap();
    oracle(&mut d, "focus b");
    d.insert_html(
        "tb",
        "beforeend",
        "<tr><td>n</td><td>added</td><td>1</td></tr>",
    )
    .unwrap();
    oracle(&mut d, "table append");
    d.set_value("q", "typed").unwrap();
    oracle(&mut d, "input value");
}

#[test]
fn repeated_append_trim_is_bounded() {
    let mut d = small_doc();
    d.set_scroll_follow(Some("msgs"), true).unwrap();
    render(&mut d);
    let mut warm = (0usize, 0u64);
    for i in 0..1000 {
        d.apply_batch(&[
            DomOp::InsertHtml {
                element: "msgs".into(),
                position: "beforeend".into(),
                html: row_html(1000 + i),
            },
            DomOp::TrimChildren {
                element: "msgs".into(),
                keep: 60,
                from_start: true,
            },
        ])
        .unwrap();
        render(&mut d);
        d.add_class(&format!("r{}", 1000 + i), "hot").unwrap();
        if i == 200 {
            warm = (
                d.pipeline_mut_for_testing().document().arena().live_count(),
                rss_kb(),
            );
        }
    }
    let live = d.pipeline_mut_for_testing().document().arena().live_count();
    assert!(live <= warm.0 + 16, "arena grew: {} -> {live}", warm.0);
    // RSS is process-wide and noisy under the parallel test runner; allow generous slack.
    let rss = rss_kb();
    assert!(rss < warm.1 + 200 * 1024, "rss grew {} -> {rss} KB", warm.1);
}

/// Plain block rows are replay roots (F1004); margins that collapse across rows, empty rows that
/// collapse through, and a float (which turns the replay of block rows off) stay equal to cold.
#[test]
fn block_row_replay_margins_and_floats_equal_cold() {
    let mut d = small_doc();
    oracle(&mut d, "initial");
    for (i, class) in ["gap", "emp", "gap", "wide", "emp", "gap"]
        .iter()
        .enumerate()
    {
        d.add_class(&format!("r{}", 10 + i), class).unwrap();
        oracle(&mut d, &format!("class {class} #{i}"));
    }
    d.remove_class("r11", "emp").unwrap();
    oracle(&mut d, "empty row filled again");
    d.add_class("r20", "fl").unwrap();
    oracle(&mut d, "float appears");
    d.add_class("r21", "gap").unwrap();
    oracle(&mut d, "row beside a float");
    d.remove_class("r20", "fl").unwrap();
    oracle(&mut d, "float gone");
    d.add_class("r30", "pct").unwrap();
    oracle(&mut d, "percentage height row");
    d.append_text(
        "r31",
        " and a longer tail that wraps onto further lines of the row",
    )
    .unwrap();
    oracle(&mut d, "text change after margins");
    d.remove_element("r12").unwrap();
    oracle(&mut d, "row removed");
    d.insert_html("msgs", "afterbegin", &row_html(900)).unwrap();
    oracle(&mut d, "row prepended");
}

/// F1006: a vector picture (inline `<svg>`, `<img src=*.svg>`) used to keep its document's state and
/// resource manager alive for the life of the process (the sized-raster handle held the state it is
/// stored in). A closed document must release its resource manager, with and without a picture.
#[test]
fn closed_document_releases_its_resource_manager_after_vector_pictures() {
    let svg = r#"<svg id=s viewBox="0 0 40 40" width=40 height=40><g id=g></g></svg><img id=i src="data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='20' height='20'%3E%3Ccircle cx='10' cy='10' r='9'/%3E%3C/svg%3E">"#;
    let mut weaks = vec![];
    for _ in 0..3 {
        let mut d = Document::parse(svg).unwrap();
        for i in 0..8 {
            d.replace_children_html(
                "g",
                &format!("<circle cx=\"{}\" cy=\"20\" r=\"6\" fill=\"red\"/>", i * 4),
            )
            .unwrap();
            render(&mut d);
        }
        weaks.push(d.resources_weak_for_testing());
    }
    assert!(
        weaks.iter().all(|w| w.upgrade().is_none()),
        "a closed document's resource manager is still alive"
    );
}
