// owner: 007-resources (T046, T048–T050, T052) — static SVG images (US3 acceptance 1–4, AD-24).
//
// Fixtures: verification/fixtures/resources-svg/svg-images (oracle-capturable HTML fixture) and
// engine-corpus.json (file/data: hrefs, nesting, SVGZ). Tests never write into the repository.
#![cfg(unix)]
#![allow(clippy::chunks_exact_to_as_chunks)]

mod support;

use std::path::{Path, PathBuf};
use support::*;
use ui4me_assets::ResourceManager;
use ui4me_assets::decode::SvgNatural;
use ui4me_assets::request::Requester;
use ui4me_assets::svg::prescan;
use ui4me_assets::uri;
use ui4me_assets::view::{ImageState, PaintImage};
use ui4me_engine_types::codes_generated::Code;
use url::Url;

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-svg/engine-corpus.json"
));

fn fixture(name: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/resources-svg/svg-images")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn corpus_bytes(name: &str) -> Vec<u8> {
    let v: serde_json::Value = serde_json::from_str(CORPUS).unwrap();
    let e = &v["files"][name];
    match (e.get("hex"), e.get("text")) {
        (Some(h), _) => {
            let h = h.as_str().unwrap();
            (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                .collect()
        }
        (_, Some(t)) => t.as_str().unwrap().as_bytes().to_vec(),
        _ => panic!("corpus entry {name}"),
    }
}

struct Page {
    mgr: ResourceManager,
    doc: ui4me_engine_types::ids::DocumentId,
    base: Url,
    tmp: TempDir,
    next: u32,
}

fn page() -> Page {
    let tmp = TempDir::new("svg");
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = tmp.write("index.html", b"<html></html>");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    Page {
        mgr,
        doc,
        base,
        tmp,
        next: 1,
    }
}

impl Page {
    fn load(&mut self, src: &str) -> ui4me_engine_types::resource_ids::ResourceRef {
        self.next += 1;
        let r = self
            .mgr
            .request_image(
                self.doc,
                Requester::ImgElement(node_id(self.next)),
                &self.base,
                src,
                1.0,
            )
            .unwrap();
        settle(&mut self.mgr, self.doc);
        r
    }
    fn svg(&mut self, name: &str, bytes: &[u8]) -> ui4me_engine_types::resource_ids::ResourceRef {
        self.tmp.write(name, bytes);
        self.load(name)
    }
    fn raster(
        &self,
        r: &ui4me_engine_types::resource_ids::ResourceRef,
        size: (u32, u32),
    ) -> (u32, u32, Vec<u8>) {
        match self.mgr.paint_image(r, size) {
            PaintImage::Raster { pixels, .. } => {
                (pixels.width, pixels.height, pixels.data.to_vec())
            }
            other => panic!("{other:?}"),
        }
    }
}

fn px(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.0 + x) * 4) as usize;
    [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
}

fn svg_with(body: &str) -> Vec<u8> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="20" height="20">{body}</svg>"#).into_bytes()
}

// ---- T048: the owned pre-scan ----

#[test]
fn prescan_natural_size_and_ratio_invent_nothing() {
    let n = |name: &str| {
        prescan::scan(&fixture(&format!("svg/{name}")), 1000)
            .unwrap()
            .natural
    };
    assert_eq!(
        n("sized-40x20.svg"),
        SvgNatural {
            width: Some(40.0),
            height: Some(20.0),
            ratio: Some((40.0, 20.0))
        }
    );
    assert_eq!(
        n("viewbox-only.svg"),
        SvgNatural {
            width: None,
            height: None,
            ratio: Some((30.0, 15.0))
        }
    );
    assert_eq!(
        n("no-size-no-viewbox.svg"),
        SvgNatural {
            width: None,
            height: None,
            ratio: None
        },
        "no invented 100x100 or 300x150"
    );
    assert_eq!(n("checkbox.svg").ratio, Some((20.0, 20.0)));
}

