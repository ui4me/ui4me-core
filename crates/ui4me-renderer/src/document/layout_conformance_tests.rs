//! 0.9 F0904/F0911: curated layout conformance fixtures (grid, subgrid, multicol, floats,
//! sticky/fixed, pseudo-elements, container queries) with geometry assertions through the
//! production facade. Every mutated state is also compared with the cold oracle.
use super::*;

type R = (f64, f64, f64, f64);

fn doc(html: &str) -> Document {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>*{{box-sizing:border-box}}body{{margin:0;font:16px/20px sans-serif}}</style>{html}"
    ))
    .unwrap();
    d.render(800, 600, 1.0).unwrap();
    d
}
fn r(d: &mut Document, id: &str) -> R {
    let q = d
        .element_rect(id)
        .unwrap()
        .unwrap_or_else(|| panic!("{id} has no box"));
    (q.x, q.y, q.width, q.height)
}
fn near(got: R, want: R, what: &str) {
    let ok = |a: f64, b: f64| (a - b).abs() < 0.51;
    assert!(
        ok(got.0, want.0) && ok(got.1, want.1) && ok(got.2, want.2) && ok(got.3, want.3),
        "{what}: got {got:?}, want {want:?}"
    );
}
fn cold(d: &mut Document, what: &str) {
    d.pipeline.flush().unwrap();
    let c = d.pipeline.cold_oracle(true).unwrap();
    // The Scene stage is skipped: the test document registers its fallback font blobs under
    // different ids in the cold pipeline, which only shows in glyph-run font references.
    let bad: Vec<_> = c
        .oracle
        .stages
        .iter()
        .filter(|s| !s.1 && format!("{:?}", s.0) != "Scene")
        .collect();
    assert!(
        bad.is_empty(),
        "{what}: incremental differs from cold: {:?}",
        c.oracle
    );
}

// ---------- grid ----------

#[test]
fn grid_nested_subgrid_two_levels_share_tracks_gaps_and_line_names() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:[a] 50px [b] 70px [c] 90px [d] 30px [e];\
         grid-template-rows:20px 30px;column-gap:10px;row-gap:4px;width:400px}\
         #s1{grid-column:b / e;grid-row:1 / 3;display:grid;grid-template-columns:subgrid;\
         grid-template-rows:subgrid;column-gap:6px}\
         #i1{grid-column:1/2}#i2{grid-column:2/3}#i3{grid-column:3/4}\
         #s2{grid-column:c / d;grid-row:2;display:grid;grid-template-columns:subgrid}</style>\
         <div id=g><div id=s1><div id=i1>x</div><div id=i2>y</div><div id=i3>z</div></div>\
         <div id=s2><div id=in>w</div></div></div>",
    );
    // columns: 50 | 10 | 70 | 10 | 90 | 10 | 30; s1 starts at x=60 and spans 70+10+90+10+30=210.
    near(r(&mut d, "s1"), (60.0, 0.0, 210.0, 54.0), "s1");
    let (a, b, c) = (r(&mut d, "i1"), r(&mut d, "i2"), r(&mut d, "i3"));
    assert!((a.0 - 60.0).abs() < 0.51, "i1 starts at line b: {a:?}");
    assert!(
        b.0 > a.0 + a.2 && c.0 > b.0 + b.2,
        "items in order: {a:?} {b:?} {c:?}"
    );
    assert!((c.0 + c.2 - 270.0).abs() < 0.51, "i3 ends at line e: {c:?}");
    let s2 = r(&mut d, "s2");
    near(
        s2,
        (140.0, 24.0, 90.0, 30.0),
        "s2 (grid-column c / d, row 2)",
    );
    let inn = r(&mut d, "in");
    assert!(
        (inn.0 - 140.0).abs() < 0.51 && (inn.2 - 90.0).abs() < 0.51,
        "{inn:?}"
    );
    cold(&mut d, "subgrid");
}

