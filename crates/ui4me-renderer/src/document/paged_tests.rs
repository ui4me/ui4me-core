//! 1.0 F1001: public pagination through the production facade.
use super::paged_facade::PageSize;
use super::*;

fn doc(body: &str) -> Document {
    let mut d = Document::parse(&format!("<!doctype html><style>body{{margin:0;font:16px/20px sans-serif}} p{{margin:0}}</style>{body}")).unwrap();
    d.render(800, 600, 1.0).unwrap();
    d
}

#[test]
fn forced_breaks_page_size_and_element_pages() {
    let mut d = doc(
        "<style>@page{size:A5;margin:20px} .b{break-before:page}</style><h1 id=a>One</h1><p class=b id=b>Two</p><p class=b id=c>Three</p>",
    );
    let info = d.paginate(None, None, true).unwrap();
    assert_eq!(info["count"], 3);
    // A5 = 148 x 210 mm at 96 px/in; the @page size wins over the default A4.
    let w = info["pages"][0]["width"].as_f64().unwrap();
    assert!((w - 148.0 * 96.0 / 25.4).abs() < 0.5, "{info}");
    assert_eq!(info["pages"][0]["content"]["x"], 20.0);
    for (id, page) in [("a", 0), ("b", 1), ("c", 2)] {
        assert_eq!(d.page_rects(id).unwrap()[0]["page"], page, "{id}");
    }
    // Repeatable: the same document paginates the same.
    let again = d.paginate(None, None, true).unwrap();
    assert_eq!(info, again);
    // A requested size applies when the document sets none.
    let mut e = doc("<p>x</p>");
    let l = e
        .paginate(
            Some(PageSize::Named("letter landscape".into())),
            Some([10.0; 4]),
            true,
        )
        .unwrap();
    assert_eq!(l["pages"][0]["width"], 1056.0);
    assert_eq!(l["pages"][0]["height"], 816.0);
    assert!(
        e.paginate(Some(PageSize::Named("A9000".into())), None, true)
            .is_err()
    );
}

#[test]
fn widows_orphans_print_media_and_staleness() {
    let lines: String = (0..60).map(|i| format!("line {i}<br>")).collect();
    let mut d = doc(&format!(
        "<style>@page{{size:400px 300px;margin:0}} p{{orphans:3;widows:3}} @media print{{#screen{{display:none}}}}</style><div id=screen style='height:250px'>screen only</div><p id=p>{lines}</p>"
    ));
    let info = d.paginate(None, None, true).unwrap();
    let pages = info["count"].as_u64().unwrap();
    // 60 lines of 20 px on 300 px pages: 15 lines per page -> 4 pages (the screen-only block is gone in print).
    assert_eq!(pages, 4, "{info}");
    let rects = d.page_rects("p").unwrap();
    assert_eq!(rects.as_array().unwrap().len(), 4);
    // Each fragment holds at least 3 lines (orphans/widows).
    for r in rects.as_array().unwrap() {
        assert!(r["height"].as_f64().unwrap() >= 60.0, "{rects}");
    }
    // Screen media keeps the block.
    let screen = d.paginate(None, None, false).unwrap();
    assert!(screen["count"].as_u64().unwrap() > pages);
    // A DOM change makes the pagination stale until paginate runs again.
    d.set_text("p", "short").unwrap();
    assert!(d.page_rects("p").is_err());
    assert_eq!(d.paginate(None, None, true).unwrap()["count"], 1);
}

#[test]
fn margin_box_headers_and_page_rendering() {
    let mut d = doc(
        "<style>@page{size:300px 200px;margin:40px;@top-center{content:'Page ' counter(page) ' of ' counter(pages)}} .b{break-before:page}</style><p>one</p><p class=b>two</p>",
    );
    assert_eq!(d.paginate(None, None, true).unwrap()["count"], 2);
    let b = d.render_page(1, 96.0).unwrap();
    assert_eq!((b.width, b.height), (300, 200));
    // The header is drawn in the top margin (dark pixels in the top 40 px), the page is white elsewhere.
    let dark =
        |y0: u32, y1: u32| (y0 * b.width..y1 * b.width).any(|i| b.data[(i * 4) as usize] < 128);
    assert!(dark(5, 38), "margin-box header painted");
    assert_eq!(&b.data[0..4], &[255, 255, 255, 255]);
    let hi = d.render_page(0, 192.0).unwrap();
    assert_eq!((hi.width, hi.height), (600, 400));
    assert!(d.render_page(2, 96.0).is_err());
    assert!(d.render_page(0, 100_000.0).is_err());
}

#[test]
fn named_pages_get_their_own_size() {
    let mut d = doc(
        "<style>@page{size:300px 200px;margin:0} @page wide{size:600px 200px} #w{page:wide}</style><p>a</p><div id=w>wide</div><p id=z>b</p>",
    );
    let info = d.paginate(None, None, true).unwrap();
    assert!(
        d.diagnostics()
            .iter()
            .all(|e| e["feature"] != "css.property.page"),
        "{:?}",
        d.diagnostics()
    );
    assert_eq!(info["count"], 3, "{info}");
    assert_eq!(info["pages"][1]["width"], 600.0);
    assert_eq!(info["pages"][2]["width"], 300.0);
    assert_eq!(d.page_rects("w").unwrap()[0]["page"], 1);
}

/// Fuzz findings (renderer review #8). harness_html_pipeline seed 1 case 247: paginating deeply
/// nested auto-height blocks asked `content_from` twice per child (2^depth calls), and its nested
/// `h1`s (2em each) make a flow of thousands of pages, all built in each of up to six passes before
/// the page limit refused it. harness_dom_sequences seed 1 case 474: a `font-size:1e9px` flow of
/// some ten million pages, every band of which was materialised and named. Neither finished; each
/// case must now paginate (or be refused) well within a second.
#[test]
fn paginating_huge_or_deep_flows_is_bounded() {
    let unit = "<main><p class=\"eyebrow\">UI4ME · LIVE BITMAP</p><h1>Generstroke-width=\"2\" =\"m2 5 6 6 6-6\"/>t\" href=\"style.css\"></head><body>\n";
    let cases = [
        // The fuzz input: unclosed HTML in inline SVG foreign content.
        format!(
            "<svg xmlns>\"http:t\" href=\"style.css\"></head><body>\n{}",
            unit.repeat(17)
        ),
        format!("{}x", "<div>".repeat(60)),
        format!("{}<p>x</p>", "<section><div>".repeat(40)),
        "<div style=\"font-size:1e9px\"><p>Hello <b>world</b> text</p></div>".to_string(),
    ];
    let over_limit = [true, false, false, true];
    for (i, html) in cases.into_iter().enumerate() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .stack_size(crate::ENGINE_STACK_BYTES)
            .spawn(move || {
                let mut d = Document::parse(&html).unwrap();
                d.render(400, 300, 1.0).unwrap();
                let t0 = std::time::Instant::now();
                let r = d.paginate(None, None, false);
                let _ = tx.send((t0.elapsed(), r.err().map(|e| e.code)));
            })
            .unwrap();
        let bound = std::time::Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 1 });
        match rx.recv_timeout(bound * 2) {
            Ok((t, code)) => {
                assert!(t < bound, "case {i}: paginate took {t:?}");
                // More than MAX_PAGES pages is a limit error (code 5), not a partial result.
                assert_eq!(code, over_limit[i].then_some(5), "case {i}");
            }
            Err(_) => panic!("case {i}: paginate did not finish within {:?}", bound * 2),
        }
    }
}