#[test]
fn prescan_text_is_detected_and_reported_once_per_resource() {
    assert!(
        prescan::scan(&fixture("svg/with-text.svg"), 1000)
            .unwrap()
            .has_text
    );
    assert!(
        !prescan::scan(&fixture("svg/sized-40x20.svg"), 1000)
            .unwrap()
            .has_text
    );
    let mut p = page();
    let r = p.svg("with-text.svg", &fixture("svg/with-text.svg"));
    p.mgr.paint_image(&r, (60, 24));
    p.mgr.paint_image(&r, (120, 48));
    assert!(
        matches!(p.mgr.natural_size(&r), ImageState::Ready { .. }),
        "the rest of the SVG still renders"
    );
    assert_eq!(
        p.mgr
            .unsupported_reports()
            .iter()
            .filter(|u| u.key == "svg.text")
            .count(),
        1
    );
}

#[test]
fn prescan_element_limit_and_gzip_and_malformed() {
    let many = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>",
        "<g/>".repeat(20)
    );
    assert_eq!(
        prescan::scan(many.as_bytes(), 10).unwrap_err().code,
        Code::SizeLimit
    );
    assert!(prescan::scan(many.as_bytes(), 100).is_ok());
    assert_eq!(
        prescan::scan(&corpus_bytes("sample.svgz"), 1000)
            .unwrap_err()
            .code,
        Code::UnsupportedMediaType,
        "no SVGZ"
    );
    assert_eq!(
        prescan::scan(b"<svg", 10).unwrap_err().code,
        Code::DecodeFailed
    );
    assert_eq!(
        prescan::scan(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"/>", 10)
            .unwrap_err()
            .code,
        Code::DecodeFailed
    );
    let dtd = b"<!DOCTYPE svg [<!ENTITY a \"b\">]><svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    assert_eq!(
        prescan::scan(dtd, 10).unwrap_err().code,
        Code::DecodeFailed,
        "DTDs are refused"
    );
}

#[test]
fn prescan_svgz_through_the_manager_is_unsupported_media_type() {
    let mut p = page();
    let r = p.svg("sample.svgz", &corpus_bytes("sample.svgz"));
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Broken {
            code: Code::UnsupportedMediaType
        }
    );
}

// ---- acceptance 1–2: natural size through the manager, rendering ----

#[test]
fn natural_sizes_through_the_manager() {
    let mut p = page();
    let r = p.svg("sized.svg", &fixture("svg/sized-40x20.svg"));
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Ready {
            width_css: Some(40.0),
            height_css: Some(20.0),
            ratio: Some((40.0, 20.0))
        }
    );
    let r = p.svg("vb.svg", &fixture("svg/viewbox-only.svg"));
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Ready {
            width_css: None,
            height_css: None,
            ratio: Some((30.0, 15.0))
        }
    );
    let r = p.svg("none.svg", &fixture("svg/no-size-no-viewbox.svg"));
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Ready {
            width_css: None,
            height_css: None,
            ratio: None
        }
    );
    let d = p.mgr.image_element_data(&r).unwrap();
    assert!(d.complete && d.failure.is_none());
}

#[test]
fn sized_svg_renders_at_the_requested_size() {
    let mut p = page();
    let r = p.svg("sized.svg", &fixture("svg/sized-40x20.svg"));
    let img = p.raster(&r, (40, 20));
    assert_eq!((img.0, img.1), (40, 20));
    assert_eq!(
        px(&img, 1, 1),
        [0x22, 0xaa, 0x77, 255],
        "rect fill #2a7 in the corner"
    );
    assert_eq!(
        px(&img, 20, 10),
        [255, 255, 255, 255],
        "white circle in the middle"
    );
}

// ---- T049: resolver, static rendering, currentColor ----