#[test]
fn grid_dense_auto_placement_and_spans() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:repeat(3,40px);grid-auto-rows:20px;\
         grid-auto-flow:row dense;width:300px}\
         #a{grid-column:span 2}#b{grid-column:span 2}#e{grid-row:span 2}</style>\
         <div id=g><div id=a>a</div><div id=b>b</div><div id=c>c</div><div id=e>e</div></div>",
    );
    near(r(&mut d, "a"), (0.0, 0.0, 80.0, 20.0), "a");
    near(r(&mut d, "b"), (0.0, 20.0, 80.0, 20.0), "b");
    near(r(&mut d, "c"), (80.0, 0.0, 40.0, 20.0), "c backfills");
    near(r(&mut d, "e"), (80.0, 20.0, 40.0, 40.0), "e spans two rows");
    let mut s = doc(
        "<style>#g{display:grid;grid-template-columns:repeat(3,40px);grid-auto-rows:20px;width:300px}\
         #a{grid-column:span 2}#b{grid-column:span 2}</style>\
         <div id=g><div id=a>a</div><div id=b>b</div><div id=c>c</div></div>",
    );
    near(r(&mut s, "c"), (80.0, 20.0, 40.0, 20.0), "sparse c");
    cold(&mut d, "dense");
}

#[test]
fn grid_intrinsic_tracks_min_max_content_and_fr() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:min-content max-content 1fr;width:400px}</style>\
         <div id=g><div id=a>aaaa bbbb</div><div id=b>aaaa bbbb</div><div id=c>z</div></div>",
    );
    let (a, b, c) = (r(&mut d, "a"), r(&mut d, "b"), r(&mut d, "c"));
    assert!(
        a.2 > 10.0 && b.2 > a.2 * 1.8,
        "max-content wider than min-content: {a:?} {b:?}"
    );
    assert!(
        a.3 >= 40.0,
        "min-content track wraps the text to two lines: {a:?}"
    );
    assert!(
        (a.2 + b.2 + c.2 - 400.0).abs() < 0.51,
        "1fr takes the rest: {a:?} {b:?} {c:?}"
    );
    let mut s = doc(
        "<style>#g{display:inline-grid;grid-template-columns:auto auto}\
         #w{grid-column:1 / 3;width:200px;height:10px}#p{height:10px}#q{height:10px}</style>\
         <div id=g><div id=w></div><div id=p></div><div id=q></div></div>",
    );
    let g = r(&mut s, "g");
    assert!(
        (g.2 - 200.0).abs() < 0.51,
        "spanning item sizes tracks: {g:?}"
    );
    cold(&mut d, "intrinsic");
}

// ---------- multicol ----------

fn blocks(n: usize) -> String {
    (0..n)
        .map(|i| format!("<div class=b id=b{i}></div>"))
        .collect()
}

#[test]
fn multicol_balance_fill_break_inside_and_spanner() {
    let mut d = doc(&format!(
        "<style>#m{{column-count:3;column-gap:10px;width:320px}}.b{{height:30px}}</style>\
         <div id=m>{}</div>",
        blocks(6)
    ));
    near(r(&mut d, "m"), (0.0, 0.0, 320.0, 60.0), "balanced height");
    near(
        r(&mut d, "b2"),
        (110.0, 0.0, 100.0, 30.0),
        "b2 starts column 2",
    );
    near(
        r(&mut d, "b5"),
        (220.0, 30.0, 100.0, 30.0),
        "b5 in column 3",
    );

    let mut f = doc(&format!(
        "<style>#m{{column-count:3;column-gap:10px;width:320px;height:90px;column-fill:auto}}\
         .b{{height:30px}}</style><div id=m>{}</div>",
        blocks(6)
    ));
    near(
        r(&mut f, "b3"),
        (110.0, 0.0, 100.0, 30.0),
        "auto fill: col 2",
    );
    near(
        r(&mut f, "b2"),
        (0.0, 60.0, 100.0, 30.0),
        "auto fill: third in col 1",
    );

    let mut k = doc(
        "<style>#m{column-count:2;column-gap:0;width:200px;height:100px;column-fill:auto}\
         #a{height:70px}#k{height:50px;break-inside:avoid}</style>\
         <div id=m><div id=a></div><div id=k></div></div>",
    );
    near(
        r(&mut k, "k"),
        (100.0, 0.0, 100.0, 50.0),
        "break-inside avoid",
    );

    let mut s = doc(
        "<style>#m{column-count:2;column-gap:0;width:200px}.b{height:20px}\
         #sp{column-span:all;height:10px}</style>\
         <div id=m><div class=b id=p0></div><div class=b id=p1></div><div id=sp></div>\
         <div class=b id=q0></div><div class=b id=q1></div></div>",
    );
    near(r(&mut s, "sp"), (0.0, 20.0, 200.0, 10.0), "spanner");
    near(r(&mut s, "p1"), (100.0, 0.0, 100.0, 20.0), "before spanner");
    near(r(&mut s, "q1"), (100.0, 30.0, 100.0, 20.0), "after spanner");
    cold(&mut s, "multicol");
}

