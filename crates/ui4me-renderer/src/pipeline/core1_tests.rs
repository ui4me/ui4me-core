//! End-to-end regressions for the Core 1.0 CSS/resource completion.
use crate::Document;
use crate::scene::display_list::DrawOp;
use std::path::Path;
use std::time::{Duration, Instant};

fn page(body: &str) -> Document {
    Document::parse(&format!(
        "<!doctype html><style>html,body{{margin:0;background:white}}\
         *{{box-sizing:border-box}}body{{font:16px/20px sans-serif}}</style>{body}"
    ))
    .unwrap()
}

fn settle(d: &mut Document) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        d.render(400, 200, 1.0).unwrap();
        d.poll_resources().unwrap();
        if !d.has_pending_resources() {
            return d.render(400, 200, 1.0).unwrap();
        }
        assert!(Instant::now() < deadline, "resource load failed to settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn intrinsic_flex_basis_reaches_layout_instead_of_auto() {
    let mut d = page(
        "<style>.row{display:flex;width:350px}.row div{flex-grow:0;flex-shrink:0;min-width:0;\
         width:200px}#min{flex-basis:min-content}#max{flex-basis:max-content}</style>\
         <div class=row><div id=min>hello hello</div></div>\
         <div class=row><div id=max>hello hello</div></div>",
    );
    settle(&mut d);
    let min = d.element_rect("min").unwrap().unwrap().width;
    let max = d.element_rect("max").unwrap().unwrap().width;
    assert!(
        min > 20.0 && max > min * 1.5 && max < 180.0,
        "min={min} max={max}"
    );
    assert!(
        !d.diagnostics()
            .iter()
            .any(|e| e["feature"] == "css.property.flex-basis")
    );
}

#[test]
fn balanced_flex_wrap_is_parsed_and_changes_line_assignment() {
    let mut d = page(
        "<style>.row{display:flex;width:200px;align-content:start}.row div{flex:0 0 60px;height:20px}\
         #normal{flex-wrap:wrap}#balanced{flex-wrap:balance}</style>\
         <div class=row id=normal><div></div><div></div><div id=n3></div><div></div></div>\
         <div class=row id=balanced><div></div><div></div><div id=b3></div><div></div></div>",
    );
    settle(&mut d);
    let normal = d.element_rect("normal").unwrap().unwrap();
    let n3 = d.element_rect("n3").unwrap().unwrap();
    let balanced = d.element_rect("balanced").unwrap().unwrap();
    let b3 = d.element_rect("b3").unwrap().unwrap();
    assert!(
        (n3.y - normal.y).abs() < 0.1,
        "ordinary wrap places three items on first line"
    );
    assert!(
        (b3.y - balanced.y - 20.0).abs() < 0.1,
        "balanced wrap should place two per line: {b3:?}"
    );
}

#[test]
fn legacy_break_word_matches_anywhere_including_intrinsic_measurement() {
    let mut d = page(
        "<style>.row{display:flex;width:80px}.row div{flex:1 1 auto}\
         #legacy{word-break:break-word;overflow-wrap:normal}#modern{overflow-wrap:anywhere}</style>\
         <div class=row><div id=legacy>abcdefghijklmnopqrstuvwx</div></div>\
         <div class=row><div id=modern>abcdefghijklmnopqrstuvwx</div></div>",
    );
    settle(&mut d);
    let legacy = d.element_rect("legacy").unwrap().unwrap();
    let modern = d.element_rect("modern").unwrap().unwrap();
    assert!((legacy.width - modern.width).abs() < 0.1);
    assert!((legacy.height - modern.height).abs() < 0.1);
    assert!(legacy.width <= 80.1 && legacy.height > 20.0);
    assert!(crate::style::profile::filter::supports_declaration(
        "word-break",
        "break-word"
    ));
}

#[test]
fn standard_clamp_is_not_claimed_as_supported() {
    assert!(!crate::style::profile::filter::supports_declaration(
        "line-clamp",
        "2"
    ));
    assert!(crate::style::profile::filter::supports_declaration(
        "-webkit-line-clamp",
        "2"
    ));
    assert_eq!(
        crate::css_profile()
            .1
            .into_iter()
            .find(|(id, _)| *id == "css.property.line-clamp")
            .unwrap()
            .1,
        "off"
    );
}