#[test]
fn resolver_denies_files_and_urls_and_records_one_diagnostic_per_href() {
    // Sockets inherited from the launching process do not count; none may be opened here (as policy_unix.rs).
    let sockets_before = support::monitor::sockets();
    let mut p = page();
    let r = p.svg("file-href.svg", &corpus_bytes("file-href.svg"));
    assert!(
        matches!(p.mgr.natural_size(&r), ImageState::Ready { .. }),
        "the SVG itself loads"
    );
    let rec = p
        .mgr
        .diagnostic_records()
        .into_iter()
        .find(|x| x.url.ends_with("file-href.svg"))
        .unwrap();
    assert_eq!(
        rec.warnings
            .iter()
            .filter(|c| **c == Code::ResourceDenied)
            .count(),
        1
    );
    // relative reference, URL reference and a duplicate of the first
    let body = r#"<image xlink:href="../img/dot.png" width="5" height="5"/><image xlink:href="http://example.invalid/a.png" width="5" height="5"/><image xlink:href="../img/dot.png" width="5" height="5"/>"#;
    let r = p.svg("many.svg", &svg_with(body));
    let rec = p
        .mgr
        .diagnostic_records()
        .into_iter()
        .find(|x| x.url.ends_with("many.svg"))
        .unwrap();
    assert_eq!(
        rec.warnings
            .iter()
            .filter(|c| **c == Code::ResourceDenied)
            .count(),
        2,
        "one per distinct href"
    );
    let img = p.raster(&r, (20, 20));
    assert_eq!(px(&img, 2, 2)[3], 0, "nothing was drawn for a denied image");
    assert_eq!(
        support::monitor::sockets(),
        sockets_before,
        "no network path was exercised"
    );
}

#[test]
fn resolver_has_no_file_system_or_network_surface() {
    // the adapter never installs usvg's default string resolver (which opens files) and names no I/O API
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/usvg_adapter.rs"),
    )
    .unwrap();
    for needle in [
        "default_string_resolver",
        "std::fs",
        "std::net",
        "File::open",
        "resources_dir: Some",
        "Options::default",
    ] {
        let hits = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .filter(|l| l.contains(needle))
            .count();
        assert_eq!(hits, 0, "usvg_adapter.rs must not use `{needle}`");
    }
}

#[test]
fn resolver_relative_href_fixture_is_denied_not_read() {
    let mut p = page();
    // the sibling file exists next to the SVG: it must still not be read by the SVG
    p.tmp.write("img/dot.png", &fixture("img/dot.png"));
    let r = p.svg(
        "svg/relative-image-href.svg",
        &fixture("svg/relative-image-href.svg"),
    );
    let img = p.raster(&r, (20, 20));
    assert!(
        img.2.chunks_exact(4).all(|c| c[3] == 0),
        "the denied image draws nothing"
    );
    let rec = p
        .mgr
        .diagnostic_records()
        .into_iter()
        .find(|x| x.url.ends_with("relative-image-href.svg"))
        .unwrap();
    assert_eq!(rec.warnings, vec![Code::ResourceDenied]);
}

#[test]
fn static_scripts_and_animation_do_not_run() {
    let svg = svg_with(
        r##"<script>document.rootElement.setAttribute('width','99')</script><rect width="20" height="20" fill="#00f"><animate attributeName="fill" from="#00f" to="#f00" dur="1s" fill="freeze"/></rect>"##,
    );
    let mut p = page();
    let r = p.svg("static.svg", &svg);
    let a = p.raster(&r, (20, 20));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let b = p.raster(&r, (20, 20));
    assert_eq!(px(&a, 10, 10), [0, 0, 255, 255]);
    assert_eq!(a, b, "no animation or script changes the output");
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Ready {
            width_css: Some(20.0),
            height_css: Some(20.0),
            ratio: Some((20.0, 20.0))
        }
    );
}

#[test]
fn current_color_resolves_inside_the_svg_with_a_black_initial_colour() {
    let mut p = page();
    let r = p.svg("cc.svg", &fixture("svg/current-color.svg"));
    let img = p.raster(&r, (24, 24));
    assert_eq!(px(&img, 12, 12), [0, 0, 0, 255], "initial colour is black");
    let red = String::from_utf8(fixture("svg/current-color.svg"))
        .unwrap()
        .replace("<svg ", "<svg color=\"#f00\" ");
    let r = p.svg("cc-red.svg", red.as_bytes());
    let img = p.raster(&r, (24, 24));
    assert_eq!(
        px(&img, 12, 12),
        [255, 0, 0, 255],
        "the document's own colour property"
    );
}

