// owner: 007-resources — lane L-PNT-r8 (WPT-D4 `replaced-elements-width-40`)
//
// An SVG document without a `viewBox` and without both dimensions has no intrinsic ratio: its
// `100%` content fills the raster it is drawn into (CSS Images 3 section 5.1: the document is laid
// out against the concrete object size); a document with a ratio is fitted (`meet`) instead.

use ui4me_assets::decode::{SvgOptions, SvgRenderer};
use ui4me_assets::svg::usvg_adapter::UsvgRenderer;

fn raster(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let r = UsvgRenderer;
    let parsed = r
        .parse(svg.as_bytes(), &SvgOptions::default())
        .expect("parse");
    r.rasterize(&parsed.tree, w, h).expect("rasterize").data
}

fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    data[i..i + 4].try_into().unwrap()
}

#[test]
fn a_ratioless_document_fills_the_raster() {
    // Height only, no viewBox: no ratio. The raster is 40x25; the blue rect covers all of it.
    let d = raster(
        r#"<svg xmlns="http://www.w3.org/2000/svg" height="25" preserveAspectRatio="none"><rect fill="blue" width="100%" height="100%"/></svg>"#,
        40,
        25,
    );
    for (x, y) in [(0, 0), (39, 0), (0, 24), (39, 24), (20, 12)] {
        assert_eq!(px(&d, 40, x, y), [0, 0, 255, 255], "({x},{y})");
    }
    // Width only: a 40x150 raster is filled as well.
    let d = raster(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="50"><rect fill="red" width="100%" height="100%"/></svg>"#,
        40,
        150,
    );
    for (x, y) in [(0, 0), (39, 149), (20, 75)] {
        assert_eq!(px(&d, 40, x, y), [255, 0, 0, 255], "({x},{y})");
    }
}

#[test]
fn a_document_with_a_ratio_is_fitted_not_stretched() {
    // viewBox 2:1 drawn into a 40x40 raster: `meet` centres a 40x20 picture; the top rows stay empty.
    let d = raster(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect fill="red" width="100" height="50"/></svg>"#,
        40,
        40,
    );
    assert_eq!(px(&d, 40, 20, 20), [255, 0, 0, 255]);
    assert_eq!(px(&d, 40, 20, 2)[3], 0, "letterbox above the picture");
}