#[test]
fn file_selection_label_changes_and_reset_rebuilds_pixels() {
    let mut d = page("<input id=f type=file>");
    let before = settle(&mut d);
    let n = d.pipeline_mut_for_testing().element_by_id("f").unwrap();
    assert!(
        d.pipeline_mut_for_testing()
            .set_file_input_names(n, &["report.txt".into()])
            .unwrap()
    );
    let one = settle(&mut d);
    assert!(before != one, "chosen basename must be visible");
    assert!(
        !d.pipeline_mut_for_testing()
            .set_file_input_names(n, &["report.txt".into()])
            .unwrap()
    );
    d.pipeline_mut_for_testing()
        .set_file_input_names(n, &["one.txt".into(), "two.txt".into()])
        .unwrap();
    assert!(one != settle(&mut d), "multiple selection has a count");
    d.pipeline_mut_for_testing()
        .set_file_input_names(n, &[])
        .unwrap();
    assert!(
        before == settle(&mut d),
        "reset must remove stale selection presentation"
    );
    // A failed scene must not acknowledge the external presentation input and
    // let the subsequent retry reuse the old label's box/layout or scene cache.
    let p = d.pipeline_mut_for_testing();
    p.set_file_input_names(n, &["report.txt".into()]).unwrap();
    p.fail_next_update_for_testing(
        ui4me_engine_types::stage::Stage::Scene,
        ui4me_engine_types::codes_generated::Code::InternalInvariant,
    );
    assert!(d.render(400, 200, 1.0).is_err());
    d.pipeline_mut_for_testing().note_input();
    assert!(
        one == settle(&mut d),
        "failed publication must retain label dirtiness"
    );
}

#[test]
fn resize_affordance_is_visible_only_when_resizing_is_allowed() {
    let mut d = page("<div id=b style='width:100px;height:100px;resize:both;overflow:auto'></div>");
    settle(&mut d);
    let count = |d: &mut Document| {
        d.pipeline_mut_for_testing()
            .committed()
            .unwrap()
            .scene
            .display_list
            .items
            .iter()
            .filter(|i| i.kind.0 == "resize-grip")
            .count()
    };
    assert_eq!(count(&mut d), 1);
    d.set_attribute(
        "b",
        "style",
        Some("width:100px;height:100px;resize:none;overflow:auto"),
    )
    .unwrap();
    settle(&mut d);
    assert_eq!(count(&mut d), 0);
}

#[test]
fn inset_shadow_with_asymmetric_elliptical_radii_is_painted() {
    let mut d = page(
        "<div style='width:100px;height:100px;background:white;border-radius:20px 5px 15px 10px / 10px 18px 6px 15px;box-shadow:inset 0 0 14px 4px black'></div>",
    );
    let pixels = settle(&mut d);
    let center = pixels[50 * 400 + 50] & 0xffffff;
    let edge = pixels[50 * 400 + 3] & 0xffffff;
    assert!(
        center > 0xeeeeee && edge < 0xaaaaaa,
        "center={center:06x} edge={edge:06x}"
    );
    assert_eq!(
        pixels[150 * 400 + 150] & 0xffffff,
        0xffffff,
        "inset never paints outside the box"
    );
    assert!(
        !d.diagnostics()
            .iter()
            .any(|e| e["stage"] == "raster" && e["code"] == "UNSUPPORTED_FEATURE")
    );
}

#[test]
fn face_feature_default_is_applied_and_element_setting_overrides_it() {
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/fonts/noto-sans");
    let mut d = Document::parse_with_source(
        "<!doctype html><style>body{margin:0} @font-face{font-family:Test;src:url(NotoSans-Regular.ttf);font-feature-settings:'liga' 0}\
         #t{font:40px/50px Test}</style><span id=t>office affinity</span>",
        Some(&fonts), 400, 200,
    ).unwrap();
    let defaults_off = settle(&mut d);
    let glyph_count = |d: &mut Document| {
        d.pipeline_mut_for_testing()
            .committed()
            .unwrap()
            .scene
            .display_list
            .items
            .iter()
            .filter_map(|item| match &item.op {
                DrawOp::DrawGlyphRun(run) => Some(run.glyphs.len()),
                _ => None,
            })
            .sum::<usize>()
    };
    let without_ligature = glyph_count(&mut d);
    d.set_attribute("t", "style", Some("font-feature-settings:'liga' 0"))
        .unwrap();
    assert_eq!(
        defaults_off,
        settle(&mut d),
        "face default and same element setting must agree"
    );
    d.set_attribute("t", "style", Some("font-feature-settings:'liga' 1"))
        .unwrap();
    let property_on = settle(&mut d);
    assert_ne!(defaults_off, property_on);
    assert!(
        glyph_count(&mut d) < without_ligature,
        "explicit property re-enables ligatures"
    );
    assert!(
        !d.diagnostics()
            .iter()
            .any(|e| e["feature"] == "css.at-rule.font-face.font-feature-settings")
    );
}