// ---- nesting and limits ----

#[test]
fn nested_data_svg_within_four_levels_renders_and_deeper_is_size_limit() {
    let mut p = page();
    let ok = p.svg("n4.svg", ui4me_support_nested(4).as_bytes());
    assert!(matches!(p.mgr.natural_size(&ok), ImageState::Ready { .. }));
    let deep = p.svg("n5.svg", ui4me_support_nested(5).as_bytes());
    assert_eq!(
        p.mgr.natural_size(&deep),
        ImageState::Broken {
            code: Code::SizeLimit
        }
    );
    let fixture5 = p.svg("nested-depth5.svg", &corpus_bytes("nested-depth5.svg"));
    assert!(matches!(
        p.mgr.natural_size(&fixture5),
        ImageState::Ready { .. }
            | ImageState::Broken {
                code: Code::SizeLimit
            }
    ));
}

fn ui4me_support_nested(depth: u32) -> String {
    support::bombs::nested_svg(depth)
}

#[test]
fn data_url_glyphs_from_the_corpus_load_as_images() {
    let mut p = page();
    for name in ["data-url-checkbox", "data-url-select"] {
        let url = String::from_utf8(corpus_bytes(name)).unwrap();
        p.next += 1;
        let r = p
            .mgr
            .request_image(
                p.doc,
                Requester::CssImage {
                    node: node_id(p.next),
                    property: "background-image".into(),
                    layer: 0,
                    url: url.clone(),
                },
                &p.base,
                &url,
                1.0,
            )
            .unwrap();
        settle(&mut p.mgr, p.doc);
        assert!(
            matches!(
                p.mgr.natural_size(&r),
                ImageState::Ready {
                    ratio: Some(_),
                    width_css: None,
                    ..
                }
            ),
            "{name}: ratio from the viewBox, no natural size"
        );
        let img = p.raster(&r, (16, 16));
        assert!(
            img.2.chunks_exact(4).any(|c| c[3] > 0),
            "{name} draws something"
        );
    }
}

// ---- T050: embedded rasters share the codec crates ----

fn b64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}

#[test]
fn embedded_raster_png_and_jpeg_render() {
    let mut p = page();
    let dot = fixture("img/dot.png");
    let svg = svg_with(&format!(
        r#"<image xlink:href="data:image/png;base64,{}" width="20" height="20"/>"#,
        b64(&dot)
    ));
    let r = p.svg("png.svg", &svg);
    let img = p.raster(&r, (20, 20));
    assert!(
        img.2.chunks_exact(4).any(|c| c[3] > 0),
        "the embedded PNG is drawn"
    );
    let codecs = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/resources-images/natural-sizes/codecs/jpeg-icc-64x48.jpg");
    let jpg = std::fs::read(codecs).unwrap();
    let svg = svg_with(&format!(
        r#"<image xlink:href="data:image/jpeg;base64,{}" width="20" height="20"/>"#,
        b64(&jpg)
    ));
    let r = p.svg("jpg.svg", &svg);
    let img = p.raster(&r, (20, 20));
    assert!(
        img.2.chunks_exact(4).filter(|c| c[3] == 255).count() > 200,
        "the embedded JPEG fills most of the box"
    );
}

#[test]
fn embedded_raster_over_the_pixel_budget_is_skipped_with_a_size_limit_warning() {
    let mut p = page();
    let bomb = support::bombs::png_declared(50_000, 50_000, 4096);
    let svg = svg_with(&format!(
        r##"<rect width="20" height="20" fill="#0f0"/><image xlink:href="data:image/png;base64,{}" width="20" height="20"/>"##,
        b64(&bomb)
    ));
    let r = p.svg("bomb.svg", &svg);
    assert!(
        matches!(p.mgr.natural_size(&r), ImageState::Ready { .. }),
        "the rest of the SVG renders"
    );
    assert_eq!(px(&p.raster(&r, (20, 20)), 10, 10), [0, 255, 0, 255]);
    let rec = p
        .mgr
        .diagnostic_records()
        .into_iter()
        .find(|x| x.url.ends_with("bomb.svg"))
        .unwrap();
    assert!(rec.warnings.contains(&Code::SizeLimit));
}

