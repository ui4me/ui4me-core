// owner: 007-resources — lane L-PNT-r8 (resources-images `srcset`, `sizes`)
//
// HTML Standard "parse a srcset attribute", "parse a sizes attribute" and "select an image source
// from a source set". Expectations are derived from the algorithms, not from engine output.

use ui4me_assets::srcset::*;

fn env(w: f32, h: f32, dpr: f32) -> SourceEnv {
    SourceEnv {
        viewport: (w, h),
        dpr,
    }
}

fn pick(src: Option<&str>, srcset: &str, sizes: Option<&str>, e: SourceEnv) -> (String, f32) {
    let s = select_image_source(src, Some(srcset), sizes, &e).unwrap();
    (s.url, s.density)
}

#[test]
fn density_descriptors_pick_the_smallest_density_at_least_the_dpr() {
    let set = "a@1x.png 1x, a@2x.png 2x";
    assert_eq!(
        pick(None, set, None, env(800.0, 600.0, 1.0)),
        ("a@1x.png".into(), 1.0)
    );
    assert_eq!(
        pick(None, set, None, env(800.0, 600.0, 2.0)),
        ("a@2x.png".into(), 2.0)
    );
    // Between: the larger; beyond all: the largest.
    assert_eq!(pick(None, set, None, env(800.0, 600.0, 1.5)).0, "a@2x.png");
    assert_eq!(pick(None, set, None, env(800.0, 600.0, 3.0)).0, "a@2x.png");
    // No descriptor = 1x.
    assert_eq!(
        pick(None, "plain.png, big.png 2x", None, env(800.0, 600.0, 1.0)).0,
        "plain.png"
    );
}

#[test]
fn src_is_a_one_x_candidate_unless_one_is_listed() {
    assert_eq!(
        pick(Some("src.png"), "two.png 2x", None, env(800.0, 600.0, 1.0)).0,
        "src.png"
    );
    assert_eq!(
        pick(
            Some("src.png"),
            "one.png 1x, two.png 2x",
            None,
            env(800.0, 600.0, 1.0)
        )
        .0,
        "one.png"
    );
    // An unusable srcset falls back to src.
    assert_eq!(
        select_image_source(
            Some("src.png"),
            Some("bad.png 0q"),
            None,
            &env(800.0, 600.0, 1.0)
        )
        .unwrap()
        .url,
        "src.png"
    );
    assert!(select_image_source(None, Some(""), None, &env(800.0, 600.0, 1.0)).is_none());
    assert!(select_image_source(Some("  "), None, None, &env(800.0, 600.0, 1.0)).is_none());
}

#[test]
fn invalid_descriptors_drop_the_candidate() {
    let c = parse_srcset_candidates(
        "a.png 1x 2x, b.png 100w 2x, c.png -1x, d.png 1.5x, e.png 12w, f.png 1.5w",
    );
    let urls: Vec<_> = c.iter().map(|c| c.url.as_str()).collect();
    assert_eq!(urls, ["d.png", "e.png"]);
    assert_eq!(c[0].density, Some(1.5));
    assert_eq!(c[1].width, Some(12));
}

#[test]
fn urls_may_contain_commas_when_no_whitespace_separates_them() {
    let c = parse_srcset_candidates("data:image/png;base64,AAAA 2x,b.png");
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].url, "data:image/png;base64,AAAA");
    assert_eq!(c[0].density, Some(2.0));
    assert_eq!(c[1].url, "b.png");
    // Trailing commas end a candidate without descriptors.
    let c = parse_srcset_candidates("a.png,, b.png");
    assert_eq!(
        c.iter().map(|c| c.url.as_str()).collect::<Vec<_>>(),
        ["a.png", "b.png"]
    );
}

#[test]
fn width_descriptors_use_the_source_size() {
    let set = "s.png 400w, m.png 800w, l.png 1600w";
    // Default size 100vw: viewport 800 -> densities 0.5, 1, 2.
    assert_eq!(
        pick(None, set, None, env(800.0, 600.0, 1.0)),
        ("m.png".into(), 1.0)
    );
    assert_eq!(
        pick(None, set, None, env(800.0, 600.0, 2.0)),
        ("l.png".into(), 2.0)
    );
    // `sizes="400px"`: 1.0, 2.0, 4.0.
    assert_eq!(
        pick(None, set, Some("400px"), env(800.0, 600.0, 1.0)),
        ("s.png".into(), 1.0)
    );
    assert_eq!(
        pick(None, set, Some("400px"), env(800.0, 600.0, 1.5)),
        ("m.png".into(), 2.0)
    );
    assert_eq!(
        pick(None, set, Some("400px"), env(800.0, 600.0, 8.0)).0,
        "l.png"
    );
    // The element's `src` is not a candidate beside width descriptors.
    assert_eq!(
        pick(Some("src.png"), set, Some("400px"), env(800.0, 600.0, 1.0)).0,
        "s.png"
    );
}