#[test]
fn content_sized_select_reacts_to_selection_and_option_text() {
    let mut d = page(
        "<select id=s style='field-sizing:content'><option value=a>A</option><option id=long value=b>A much longer selection</option></select>",
    );
    settle(&mut d);
    let initial = d.element_rect("s").unwrap().unwrap().width;
    d.set_value("s", "b").unwrap();
    settle(&mut d);
    let selected = d.element_rect("s").unwrap().unwrap().width;
    assert!(
        selected > initial + 50.0,
        "initial={initial}, selected={selected}"
    );
    d.set_text("long", "B").unwrap();
    settle(&mut d);
    let renamed = d.element_rect("s").unwrap().unwrap().width;
    assert!(renamed < selected - 50.0);
}

#[test]
fn anchor_size_is_explicitly_excluded_in_supports_and_computed_variables() {
    assert!(!crate::style::profile::filter::supports_declaration(
        "width",
        "anchor-size(width)"
    ));
    let mut d = page("<div id=a style='--size:anchor-size(width);width:var(--size)'>A</div>");
    settle(&mut d);
    assert!(
        d.diagnostics()
            .iter()
            .any(|e| e["feature"] == "css.function.anchor-size")
    );
}

#[test]
fn embedded_png_color_glyph_is_rendered_in_color() {
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/fonts/bitmap");
    let mut d = Document::parse_with_source(
        "<!doctype html><style>body{margin:0;color:black;background:white}\
         @font-face{font-family:Bitmap;src:url(UI4MEBitmapTest.ttf)}\
         span{font:32px/40px Bitmap}</style><span>&#xE000;</span>",
        Some(&fonts),
        400,
        200,
    )
    .unwrap();
    let pixels = settle(&mut d);
    assert!(
        pixels
            .iter()
            .filter(|p| (**p & 0xffffff) == 0xff0000)
            .count()
            > 100,
        "the sbix PNG glyph must paint red, not the fallback black outline"
    );
}

/// Core 1.0 C09 (found natively on KDE): every replacement of a displayed `ui4me-res:` bundle left
/// the previous decoded raster alive in the document's picture table (+256 KiB per replacement for a
/// 256x256 image, visible in the cache's retired bytes and the host PSS), and a removed image kept
/// its pixels after a purge. Replacement churn must settle, and remove + purge must free them.
#[test]
fn replaced_bundles_and_removed_images_do_not_retain_decoded_rasters() {
    // A 200x100 PNG fixture: 80,000 decoded bytes. Each registration is a new revision.
    let png: std::sync::Arc<[u8]> = include_bytes!(
        "../../../ui4me-assets/tests/fixtures/resources-images/natural-sizes/codecs/png-200x100.png"
    )
    .as_slice()
    .into();
    let mut d = Document::parse_hosted(
        "<!doctype html><body style='margin:0'><img id=i src='ui4me-res://b/a.png'></body>",
        None,
        400,
        200,
    )
    .unwrap();
    let bytes = |d: &Document| {
        d.resource_stats().unwrap()["cache"]["bytes"]
            .as_u64()
            .unwrap()
    };
    let mut after = Vec::new();
    for _ in 0..12 {
        d.register_resource_bundle(
            "b",
            vec![ui4me_assets::bundles::BundleEntryInput {
                path: "a.png".into(),
                data: png.clone(),
                mime: None,
            }],
        )
        .unwrap();
        settle(&mut d);
        after.push(bytes(&d));
    }
    let one = 200 * 100 * 4;
    assert!(
        after.iter().skip(2).all(|b| *b <= 2 * one),
        "decoded bytes must not grow with replacements: {after:?}"
    );
    d.remove_element("i").unwrap();
    settle(&mut d);
    d.purge_unused_resources(None).unwrap();
    settle(&mut d);
    assert_eq!(
        bytes(&d),
        0,
        "a removed, purged image keeps no decoded pixels"
    );
}

/// RC2: a raw bitmap is a retained image source. Fixed dimensions must update
/// the painted pixels without replacing the document or its layout geometry.
#[test]
fn live_bitmap_replaces_pixels_without_replacing_document() {
    use std::sync::Arc;
    let mut d = Document::parse_hosted(
        "<!doctype html><html><body style='margin:0;background:white'>\
         <img id=plot width=8 height=8 src='ui4me-bitmap://plot'></body></html>",
        None,
        400,
        200,
    )
    .unwrap();
    let mut red = vec![0u8; 8 * 8 * 4];
    for p in red.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[255, 0, 0, 255]);
    }
    d.set_live_bitmap("plot", 8, 8, Arc::from(red)).unwrap();
    let first = settle(&mut d);
    let node_id = d.id();
    let geometry = d.element_rect("plot").unwrap().unwrap();
    let mut green = vec![0u8; 8 * 8 * 4];
    for p in green.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[0, 255, 0, 255]);
    }
    d.set_live_bitmap("plot", 8, 8, Arc::from(green)).unwrap();
    let second = settle(&mut d);
    assert_ne!(
        first, second,
        "bitmap replacement must visibly update pixels"
    );
    assert_eq!(d.id(), node_id);
    let after = d.element_rect("plot").unwrap().unwrap();
    assert!((after.width - geometry.width).abs() < 0.01);
    assert!((after.height - geometry.height).abs() < 0.01);
    assert!(d.remove_live_bitmap("plot").unwrap());
    let third = settle(&mut d);
    assert_ne!(second, third);
}