// ---- T052: device-scale rasterisation ----

#[test]
fn raster_scale_rasterises_at_the_device_size_and_caches_per_size() {
    let mut p = page();
    let r = p.svg("sized.svg", &fixture("svg/sized-40x20.svg"));
    let a = p.raster(&r, (40, 20));
    let s = p.mgr.svg_raster_stats();
    assert_eq!((s.rasterisations, s.hits), (1, 0));
    let a2 = p.raster(&r, (40, 20));
    assert_eq!(a, a2);
    assert_eq!(
        p.mgr.svg_raster_stats().hits,
        1,
        "the same size is a cache hit"
    );
    // a scale of 1.5: 60 x 30 device pixels, sharp (re-rasterised, not a stretched 40x20)
    let b = p.raster(&r, (60, 30));
    assert_eq!((b.0, b.1), (60, 30));
    assert_eq!(px(&b, 30, 15), [255, 255, 255, 255]);
    assert_eq!(px(&b, 58, 28), [0x22, 0xaa, 0x77, 255]);
    assert_eq!(p.mgr.svg_raster_stats().rasterisations, 2);
    // back to the first size: served from the cache, never the 60x30 raster
    let a3 = p.raster(&r, (40, 20));
    assert_eq!((a3.0, a3.1), (40, 20));
    assert_eq!(p.mgr.svg_raster_stats().rasterisations, 2);
}

#[test]
fn reraster_generation_changes_with_the_size_and_a_previous_size_is_never_returned() {
    let mut p = page();
    let r = p.svg("vb.svg", &fixture("svg/viewbox-only.svg"));
    let gen_of = |p: &Page, size| match p.mgr.paint_image(&r, size) {
        PaintImage::Raster { pixels, generation } => (generation, pixels.width, pixels.height),
        other => panic!("{other:?}"),
    };
    let g1 = gen_of(&p, (30, 15));
    let g2 = gen_of(&p, (45, 22));
    assert_ne!(g1.0, g2.0);
    assert_eq!((g1.1, g1.2, g2.1, g2.2), (30, 15, 45, 22));
    assert_eq!(gen_of(&p, (30, 15)), g1, "the cached raster of that size");
    assert!(
        matches!(p.mgr.paint_image(&r, (0, 0)), PaintImage::NotReady),
        "no size asked for yet"
    );
}

#[test]
fn raster_scale_clamps_each_side_to_65535_with_a_warning_and_refuses_over_budget_rasters() {
    let mut p = page();
    let r = p.svg("vb.svg", &fixture("svg/viewbox-only.svg"));
    // 65,535 x 65,535 x 4 bytes is far over the decoded-memory budget
    assert!(matches!(
        p.mgr.paint_image(&r, (65_535, 65_535)),
        PaintImage::Broken {
            code: Code::BudgetExceeded
        }
    ));
    let r = p.svg("vb2.svg", &fixture("svg/viewbox-only.svg"));
    let img = p.raster(&r, (70_000, 8));
    assert_eq!((img.0, img.1), (65_535, 8), "clamped to 65,535 px");
    assert_eq!(
        p.mgr.svg_raster_stats().clamped,
        1,
        "a clamped request is recorded as a SIZE_LIMIT warning"
    );
    // a refused new size keeps showing the previous raster when there is one
    let _ = p.raster(&r, (30, 15));
    assert!(matches!(
        p.mgr.paint_image(&r, (65_535, 65_535)),
        PaintImage::Raster { .. }
    ));
}

#[test]
fn meet_fit_keeps_the_aspect_ratio_inside_a_differently_shaped_box() {
    let mut p = page();
    let r = p.svg("sized.svg", &fixture("svg/sized-40x20.svg"));
    let img = p.raster(&r, (40, 40));
    assert_eq!(px(&img, 20, 2)[3], 0, "letterboxed above the 2:1 image");
    assert_eq!(px(&img, 2, 12), [0x22, 0xaa, 0x77, 255]);
}