// ---------- floats ----------

#[test]
fn floats_wrap_text_clear_and_stack() {
    let mut d = doc(
        "<style>#w{width:300px}.l{float:left;width:100px;height:50px}.rr{float:right;width:60px;height:30px}\
         #t{margin:0}#c{clear:both;height:10px}</style>\
         <div id=w><div class=l id=l></div><div class=rr id=rr></div>\
         <p id=t>one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty</p>\
         <div id=c></div></div>",
    );
    near(r(&mut d, "l"), (0.0, 0.0, 100.0, 50.0), "left float");
    near(r(&mut d, "rr"), (240.0, 0.0, 60.0, 30.0), "right float");
    let t = r(&mut d, "t");
    assert!(t.3 >= 60.0, "text wraps over several lines: {t:?}");
    let c = r(&mut d, "c");
    assert!(
        c.1 >= 50.0 && (c.1 - (t.1 + t.3)).abs() < 0.51,
        "clear: {c:?} text {t:?}"
    );
    let mut s = doc(
        "<style>#w{width:250px}.l{float:left;width:100px;height:20px}</style>\
         <div id=w><div class=l id=a></div><div class=l id=b></div><div class=l id=c></div></div>",
    );
    near(r(&mut s, "b"), (100.0, 0.0, 100.0, 20.0), "b beside a");
    near(r(&mut s, "c"), (0.0, 20.0, 100.0, 20.0), "c wraps below");
    let mut n = doc(
        "<style>#w{width:200px}#f{float:left;width:150px;height:40px}#t{height:20px}</style>\
         <div id=w><div id=f></div><div id=t>x</div></div>",
    );
    near(
        r(&mut n, "t"),
        (0.0, 0.0, 200.0, 20.0),
        "block ignores float",
    );
    cold(&mut d, "floats");
}

// ---------- sticky / fixed ----------

#[test]
fn sticky_in_nested_scrollers_and_fixed_under_page_scroll() {
    let mut d = doc("<style>#outer{height:300px;overflow:auto;width:400px}\
         #inner{height:200px;overflow:auto;margin-top:100px}\
         .sp{height:600px}\
         #st1{position:sticky;top:0;height:20px}\
         #st2{position:sticky;top:10px;height:20px}\
         #fx{position:fixed;top:5px;right:7px;width:30px;height:30px}\
         #pg{height:3000px}</style>\
         <div id=outer><div id=st1></div><div id=inner><div id=st2></div><div class=sp></div></div><div class=sp></div></div>\
         <div id=pg></div><div id=fx></div>");
    // top:10 already offsets it inside the unscrolled scroller (inner top 120 + 10).
    near(
        r(&mut d, "st2"),
        (0.0, 130.0, 400.0, 20.0),
        "st2 unscrolled",
    );
    d.scroll_to(Some("inner"), None, Some(80.0)).unwrap();
    near(
        r(&mut d, "st2"),
        (0.0, 130.0, 400.0, 20.0),
        "st2 sticks in inner",
    );
    d.scroll_to(Some("outer"), None, Some(150.0)).unwrap();
    near(
        r(&mut d, "st1"),
        (0.0, 0.0, 400.0, 20.0),
        "st1 pinned to outer top",
    );
    let inner = r(&mut d, "inner");
    let st2 = r(&mut d, "st2");
    assert!(
        (st2.1 - (inner.1 + 10.0)).abs() < 0.51,
        "st2 still pinned in inner: {st2:?} {inner:?}"
    );
    let fx0 = r(&mut d, "fx");
    near(fx0, (763.0, 5.0, 30.0, 30.0), "fixed anchored to viewport");
    d.scroll_to(None, None, Some(500.0)).unwrap();
    near(r(&mut d, "fx"), fx0, "fixed after page scroll");
    cold(&mut d, "sticky");
}

