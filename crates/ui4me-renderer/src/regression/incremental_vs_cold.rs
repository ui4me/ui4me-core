// owner: L-PERF-b1 (beta B2) — incremental results equal cold results after representative mutations
// (005 G-INVALIDATION; dirty.rs invariant): a table, a flex list and block text are mutated by text,
// class, style, structure and attribute changes; after every update the cold oracle (fresh style,
// font, layout and scene state over the live DOM; frames byte-identical) must agree with the live
// committed set. The reuse paths (shaped-paragraph content cache, line-break memo, unchanged-box-tree
// layout reuse, paint-only styles) are the ones under test: a reuse that changed a result shows up as
// a difference to the cold recompute.
use crate::pipeline::state::{DocumentPipeline, PipelineConfig};
use std::path::Path;

const HTML: &str = r#"<!DOCTYPE html><html><head><style>
body{margin:8px;font-family:"Noto Sans";font-size:14px}
table{border-collapse:collapse;width:100%}
td,th{padding:4px;border-bottom:1px solid #ccc}
tr.sel td{background:#cde}
tr.hot td{color:#c00}
tr.fat td{padding:12px}
ul.l{display:flex;flex-direction:column;list-style:none;margin:0;padding:0}
ul.l li{padding:6px;border:1px solid #999}
ul.l li.on{background:#9c9}
.card{box-shadow:0 0 4px #000;padding:8px;margin:6px 0}
.card.lift{box-shadow:0 4px 8px #333}
.float{float:left;width:40px;height:20px;background:#fa0}
</style></head><body>
<h1 id="title">Report</h1>
<div class="card" id="card"><p id="para">Some wrapped paragraph text that is long enough to need more than one line in a narrow viewport, with <b>bold</b> and <i>italic</i> parts.</p></div>
<div><span class="float"></span><p id="floated">Text beside a float that wraps around the float box when it is long enough.</p></div>
<table id="t"><thead><tr><th>#</th><th>Name</th><th>Total</th></tr></thead><tbody id="rows">
<tr id="r0"><td>0</td><td id="n0">Alpha</td><td>10.00</td></tr>
<tr id="r1"><td>1</td><td id="n1">Bravo bravo</td><td>20.50</td></tr>
<tr id="r2"><td>2</td><td id="n2">Charlie</td><td>7.25</td></tr>
<tr id="r3"><td>3</td><td id="n3">Delta delta delta</td><td>99.99</td></tr>
</tbody></table>
<ul class="l" id="list"><li id="i0">one</li><li id="i1">two two</li><li id="i2">three</li></ul>
<input id="in" type="text" value="abc">
</body></html>"#;

fn load() -> DocumentPipeline {
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
    DocumentPipeline::load_html(
        HTML,
        PipelineConfig {
            viewport_width: 360.0,
            viewport_height: 480.0,
            scale: 1.0,
            font_files: vec![font],
        },
    )
    .unwrap()
}

fn check(p: &mut DocumentPipeline, what: &str) {
    p.flush().unwrap();
    let r = p.cold_oracle(true).unwrap();
    assert!(
        r.equal(),
        "{what}: incremental differs from cold: {:?} frames_equal={:?}",
        r.oracle,
        r.frames_equal
    );
}

fn id(p: &DocumentPipeline, name: &str) -> ui4me_engine_types::ids::NodeId {
    p.element_by_id(name)
        .unwrap_or_else(|| panic!("no #{name}"))
}

#[test]
fn text_class_style_and_structure_changes_equal_cold() {
    let mut p = load();
    check(&mut p, "initial");
    // Cell text (width of the column may change).
    for (i, text) in [
        "Alpha",
        "A much longer name than before that wraps the cell",
        "x",
        "Bravo bravo",
    ]
    .iter()
    .enumerate()
    {
        let n = id(&p, "n1");
        p.set_text_content(n, text).unwrap();
        check(&mut p, &format!("cell text #{i}"));
    }
    // Paragraph text: shaped paragraph content changes, then returns to an earlier content (cache hit).
    let para = id(&p, "para");
    p.set_text_content(para, "short").unwrap();
    check(&mut p, "para short");
    p.set_text_content(para, "Some wrapped paragraph text that is long enough to need more than one line in a narrow viewport, with bold and italic parts.").unwrap();
    check(&mut p, "para long again");
    // Class toggles: background only (reusable layout), text colour, padding (layout).
    let r2 = id(&p, "r2");
    for class in ["sel", "hot", "fat", "sel", "fat", "hot"] {
        p.toggle_class(r2, class).unwrap();
        check(&mut p, &format!("toggle {class}"));
    }
    let card = id(&p, "card");
    p.toggle_class(card, "lift").unwrap();
    check(&mut p, "box-shadow value change");
    let i1 = id(&p, "i1");
    p.toggle_class(i1, "on").unwrap();
    check(&mut p, "list item background");
    // Inline style.
    p.set_attribute(i1, "style", "padding: 20px").unwrap();
    check(&mut p, "inline style padding");
    p.set_attribute(i1, "style", "color: red").unwrap();
    check(&mut p, "inline style colour");
    // Structure: append, insert-like replace, remove.
    let list = id(&p, "list");
    for k in 0..3 {
        let li = p.create_element("li").unwrap();
        p.set_text_content(li, &format!("added item {k} with text"))
            .unwrap();
        p.append_child(list, li).unwrap();
        check(&mut p, &format!("append li {k}"));
    }
    let rows = id(&p, "rows");
    let tr = p.create_element("tr").unwrap();
    for text in ["4", "Echo", "1.00"] {
        let td = p.create_element("td").unwrap();
        p.set_text_content(td, text).unwrap();
        p.append_child(tr, td).unwrap();
    }
    p.append_child(rows, tr).unwrap();
    check(&mut p, "append table row");
    // Control value and other attributes.
    let input = id(&p, "in");
    p.set_attribute(input, "value", "a longer value than before")
        .unwrap();
    check(&mut p, "input value attribute");
    p.set_attribute(input, "type", "password").unwrap();
    check(&mut p, "input type");
    let title = id(&p, "title");
    p.set_attribute(title, "hidden", "").unwrap();
    check(&mut p, "hidden attribute");
}

#[test]
fn unchanged_box_tree_reuses_layout_and_equals_cold() {
    let mut p = load();
    p.flush().unwrap();
    let i0 = id(&p, "i0");
    let before = p.layout_reuses();
    // A background-colour class change moves no layout input: the committed layout is reused.
    p.toggle_class(i0, "on").unwrap();
    check(&mut p, "background change");
    p.toggle_class(i0, "on").unwrap();
    check(&mut p, "background change back");
    let card = id(&p, "card");
    p.toggle_class(card, "lift").unwrap();
    check(&mut p, "shadow change");
    // A text colour change takes the box-tree comparison path (colour is a box input).
    let r1 = id(&p, "r1");
    p.toggle_class(r1, "hot").unwrap();
    check(&mut p, "text colour change");
    assert!(
        p.layout_reuses() > before,
        "paint-only style changes reuse the committed layout (reuses {} -> {})",
        before,
        p.layout_reuses()
    );
}

#[test]
fn reuse_and_no_reuse_publish_the_same_scene() {
    let mut a = load();
    let mut b = load();
    b.disable_layout_reuse_for_testing();
    b.disable_animation_paint_path_for_testing();
    for p in [&mut a, &mut b] {
        p.flush().unwrap();
    }
    for step in 0..6 {
        for p in [&mut a, &mut b] {
            // `lift` changes a shadow's value only (reusable layout); `sel` adds a background (not reusable).
            let n = p
                .element_by_id(if step % 2 == 0 { "card" } else { "r2" })
                .unwrap();
            p.toggle_class(n, if step % 2 == 0 { "lift" } else { "sel" })
                .unwrap();
            p.flush().unwrap();
        }
        let fa = a.cold_oracle(true).unwrap();
        let fb = b.cold_oracle(true).unwrap();
        assert!(fa.equal() && fb.equal(), "step {step}");
        assert_eq!(
            fa.frame_hashes, fb.frame_hashes,
            "step {step}: frames of the reuse and no-reuse pipelines differ"
        );
    }
    assert!(
        a.layout_reuses() > 0,
        "the shadow-value toggles reuse the layout"
    );
    assert_eq!(b.layout_reuses(), 0);
}

// ---- L-PERF-b2: every reuse path (damage raster, scene reuse, layout reuse) against the cold oracle
// and against a pipeline with every reuse disabled (full raster): frames byte-identical, display lists
// and hit tests equal.

const RICH: &str = r#"<!DOCTYPE html><html><head><style>
body{margin:8px;font-family:"Noto Sans";font-size:14px}
table{border-collapse:collapse;width:100%}
td{padding:3px;border-bottom:1px solid #ccc}
tr:hover td{background:#eef}
tr.sel td{background:#cde}
tr.fat td{padding:10px}
tr.ink td{color:#a00}
#modal{display:none;position:fixed;left:40px;top:60px;width:220px;padding:12px;background:#fff;box-shadow:0 6px 18px rgba(0,0,0,.5);border-radius:6px}
#modal.show{display:block}
#backdrop{display:none;position:fixed;left:0;top:0;right:0;bottom:0;background:#000;opacity:.4}
#backdrop.show{display:block}
#scroller{height:60px;overflow:auto;border:1px solid #888}
input:focus{outline:2px solid #36c}
.fade{transition:background-color 400ms linear;background:#fff;padding:4px}
.fade.on{background:#f80}
.fade2{transition:background-color 400ms linear,opacity 400ms linear;padding:4px}
.fade2.on{background:#08f;opacity:.3}
.ghost{opacity:.5;position:relative}
.rot{transform:rotate(3deg);width:80px;height:20px;background:#9c9}
</style></head><body>
<div id="nav" style="padding:4px;background:#345;color:#fff">Navbar <span id="badge">3</span></div>
<div id="fade" class="fade">transition</div>
<div id="fade2" class="fade2">transition from a transparent background</div>
<div class="ghost" id="ghost"><span id="ghosttext">translucent</span> <b>group</b></div>
<div class="rot" id="rot"></div>
<div id="scroller"><p>one</p><p>two</p><p>three</p><p>four</p><p>five</p></div>
<input id="in" type="text" value="hello world"> <select id="sel"><option>Alpha</option><option selected>Beta</option><option>Gamma</option></select>
<table><tbody id="rows">
<tr id="r0"><td>0</td><td id="n0">Alpha</td><td>10.00</td></tr>
<tr id="r1"><td>1</td><td id="n1">Bravo</td><td>20.50</td></tr>
<tr id="r2"><td>2</td><td id="n2">Charlie</td><td>7.25</td></tr>
<tr id="r3"><td>3</td><td id="n3">Delta</td><td>99.99</td></tr>
<tr id="r4"><td>4</td><td id="n4">Echo</td><td>1.00</td></tr>
</tbody></table>
<div id="backdrop"></div><div id="modal">Dialog <button id="ok">OK</button></div>
</body></html>"#;

fn load_page(html: &str) -> DocumentPipeline {
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
    DocumentPipeline::load_html(
        html,
        PipelineConfig {
            viewport_width: 400.0,
            viewport_height: 420.0,
            scale: 1.0,
            font_files: vec![font],
        },
    )
    .unwrap()
}

/// The optimised pipeline `a` and the reference `b` (no layout reuse, no paint-only paths, full
/// rasters), driven through the same mutations.
struct Pair {
    a: DocumentPipeline,
    b: DocumentPipeline,
    keys: [ui4me_document::ui_state::UiStateKey; 2],
}

impl Pair {
    fn new() -> Pair {
        Pair::with(RICH)
    }

    fn with(html: &str) -> Pair {
        let mut a = load_page(html);
        // The optimised pipeline with scene reuse on (015 PERF-2; off by default until PERF-4).
        a.set_scene_reuse(true);
        let mut b = load_page(html);
        b.disable_layout_reuse_for_testing();
        b.disable_animation_paint_path_for_testing();
        b.disable_damage_raster_for_testing();
        let ka = a
            .mutate(|d| Ok(d.take_ui_state_key().expect("key")))
            .unwrap();
        let kb = b
            .mutate(|d| Ok(d.take_ui_state_key().expect("key")))
            .unwrap();
        let mut pair = Pair {
            a,
            b,
            keys: [ka, kb],
        };
        pair.compare("initial", true);
        pair
    }

    fn each(
        &mut self,
        mut f: impl FnMut(&mut DocumentPipeline, &ui4me_document::ui_state::UiStateKey),
    ) {
        f(&mut self.a, &self.keys[0]);
        f(&mut self.b, &self.keys[1]);
    }

    fn flags(&mut self, el: &str, flags: ui4me_document::ui_state::UiFlags) {
        self.each(|p, k| {
            let n = id(p, el);
            p.mutate(|d| d.ui_state_writer(k)?.set_flags(n, flags))
                .unwrap();
        });
    }

    /// Frames of `a` (damage raster) and `b` (full raster) byte-identical, display lists and hit tests
    /// over a grid equal, and (unless an animation is mid-flight, which a cold restyle does not
    /// reproduce) `a` equal to the cold oracle.
    fn compare(&mut self, what: &str, cold: bool) {
        let fa = self.a.render_headless().unwrap();
        let fb = self.b.render_headless().unwrap();
        assert_eq!(
            (fa.pixels.width, fa.pixels.height),
            (fb.pixels.width, fb.pixels.height),
            "{what}: frame size"
        );
        if fa.pixels.data != fb.pixels.data {
            let w = fa.pixels.width as usize;
            let first = fa
                .pixels
                .data
                .chunks(4)
                .zip(fb.pixels.data.chunks(4))
                .position(|(x, y)| x != y)
                .unwrap();
            panic!(
                "{what}: damage-raster frame differs from the full raster (first pixel {},{})",
                first % w,
                first / w
            );
        }
        let (sa, sb) = (
            self.a.committed_result().unwrap().committed.scene.clone(),
            self.b.committed_result().unwrap().committed.scene.clone(),
        );
        assert_eq!(
            sa.display_list.items, sb.display_list.items,
            "{what}: display lists differ"
        );
        assert_eq!(
            sa.display_list.scopes, sb.display_list.scopes,
            "{what}: scope trees differ"
        );
        use crate::scene::hit_index::{HitOptions, ViewportPoint};
        for y in (0..420).step_by(9) {
            for x in (0..400).step_by(13) {
                let pt = ViewportPoint {
                    x: f64::from(x) + 0.5,
                    y: f64::from(y) + 0.5,
                };
                let o = HitOptions {
                    all: true,
                    include_scrollbars: true,
                };
                assert_eq!(
                    format!("{:?}", sa.hit_test(pt, o).hits),
                    format!("{:?}", sb.hit_test(pt, o).hits),
                    "{what}: hit test at {x},{y}"
                );
            }
        }
        if cold {
            let r = self.a.cold_oracle(true).unwrap();
            assert!(
                r.equal(),
                "{what}: incremental differs from cold: {:?} frames_equal={:?}",
                r.oracle,
                r.frames_equal
            );
        }
    }
}

#[test]
fn every_reuse_path_equals_cold_and_full_raster() {
    use ui4me_document::ui_state::UiFlags;
    let mut t = Pair::new();
    // Text edits: a cell (column width moves) and back, a badge.
    for (i, text) in [
        "Renamed",
        "A much longer name that changes the column width",
        "Bravo",
    ]
    .iter()
    .enumerate()
    {
        t.each(|p, _| {
            let n = id(p, "n1");
            p.set_text_content(n, text).unwrap();
        });
        t.compare(&format!("cell text #{i}"), true);
    }
    t.each(|p, _| {
        let n = id(p, "badge");
        p.set_text_content(n, "42").unwrap();
    });
    t.compare("badge text", true);
    // Class toggles: paint only, layout, text colour.
    for class in ["sel", "fat", "ink", "sel", "fat", "ink"] {
        t.each(|p, _| {
            let n = id(p, "r2");
            p.toggle_class(n, class).unwrap();
        });
        t.compare(&format!("row class {class}"), true);
    }
    // Inline style.
    t.each(|p, _| {
        let n = id(p, "nav");
        p.set_attribute(n, "style", "padding:10px;background:#543;color:#ff0")
            .unwrap();
    });
    t.compare("inline style", true);
    t.each(|p, _| {
        let n = id(p, "ghosttext");
        p.set_attribute(n, "style", "color:#00f").unwrap();
    });
    t.compare("text inside an opacity group", true);
    // Insert and remove rows.
    t.each(|p, _| {
        let rows = id(p, "rows");
        let tr = p.create_element("tr").unwrap();
        p.set_attribute(tr, "id", "rx").unwrap();
        for text in ["5", "Foxtrot", "3.50"] {
            let td = p.create_element("td").unwrap();
            p.set_text_content(td, text).unwrap();
            p.append_child(tr, td).unwrap();
        }
        p.append_child(rows, tr).unwrap();
    });
    t.compare("append row", true);
    t.each(|p, _| {
        let (rows, r1) = (id(p, "rows"), id(p, "r1"));
        p.mutate(|d| d.remove_child(rows, r1)).unwrap();
    });
    t.compare("remove row", true);
    // Modal show / hide via class.
    for on in [true, false, true, false] {
        t.each(|p, _| {
            for el in ["modal", "backdrop"] {
                let n = id(p, el);
                if p.has_class(n, "show").unwrap() != on {
                    p.toggle_class(n, "show").unwrap();
                }
            }
        });
        t.compare(&format!("modal {on}"), true);
    }
    // Hover, focus with caret, select popup.
    for el in ["r0", "r2", "r3"] {
        t.flags(el, UiFlags::HOVER);
        t.compare(&format!("hover {el}"), true);
        t.flags(el, UiFlags::empty());
    }
    t.compare("hover off", true);
    t.flags("in", UiFlags::FOCUS);
    t.compare("focus", true);
    for (k, (start, visible)) in [(3u32, true), (3, false), (5, true), (11, true)]
        .iter()
        .enumerate()
    {
        t.each(|p, _| {
            let n = id(p, "in");
            let sel = ui4me_document::control_state::Selection {
                start: *start,
                end: *start,
                direction: Default::default(),
            };
            p.mutate(|d| d.control_state_writer().set_selection(n, Some(sel)))
                .unwrap();
            p.note_caret_selection_paint(n).unwrap();
            p.set_caret_state(*visible, false, n).unwrap();
        });
        t.compare(&format!("caret #{k}"), true);
    }
    t.flags("in", UiFlags::empty());
    t.flags("sel", UiFlags::PICKER_OPEN);
    t.compare("select popup open", true);
    t.flags("sel", UiFlags::empty());
    t.compare("select popup closed", true);
    // Scroll an element.
    for y in [20.0, 45.0, 0.0] {
        t.each(|p, _| {
            let n = id(p, "scroller");
            p.scroll_to(
                crate::layout::coordinator::overflow::ScrollTarget::Node(n),
                0.0,
                y,
            )
            .unwrap();
        });
        t.compare(&format!("scroll {y}"), true);
    }
    // A transition mid-flight (a cold restyle starts none: compared with the reference pipeline only).
    t.each(|p, _| {
        for el in ["fade", "fade2"] {
            let n = id(p, el);
            p.toggle_class(n, "on").unwrap();
        }
        p.flush().unwrap();
    });
    for k in 0..4 {
        t.each(|p, _| p.advance_clock(100.0).unwrap());
        t.compare(&format!("transition tick {k}"), false);
    }
    t.each(|p, _| p.advance_clock(400.0).unwrap());
    t.compare("transition done", true);
    // Viewport resize and scale change.
    t.each(|p, _| {
        p.set_environment(360.0, 420.0, 1.0).unwrap();
    });
    t.compare("resize", true);
    t.each(|p, _| {
        p.set_environment(360.0, 420.0, 1.5).unwrap();
    });
    t.compare("scale", true);
    t.each(|p, _| {
        let n = id(p, "n0");
        p.set_text_content(n, "after scale").unwrap();
    });
    t.compare("text after scale", true);
    assert!(
        t.a.partial_frames() > 10,
        "the damage raster served most frames ({})",
        t.a.partial_frames()
    );
    assert_eq!(t.b.partial_frames(), 0);
}

// ---- L-PERF-b3: paint-only properties out of layout. Text colour reaches collapsed borders
// (`currentColor`), text decorations (`currentColor` underlines, also on a pseudo-element) and
// backgrounds; a hover / `:active` colour change must reuse the committed layout and still paint
// exactly what a cold pass paints.

const COLOURS: &str = r##"<!DOCTYPE html><html><head><style>
body{margin:8px;font-family:"Noto Sans";font-size:14px;color:#222}
nav{display:flex;flex-direction:column;width:120px;background:#234;padding:4px}
nav button{color:#abc;background:none;border:0;text-align:left;padding:4px}
nav button:hover{color:#fff}
nav button:active{color:#ff0;background:#06c}
nav button.on{color:#fff;background:#06c}
table{border-collapse:collapse;width:100%}
td{padding:3px;border-bottom:1px solid;border-left:2px solid}
tr:hover td{color:#c00}
tr:active td{color:rgba(0,0,200,.5)}
tr.alt td{color:#060}
a{color:#06c;text-decoration:underline}
a:hover{color:#c60}
a:active{background:#ff0}
p.d::after{content:" after";text-decoration:underline;color:#909}
p.d:hover::after{color:#090}
span.bg{background:#ffd}
p.d:hover span.bg{background:#fdd}
#grow:hover{padding-left:20px;color:#00c}
</style></head><body>
<nav id="nav"><button id="b0" class="on">Dashboard</button><button id="b1">Customers</button><button id="b2">Orders</button></nav>
<table><tbody>
<tr id="r0"><td>0</td><td>Alpha <a href="#x" id="l0">open</a></td><td>10.00</td></tr>
<tr id="r1"><td>1</td><td>Bravo <a href="#y" id="l1">open</a></td><td>20.50</td></tr>
<tr id="r2"><td>2</td><td>Charlie</td><td>7.25</td></tr>
</tbody></table>
<p class="d" id="pd">Decorated <span class="bg" id="sbg">span</span></p>
<div id="grow">grows on hover</div>
</body></html>"##;

// ---- L-PERF-b3: formatting-context replay (flex and grid items, atomic inlines seeded with the
// previous pass's layout results when their box subtree is unchanged).

const REPLAY: &str = r##"<!DOCTYPE html><html><head><style>
body{margin:6px;font-family:"Noto Sans";font-size:13px}
.row{display:flex;gap:4px;width:380px}
.row.narrow{width:300px}
.card{flex:1 1 0;border:1px solid #888;padding:3px}
.card.fat{padding:9px 30px}
.grid{display:grid;grid-template-columns:1fr 2fr;gap:3px}
.grid div{background:#eef}
table{border-collapse:collapse;width:100%}
td{border-bottom:1px solid #ccc;padding:2px;vertical-align:middle}
.badge{display:inline-block;padding:1px 5px;border-radius:6px;background:#06c;color:#fff;font-size:11px}
.badge.wide{padding:1px 14px}
.btn{display:inline-flex;align-items:center;padding:2px 6px;border:1px solid #444}
.pct{width:50%;background:#fd8}
.inner{background:#dfd}
.inner.shift{margin-left:12px;padding-top:6px}
</style></head><body>
<div class="row" id="row"><div class="card" id="c0">First card with some text that wraps</div><div class="card" id="c1">Second card with a badge <span class="badge" id="cb">7</span> and text after it</div><div class="card" id="c2"><div class="pct">half</div><img width="20" height="10" alt=""></div><div class="card" id="c3"><div class="inner" id="in3">inner block</div></div></div>
<div class="grid" id="grid"><div id="g0">one</div><div id="g1">two two two two two two two two</div><div id="g2">three</div><div id="g3">four</div></div>
<table><tbody id="rows">
<tr><td>0</td><td>Alpha <span class="badge" id="b0">new</span></td><td><button class="btn">Edit</button></td></tr>
<tr><td>1</td><td>Bravo <span class="badge" id="b1">paid</span></td><td><button class="btn">Edit</button></td></tr>
<tr><td>2</td><td>Charlie <span class="badge" id="b2">late</span></td><td><button class="btn">Edit</button></td></tr>
</tbody></table>
</body></html>"##;

#[test]
fn replayed_formatting_contexts_equal_cold_and_full_raster() {
    let mut t = Pair::with(REPLAY);
    let replayed = |t: &Pair| {
        t.a.committed_result()
            .unwrap()
            .committed
            .layout
            .replayed_roots()
    };
    let text = |t: &mut Pair, el: &str, s: &str| {
        let s = s.to_string();
        let el = el.to_string();
        t.each(move |p, _| {
            let n = id(p, &el);
            p.set_text_content(n, &s).unwrap();
        });
    };
    // A text edit in one flex item: the other items, the grid items and the badges replay.
    text(
        &mut t,
        "c0",
        "First card, edited, still long enough to wrap",
    );
    t.compare("flex item text", true);
    assert!(
        replayed(&t) >= 8,
        "unchanged items and atomics replay ({})",
        replayed(&t)
    );
    // A badge text in one row (the cell is relaid out, the other rows' badges and buttons replay).
    text(&mut t, "b1", "refunded");
    t.compare("badge text", true);
    assert!(replayed(&t) >= 8, "({})", replayed(&t));
    // The badge inside a flex item (an atomic inside a replay root).
    text(&mut t, "cb", "42");
    t.compare("badge in a flex item", true);
    // A box input of a badge itself (its padding): asked the same size as before by its line, it must
    // not answer from the previous pass.
    for k in 0..2 {
        t.each(|p, _| {
            let n = id(p, "b2");
            p.toggle_class(n, "wide").unwrap();
        });
        t.compare(&format!("badge padding #{k}"), true);
    }
    // A box input deep inside a flex item that does not change what the item is asked: the item must
    // lay out again (its subtree differs), not answer from the previous pass.
    for k in 0..2 {
        t.each(|p, _| {
            let n = id(p, "in3");
            p.toggle_class(n, "shift").unwrap();
        });
        t.compare(&format!("nested box input #{k}"), true);
    }
    // The flex container narrows: the items are asked other sizes (seeded caches miss).
    t.each(|p, _| {
        let n = id(p, "row");
        p.toggle_class(n, "narrow").unwrap();
    });
    t.compare("container width", true);
    t.each(|p, _| {
        let n = id(p, "row");
        p.toggle_class(n, "narrow").unwrap();
    });
    t.compare("container width back", true);
    // A box input inside one item; then a sibling grows and moves the others.
    t.each(|p, _| {
        let n = id(p, "c1");
        p.toggle_class(n, "fat").unwrap();
    });
    t.compare("item padding", true);
    text(&mut t, "g1", "short");
    t.compare("grid item text", true);
    // An item inserted first: every box id after it shifts (nothing to replay by id).
    t.each(|p, _| {
        let (row, c0) = (id(p, "row"), id(p, "c0"));
        let d = p.create_element("div").unwrap();
        p.set_attribute(d, "class", "card").unwrap();
        p.set_text_content(d, "new first").unwrap();
        p.mutate(|doc| doc.insert_before(row, d, Some(c0))).unwrap();
    });
    t.compare("item inserted", true);
    text(&mut t, "c2", "now plain text");
    t.compare("after insert", true);
    // Viewport change: no replay across viewports.
    t.each(|p, _| {
        p.set_environment(380.0, 420.0, 1.0).unwrap();
    });
    t.compare("resize", true);
    assert_eq!(replayed(&t), 0, "no replay across a viewport change");
    text(&mut t, "b0", "x");
    t.compare("after resize", true);
    assert!(replayed(&t) > 0);
}

// ---- L-PERF-b3: a full frame of the damage raster leaves out the items that paint no pixel of the
// surface (below the viewport, clipped by a scroller); its pixels equal a plain full raster's (the
// reference pipeline and the cold oracle raster every item).

#[test]
fn culled_full_frames_equal_plain_full_rasters() {
    let rows: String = (0..60)
        .map(|i| format!("<tr><td>{i}</td><td>Row {i} <span class=\"b\">tag</span></td></tr>"))
        .collect();
    let inner: String = (0..30)
        .map(|i| format!("<p>scrolled line {i}</p>"))
        .collect();
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>
body{{margin:6px;font-family:"Noto Sans";font-size:13px}} body.alt{{background:#ffe}}
#sc{{height:80px;overflow:auto;border:1px solid #888}} #sc p{{margin:2px}}
.shadow{{position:relative;top:330px;height:20px;box-shadow:0 -60px 30px #f00}}
.rot{{transform:rotate(-30deg);width:200px;height:12px;background:#09c;margin-top:-6px}}
.b{{display:inline-block;padding:0 4px;background:#06c;color:#fff;opacity:.8}}
</style></head><body><div id="sc">{inner}</div><div class="rot"></div><table>{rows}</table><div class="shadow"></div></body></html>"#
    );
    let mut t = Pair::with(&html);
    for k in 0..2 {
        t.each(|p, _| {
            let body = p.document().body().unwrap();
            p.toggle_class(body, "alt").unwrap();
        });
        t.compare(&format!("full frame #{k}"), true);
    }
    for y in [40.0, 400.0, 0.0] {
        t.each(|p, _| {
            let n = id(p, "sc");
            p.scroll_to(
                crate::layout::coordinator::overflow::ScrollTarget::Node(n),
                0.0,
                y,
            )
            .unwrap();
        });
        t.compare(&format!("scroller at {y}"), true);
        t.each(|p, _| {
            p.scroll_to(
                crate::layout::coordinator::overflow::ScrollTarget::Viewport,
                0.0,
                y,
            )
            .unwrap();
        });
        t.compare(&format!("viewport at {y}"), true);
    }
    t.each(|p, _| {
        p.set_environment(300.0, 260.0, 1.0).unwrap();
    });
    t.compare("resize", true);
}

#[test]
fn elements_with_shared_computed_values_share_an_exact_box_style_mapping() {
    // Many rows with the same computed values, each with an underlined link (its decoration names its
    // own element) and a decorated `::after`: the shared mapping equals the element's own mapping.
    let rows: String = (0..40).map(|i| format!(r##"<tr><td>{i}</td><td>Name {i} <a href="#r{i}">open</a></td><td class="x">7.25</td></tr>"##)).collect();
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>body{{font-family:"Noto Sans";font-size:14px}} a{{text-decoration:underline}} td.x::after{{content:"!";text-decoration:line-through}}</style></head><body><table>{rows}</table><select><option>a</option></select><select><option>b</option></select></body></html>"#
    );
    let mut p = load_page(&html);
    p.flush().unwrap();
    let style = p.committed_result().unwrap().committed.style.clone();
    let (mapped, differ) =
        crate::layout::box_tree::build::shared_mapping_differences(p.document(), &style);
    assert!(mapped > 160, "{mapped} elements mapped");
    assert!(
        differ.is_empty(),
        "{} elements map differently when shared: {differ:?}",
        differ.len()
    );
}

#[test]
fn hover_and_active_colour_changes_reuse_layout_and_equal_cold() {
    use ui4me_document::ui_state::UiFlags;
    let mut t = Pair::with(COLOURS);
    // (element, flags, whether the layout is reused). Every step repaints something.
    let steps: &[(&str, UiFlags, bool)] = &[
        // Nav buttons: hover then press (text colour; a background on an atomic inline), as a click does.
        ("b1", UiFlags::HOVER, true),
        ("b1", UiFlags::HOVER | UiFlags::ACTIVE, true),
        ("b1", UiFlags::HOVER, true),
        ("b1", UiFlags::empty(), true),
        ("b2", UiFlags::HOVER, true),
        ("b2", UiFlags::empty(), true),
        // Table rows: the cells' text colour moves their `currentColor` collapsed borders. The `:active`
        // colour is translucent: opaque vs translucent is a layout input of the collapsed-border
        // junctions, so going there and back relays out.
        ("r0", UiFlags::HOVER, true),
        ("r0", UiFlags::HOVER | UiFlags::ACTIVE, false),
        ("r0", UiFlags::HOVER, false),
        ("r0", UiFlags::empty(), true),
        ("r1", UiFlags::HOVER, true),
        ("r1", UiFlags::empty(), true),
        // Underlined links (`currentColor` decorations); an `:active` background on an inline box
        // gives it line pieces (layout).
        ("l0", UiFlags::HOVER, true),
        ("l0", UiFlags::empty(), true),
        ("l1", UiFlags::HOVER | UiFlags::ACTIVE, false),
        ("l1", UiFlags::empty(), false),
        // A decorated pseudo-element and an inline box background colour.
        ("pd", UiFlags::HOVER, true),
        ("pd", UiFlags::empty(), true),
        // A hover that moves a box input (padding).
        ("grow", UiFlags::HOVER, false),
        ("grow", UiFlags::empty(), false),
    ];
    let mut prev = t.a.render_headless().unwrap().pixels.data.clone();
    for (k, (el, flags, reuse)) in steps.iter().enumerate() {
        let before = t.a.layout_reuses();
        t.flags(el, *flags);
        let what = format!("step {k}: {el} {flags:?}");
        t.compare(&what, true);
        assert_eq!(
            t.a.layout_reuses() > before,
            *reuse,
            "{what}: layout reused"
        );
        let now = t.a.render_headless().unwrap().pixels.data.clone();
        assert!(now != prev, "{what}: repaints");
        prev = now;
    }
    // A class toggle that only changes text colour reuses it too.
    let before = t.a.layout_reuses();
    t.each(|p, _| {
        let n = id(p, "r2");
        p.toggle_class(n, "alt").unwrap();
    });
    t.compare("row class text colour", true);
    assert_eq!(
        t.a.layout_reuses(),
        before + 1,
        "a text-colour class toggle reuses the layout"
    );
}

const CONTAINERS: &str = r#"<!DOCTYPE html><html><head><style>
body{margin:0;font-family:"Noto Sans";font-size:14px;line-height:18px}
.side{container:side / inline-size;width:150px;float:left;contain:paint}
.main{container-type:inline-size;margin-left:150px}
.wide{width:300px}
.card{padding:6px;border:1px solid #999;background:#eee}
.card .t{font-size:12px}
@container (min-width: 200px){.card{display:flex;gap:8px;background:#cde}.card .t{font-size:5cqi}}
@container side (min-width: 250px){.card{border-color:#c00}}
.box{contain:layout;position:relative}
.box .abs{position:absolute;right:0;top:0;width:10px;height:10px;background:#f00}
</style></head><body>
<div class="side" id="side"><div class="card" id="c1"><span class="t">Side card</span><span>text</span></div></div>
<div class="main" id="main"><div class="card" id="c2"><span class="t">Main card title</span><span>body text</span></div>
<div class="box" id="box"><div class="abs"></div>contained</div></div>
</body></html>"#;

// 015 CQ-3: container queries and containment under the reuse paths: a container resized by a class,
// by the viewport and by content, a container appearing and disappearing, content inserted into a
// container. The optimised pipeline equals the no-reuse reference and the cold oracle after each.
#[test]
fn container_queries_equal_cold_and_the_no_reuse_reference() {
    let mut t = Pair::with(CONTAINERS);
    t.each(|p, _| {
        let n = id(p, "side");
        p.toggle_class(n, "wide").unwrap();
    });
    t.compare("side container widened", true);
    t.each(|p, _| {
        let n = id(p, "side");
        p.toggle_class(n, "wide").unwrap();
    });
    t.compare("side container narrowed", true);
    t.each(|p, _| {
        let n = id(p, "main");
        p.set_attribute(n, "class", "").unwrap();
    });
    t.compare("main no longer a container", true);
    t.each(|p, _| {
        let n = id(p, "main");
        p.set_attribute(n, "class", "main").unwrap();
    });
    t.compare("main a container again", true);
    t.each(|p, _| {
        let n = id(p, "main");
        let c = p.create_element("div").unwrap();
        p.set_attribute(c, "class", "card").unwrap();
        p.set_text_content(c, "inserted card").unwrap();
        p.append_child(n, c).unwrap();
    });
    t.compare("card inserted into a container", true);
    t.each(|p, _| {
        assert!(p.set_environment(260.0, 420.0, 1.0).unwrap());
    });
    t.compare("viewport narrowed", true);
    t.each(|p, _| {
        assert!(p.set_environment(400.0, 420.0, 1.0).unwrap());
    });
    t.compare("viewport widened", true);
}