/// The update records published since `seq`, as the stages each one ran.
fn stages_since(d: &Document, seq: u64) -> Vec<Vec<ui4me_engine_types::stage::Stage>> {
    d.pipeline_for_tests()
        .stores
        .updates
        .iter()
        .filter(|u| u.update_seq > seq)
        .map(|u| u.stages_run.clone())
        .collect()
}

fn last_seq(d: &Document) -> u64 {
    d.pipeline_for_tests()
        .stores
        .updates
        .iter()
        .map(|u| u.update_seq)
        .max()
        .unwrap_or(0)
}

/// RC2 final: a same-size live frame is paint-only. It must rebuild the scene
/// without restyling or relaying out the document; a geometry-changing
/// replacement (a new bitmap with another size) still lays out.
#[test]
fn same_size_live_bitmap_frames_skip_style_and_layout() {
    use std::sync::Arc;
    use ui4me_engine_types::stage::Stage;
    let mut d = Document::parse_hosted(
        "<!doctype html><html><body style='margin:0'><p id=t>text</p>\
         <img id=plot src='ui4me-bitmap://plot'></body></html>",
        None,
        400,
        200,
    )
    .unwrap();
    d.set_live_bitmap("plot", 8, 8, Arc::from(vec![255u8; 8 * 8 * 4]))
        .unwrap();
    settle(&mut d);
    for frame in 0..5u8 {
        let before = last_seq(&d);
        let pixels = vec![frame.wrapping_mul(40); 8 * 8 * 4];
        d.set_live_bitmap("plot", 8, 8, Arc::from(pixels)).unwrap();
        settle(&mut d);
        let updates = stages_since(&d, before);
        assert!(!updates.is_empty(), "a frame must publish an update");
        for stages in &updates {
            assert!(
                stages.contains(&Stage::Scene),
                "frame must repaint: {stages:?}"
            );
            for skipped in [Stage::Cascade, Stage::Box, Stage::Text, Stage::Layout] {
                assert!(
                    !stages.contains(&skipped),
                    "pixel-only frame ran {skipped:?}: {stages:?}"
                );
            }
        }
    }
    // Removing the surface changes the intrinsic size: layout must run.
    let before = last_seq(&d);
    assert!(d.remove_live_bitmap("plot").unwrap());
    settle(&mut d);
    assert!(
        stages_since(&d, before)
            .iter()
            .any(|s| s.contains(&Stage::Layout)),
        "removing a sized image must relayout"
    );
}

/// RC2 final: closing a live surface and creating it again under the same ID
/// (the producer's resize path) must lay the image out at the new intrinsic
/// size and paint its pixels.
#[test]
fn recreated_live_bitmap_is_laid_out_and_painted_again() {
    use std::sync::Arc;
    let mut d = Document::parse_hosted(
        "<!doctype html><html><body style='margin:0;background:white'>\
         <img id=plot src='ui4me-bitmap://plot'></body></html>",
        None,
        400,
        200,
    )
    .unwrap();
    let solid =
        |w: u32, h: u32, rgba: [u8; 4]| -> Arc<[u8]> { Arc::from(rgba.repeat((w * h) as usize)) };
    d.set_live_bitmap("plot", 32, 16, solid(32, 16, [255, 0, 0, 255]))
        .unwrap();
    settle(&mut d);
    let rect = d.element_rect("plot").unwrap().unwrap();
    assert_eq!((rect.width, rect.height), (32.0, 16.0));
    assert!(d.remove_live_bitmap("plot").unwrap());
    settle(&mut d);
    d.set_live_bitmap("plot", 64, 48, solid(64, 48, [0, 0, 255, 255]))
        .unwrap();
    let px = settle(&mut d);
    let rect = d.element_rect("plot").unwrap().unwrap();
    assert_eq!(
        (rect.width, rect.height),
        (64.0, 48.0),
        "the recreated surface must be laid out at its new size"
    );
    let blue = px[(10 * 400 + 10) as usize];
    let frame = solid(64, 48, [0, 255, 0, 255]);
    d.set_live_bitmap("plot", 64, 48, frame).unwrap();
    let px2 = settle(&mut d);
    assert_ne!(blue, px2[(10 * 400 + 10) as usize], "new frame must paint");
    assert_ne!(
        blue,
        px[(150 * 400 + 300) as usize],
        "image painted inside its box only"
    );
}