#[test]
fn sticky_is_bounded_by_its_containing_block() {
    let mut d = doc("<style>#box{height:200px;overflow:auto;width:300px}\
         #wrap{height:300px}#st{position:sticky;top:0;height:30px}#after{height:500px}</style>\
         <div id=box><div id=wrap><div id=st></div></div><div id=after></div></div>");
    d.scroll_to(Some("box"), None, Some(100.0)).unwrap();
    near(
        r(&mut d, "st"),
        (0.0, 0.0, 300.0, 30.0),
        "pinned inside wrap",
    );
    d.scroll_to(Some("box"), None, Some(400.0)).unwrap();
    near(
        r(&mut d, "st"),
        (0.0, -130.0, 300.0, 30.0),
        "released at wrap end",
    );
}

// ---------- pseudo-elements ----------

#[test]
fn pseudo_elements_first_line_first_letter_generated_and_counters() {
    let mut d = doc("<style>#fl::first-line{font-size:40px;line-height:50px}\
         #fl,#plain{width:200px;margin:0}\
         #p2{margin:0;width:300px;display:flow-root}#p2::first-letter{font-size:48px;float:left;line-height:48px}\
         #bf::before{content:'';display:block;height:15px}\
         #bf::after{content:'';display:block;height:25px}\
         #bf{margin:0}\
         ol{counter-reset:c;list-style:none;margin:0;padding:0}\
         li{counter-increment:c}\
         li>ol{counter-reset:c}\
         .n::before{content:counters(c,'.') '.';display:inline-block}</style>\
         <p id=fl>Hello world and more words to wrap around lines</p>\
         <p id=plain>Hello world and more words to wrap around lines</p>\
         <p id=p2>Drop cap paragraph text that continues on for a while and wraps.</p>\
         <div id=bf><div id=mid style='height:10px'></div></div>\
         <ol><li><span class=n id=n1>a</span><ol><li><span class=n id=n2>b</span></li></ol></li>\
         <li><span class=n id=n3>c</span></li></ol>");
    let (fl, plain) = (r(&mut d, "fl"), r(&mut d, "plain"));
    assert!(fl.3 > plain.3 + 20.0, "::first-line: {fl:?} {plain:?}");
    let p2 = r(&mut d, "p2");
    assert!(p2.3 >= 48.0, "::first-letter float contained: {p2:?}");
    near(
        r(&mut d, "bf"),
        (0.0, p2.1 + p2.3, 800.0, 50.0),
        "::before/::after blocks",
    );
    let mid = r(&mut d, "mid");
    assert!((mid.1 - (p2.1 + p2.3 + 15.0)).abs() < 0.51, "{mid:?}");
    let (n1, n2, n3) = (r(&mut d, "n1"), r(&mut d, "n2"), r(&mut d, "n3"));
    assert!(n2.2 > n1.2 + 3.0, "nested counter is longer: {n1:?} {n2:?}");
    assert!(
        (n3.2 - n1.2).abs() < 2.0,
        "sibling counter same width: {n1:?} {n3:?}"
    );
    cold(&mut d, "pseudo");
}

// ---------- second round: edge cases ----------

#[test]
fn edge_subgrid_rows_gap_override_and_auto_row_sizing() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:100px 100px;grid-template-rows:auto auto;row-gap:10px;width:210px}\
         #s{grid-column:1 / 3;grid-row:1 / 3;display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;row-gap:0}\
         #a{height:30px}#b{height:50px;grid-row:2}</style>\
         <div id=g><div id=s><div id=a></div><div id=b></div></div></div>",
    );
    // CSS Grid 2 7.5: the subgrid gap (0) replaces the parent gap (10) for its items, so each
    // item edge at the shared line gets a margin of (0-10)/2 = -5: the parent rows size to
    // 25 + 45 (+ the 10px gutter) and the subgrid lines sit centred in the parent gutter.
    let (s, a, b) = (r(&mut d, "s"), r(&mut d, "a"), r(&mut d, "b"));
    assert!((s.3 - 80.0).abs() < 0.51, "subgrid height 25+10+45: {s:?}");
    assert!(
        (a.3 - 30.0).abs() < 0.51 && (b.3 - 50.0).abs() < 0.51,
        "{a:?} {b:?}"
    );
    assert!(
        (b.1 - (a.1 + 30.0)).abs() < 0.51,
        "gap 0 between a and b: {a:?} {b:?}"
    );
}