#[test]
fn sizes_takes_the_first_entry_whose_condition_holds() {
    let sizes = "(max-width: 500px) 100vw, (min-width: 501px) and (max-width: 900px) 50vw, 400px";
    assert_eq!(source_size(Some(sizes), &env(400.0, 800.0, 1.0)), 400.0);
    assert_eq!(source_size(Some(sizes), &env(800.0, 600.0, 1.0)), 400.0);
    assert_eq!(source_size(Some(sizes), &env(1200.0, 600.0, 1.0)), 400.0);
    assert_eq!(
        source_size(
            Some("(max-width: 900px) 50vw, 10px"),
            &env(800.0, 600.0, 1.0)
        ),
        400.0
    );
    assert_eq!(
        source_size(
            Some("(max-width: 900px) 50vw, 10px"),
            &env(1000.0, 600.0, 1.0)
        ),
        10.0
    );
    assert_eq!(source_size(None, &env(640.0, 480.0, 1.0)), 640.0);
    // No entry applies: 100vw.
    assert_eq!(
        source_size(Some("(max-width: 100px) 10px"), &env(640.0, 480.0, 1.0)),
        640.0
    );
    // An entry with an invalid length is dropped.
    assert_eq!(
        source_size(Some("50% , 30px"), &env(640.0, 480.0, 1.0)),
        30.0
    );
}

#[test]
fn sizes_lengths_and_calc() {
    let e = env(1000.0, 500.0, 1.0);
    assert_eq!(source_size(Some("10em"), &e), 160.0);
    assert_eq!(source_size(Some("calc(100vw - 40px)"), &e), 960.0);
    assert_eq!(source_size(Some("calc(50vw / 2 + 2 * 10px)"), &e), 270.0);
    assert_eq!(
        source_size(
            Some("(min-width: 600px) calc(100vw - 2 * (10px + 5px))"),
            &e
        ),
        970.0
    );
    assert_eq!(source_size(Some("50vh"), &e), 250.0);
    assert_eq!(source_size(Some("1in"), &e), 96.0);
}

#[test]
fn media_conditions() {
    let e = env(700.0, 500.0, 2.0);
    assert!(eval_media("(min-width: 600px)", &e));
    assert!(!eval_media("(min-width: 800px)", &e));
    assert!(eval_media("(width >= 600px) and (height < 600px)", &e));
    assert!(eval_media("(400px <= width <= 800px)", &e));
    assert!(eval_media("not (max-width: 600px)", &e));
    assert!(eval_media(
        "(max-width: 100px) or (orientation: landscape)",
        &e
    ));
    assert!(eval_media("(min-resolution: 2dppx)", &e));
    assert!(!eval_media("(min-resolution: 3dppx)", &e));
    assert!(eval_media(
        "((min-width: 600px) and (max-width: 800px))",
        &e
    ));
    // Unknown features are false.
    assert!(!eval_media("(color-gamut: p3)", &e));
    assert!(!eval_media("garbage", &e));
}

// ---- picture (015 HTML-4) ------------------------------------------------------------------------

fn src(
    srcset: &'static str,
    media: Option<&'static str>,
    mime: Option<&'static str>,
) -> PictureSource<'static> {
    PictureSource {
        srcset: Some(srcset),
        sizes: None,
        media,
        mime,
    }
}

#[test]
fn picture_takes_the_first_matching_source_then_the_img() {
    let sources = [
        src("big.png", Some("(min-width: 600px)"), None),
        src("mid.png", Some("screen and (min-width: 400px)"), None),
    ];
    let sel = |w: f32| {
        select_picture_source(&sources, Some("small.png"), None, None, &env(w, 600.0, 1.0))
            .unwrap()
            .url
    };
    assert_eq!(sel(800.0), "big.png");
    assert_eq!(sel(500.0), "mid.png");
    assert_eq!(sel(300.0), "small.png");
}

#[test]
fn picture_skips_unsupported_types_empty_sets_and_print_media() {
    let sources = [
        src("a.avifx", None, Some("image/x-unknown")),
        src(" , ", None, None),
        src("print.png", Some("print"), None),
        src(
            "ok.png 1x, ok2.png 2x",
            Some("not print"),
            Some("image/png"),
        ),
    ];
    assert_eq!(
        select_picture_source(
            &sources,
            Some("img.png"),
            None,
            None,
            &env(800.0, 600.0, 2.0)
        )
        .unwrap()
        .url,
        "ok2.png"
    );
    assert!(
        supported_image_type("image/webp")
            && supported_image_type("IMAGE/SVG+XML; charset=x")
            && !supported_image_type("image/avif")
    );
}

#[test]
fn media_query_lists() {
    let e = env(700.0, 500.0, 1.0);
    assert!(eval_media_query_list("", &e));
    assert!(eval_media_query_list(
        "(max-width: 500px), (min-width: 600px)",
        &e
    ));
    assert!(eval_media_query_list(
        "only screen and (orientation: landscape)",
        &e
    ));
    assert!(!eval_media_query_list("print, (max-width: 100px)", &e));
    assert!(eval_media_query_list("all", &e));
    assert!(!eval_media_query_list("bogus", &e));
}

#[test]
fn width_descriptors_in_a_source_use_its_sizes() {
    let sources = [PictureSource {
        srcset: Some("s.png 100w, m.png 200w, l.png 400w"),
        sizes: Some("(min-width: 600px) 200px, 100px"),
        media: None,
        mime: None,
    }];
    let sel = |w: f32, d: f32| {
        select_picture_source(&sources, None, None, None, &env(w, 600.0, d))
            .unwrap()
            .url
    };
    assert_eq!(sel(800.0, 1.0), "m.png");
    assert_eq!(sel(800.0, 2.0), "l.png");
    assert_eq!(sel(500.0, 1.0), "s.png");
}