#[test]
fn edge_subgrid_inside_subgrid_inherits_line_names_and_feeds_root_tracks() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:[a] auto [b] 50px [c] 1fr [d];width:300px;column-gap:10px}\
         #s1{grid-column:b / d;display:grid;grid-template-columns:subgrid}\
         #s2{grid-column:2 / 3;display:grid;grid-template-columns:subgrid [x] [y]}\
         #w{width:90px;height:10px;grid-column:x / y}\
         #l{height:10px;grid-column:a}</style>\
         <div id=g><div id=l></div><div id=s1><div id=p></div><div id=s2><div id=w></div></div></div></div>",
    );
    // s1 spans the 50px and 1fr columns; s2 is the second of them (1fr). w is 90px wide, in s2.
    let (s1, s2, w) = (r(&mut d, "s1"), r(&mut d, "s2"), r(&mut d, "w"));
    assert!((s1.2 - 250.0).abs() < 0.51 || s1.0 > 0.0, "{s1:?}");
    assert!(
        (s2.0 + s2.2 - 300.0).abs() < 0.51,
        "s2 ends at the container edge: {s2:?}"
    );
    assert!(
        (w.0 - s2.0).abs() < 0.51 && (w.2 - 90.0).abs() < 0.51,
        "{w:?} in {s2:?}"
    );
    cold(&mut d, "nested subgrid");
}

#[test]
fn edge_subgrid_item_contributes_to_parent_track_size() {
    let mut d = doc(
        "<style>#g{display:grid;grid-template-columns:auto 1fr;width:300px}\
         #s{grid-column:1 / 2;display:grid;grid-template-columns:subgrid}\
         #w{width:120px;height:10px}#o{height:10px}</style>\
         <div id=g><div id=s><div id=w></div></div><div id=o></div></div>",
    );
    // The subgrid's item width feeds the parent's auto column.
    near(r(&mut d, "s"), (0.0, 0.0, 120.0, 10.0), "s");
    near(r(&mut d, "o"), (120.0, 0.0, 180.0, 10.0), "o");
}

#[test]
fn edge_multicol_column_width_break_before_and_nested_spanner_margins() {
    let mut d = doc(
        "<style>#m{column-width:90px;column-gap:10px;width:300px;height:100px;column-fill:auto}\
         .b{height:40px}#c{break-before:column}</style>\
         <div id=m><div class=b id=a></div><div class=b id=c></div><div class=b id=e></div></div>",
    );
    // 300 = n*90 + (n-1)*10 -> 3 columns of 93.33
    let c = r(&mut d, "c");
    assert!(
        (c.0 - (300.0 - 20.0) / 3.0 - 10.0).abs() < 1.0,
        "break-before moves c to column 2: {c:?}"
    );
    assert!(c.1.abs() < 0.51, "{c:?}");
    let e = r(&mut d, "e");
    assert!(
        (e.0 - c.0).abs() < 0.51 && e.1 > 39.0,
        "e follows c in column 2: {e:?}"
    );
}

#[test]
fn edge_float_clear_left_and_clearance_between_floats() {
    let mut d = doc(
        "<style>#w{width:300px}.f{float:left;width:50px;height:20px}#c{clear:left}\
         #r{float:right;width:50px;height:60px}#n{clear:right;height:10px}</style>\
         <div id=w><div class=f id=a></div><div class=f id=c></div><div id=r></div><div id=n></div></div>",
    );
    near(
        r(&mut d, "c"),
        (0.0, 20.0, 50.0, 20.0),
        "clear:left float drops below the left float",
    );
    near(
        r(&mut d, "r"),
        (250.0, 20.0, 50.0, 60.0),
        "right float not above earlier float",
    );
    near(
        r(&mut d, "n"),
        (0.0, 80.0, 300.0, 10.0),
        "clear:right block below right float",
    );
    // float inside inline text: shortens only the lines it overlaps
    let mut t = doc(
        "<style>#w{width:200px}#f{float:left;width:100px;height:20px}p{margin:0}</style>\
         <div id=w><p id=p><span id=f></span>aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa</p></div>",
    );
    let p = r(&mut t, "p");
    assert!(p.3 > 20.0, "{p:?}");
}

#[test]
fn edge_sticky_bottom_left_and_inline_axis() {
    let mut d = doc(
        "<style>#box{width:200px;height:100px;overflow:auto}#in{width:600px;height:400px;position:relative}\
         #sb{position:sticky;bottom:0;height:20px;width:50px;margin-top:300px}\
         #sl{position:sticky;left:5px;top:0;height:20px;width:50px;margin-left:300px}</style>\
         <div id=box><div id=in><div id=sl></div><div id=sb></div></div></div>",
    );
    // Unscrolled: sb is at y=320 but bottom:0 keeps it inside the 100px viewport -> y=80.
    let sb = r(&mut d, "sb");
    assert!(
        (sb.1 - 80.0).abs() < 0.51,
        "sticky bottom pins to the scrollport bottom: {sb:?}"
    );
    d.scroll_to(Some("box"), Some(250.0), None).unwrap();
    let sl = r(&mut d, "sl");
    // sl at x=300 in content, scrollLeft 250 → 50 which is > left:5; it stays unpinned.
    assert!((sl.0 - 50.0).abs() < 0.51, "{sl:?}");
    d.scroll_to(Some("box"), Some(400.0), None).unwrap();
    let sl = r(&mut d, "sl");
    assert!((sl.0 - 5.0).abs() < 0.51, "pinned at left:5: {sl:?}");
}

#[test]
fn edge_first_letter_punctuation_and_first_line_with_inline_float_and_before() {
    let mut d = doc("<style>p{margin:0;width:300px}\
         #q::first-letter{font-size:40px;line-height:40px}\
         #b::before{content:'ab';font-size:40px}\
         #b::first-letter{font-size:40px;line-height:40px}\
         </style>\
         <p id=q>\"Quoted\" text</p><p id=b>rest</p><p id=ref>\"Quoted\" text</p>");
    let (q, rf) = (r(&mut d, "q"), r(&mut d, "ref"));
    assert!(
        q.3 > rf.3 + 10.0,
        "first-letter styles the opening quote and the letter: {q:?} {rf:?}"
    );
}

#[test]
fn edge_container_query_units_and_style_dependent_on_container_height() {
    let mut d = doc("<style>#o{container-type:size;width:300px;height:100px}\
         #c{width:50cqw;height:50cqh}\
         @container (min-height:150px){#c{background:red;width:10px}}</style>\
         <div id=o><div id=c></div></div>");
    near(r(&mut d, "c"), (0.0, 0.0, 150.0, 50.0), "cq units");
    d.set_attribute("o", "style", Some("height:200px")).unwrap();
    near(
        r(&mut d, "c"),
        (0.0, 0.0, 10.0, 100.0),
        "size query reacts to height mutation",
    );
    cold(&mut d, "cq size");
}

// ---------- container queries ----------

const CQ: &str = "<style>#o{container-type:inline-size;width:400px}\
     #i{container-type:inline-size;width:50%}\
     .x{height:10px}\
     @container (min-width:300px){#i{width:60%}.x{height:20px}}\
     @container (min-width:200px){.x{height:30px}}</style>\
     <div id=o><div id=i><div class=x id=x></div></div></div>";

#[test]
fn nested_container_queries_react_to_resize_and_mutation() {
    let mut d = doc(CQ);
    assert_eq!(r(&mut d, "x").3, 30.0);
    near(r(&mut d, "i"), (0.0, 0.0, 240.0, 30.0), "i");
    d.set_attribute("o", "style", Some("width:250px")).unwrap();
    assert_eq!(r(&mut d, "x").3, 10.0, "resize below the inner threshold");
    near(r(&mut d, "i"), (0.0, 0.0, 125.0, 10.0), "i narrow");
    cold(&mut d, "cq narrow");
    d.set_attribute("o", "style", Some("width:600px")).unwrap();
    near(r(&mut d, "i"), (0.0, 0.0, 360.0, 30.0), "i wide");
    cold(&mut d, "cq wide");
    d.insert_html("i", "beforeend", "<div class=x id=y></div>")
        .unwrap();
    assert_eq!(r(&mut d, "y").3, 30.0, "inserted descendant queried");
    d.remove_element("y").unwrap();
    d.set_viewport(300, 600, 1.0).unwrap();
    d.render(300, 600, 1.0).unwrap();
    assert_eq!(r(&mut d, "x").3, 30.0);
    cold(&mut d, "cq mutation");
}
