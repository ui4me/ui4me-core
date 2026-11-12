//! 0.9 F0905: a Unicode corpus through the production facade. Each script either renders with
//! real glyphs or reports the missing-glyph diagnostic (never silent .notdef); scripts without
//! spaces wrap; right-to-left and vertical text keep logical offsets with geometry. Fonts come
//! from the host, so glyph coverage is reported per script, not assumed.
use super::*;

const CORPUS: &[(&str, &str, &str)] = &[
    ("latin", "en", "Příliš žluťoučký kůň úpěl ďábelské ódy"),
    (
        "cyrillic",
        "uk",
        "Чуєш їх, доцю, га? Кумедна ж ти, прощайся без ґольфів!",
    ),
    ("greek", "el", "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία"),
    (
        "arabic",
        "ar",
        "نص حكيم له سر قاطع وذو شأن عظيم مكتوب على ثوب أخضر",
    ),
    ("hebrew", "he", "דג סקרן שט בים מאוכזב ולפתע מצא חברה"),
    (
        "devanagari",
        "hi",
        "ऋषियों को सताने वाले दुष्ट राक्षसों के राजा रावण का सर्वनाश",
    ),
    ("thai", "th", "เป็นมนุษย์สุดประเสริฐเลิศคุณค่ากว่าบรรดาฝูงสัตว์เดรัจฉาน"),
    (
        "cjk",
        "ja",
        "いろはにほへとちりぬるをわかよたれそつねならむ漢字仮名交じり文",
    ),
    (
        "hangul",
        "ko",
        "다람쥐 헌 쳇바퀴에 타고파 키스의 고유조건은 입술끼리 만나야 하고",
    ),
    ("emoji", "en", "Family 👨‍👩‍👧‍👦 flag 🇨🇿 thumbs 👍🏽 done"),
];

fn missing_glyph(d: &Document) -> bool {
    d.diagnostics()
        .iter()
        .any(|e| e.to_string().contains("missing glyph"))
}

#[test]
fn every_script_renders_or_reports_and_wraps_in_a_narrow_box() {
    let mut covered = Vec::new();
    for &(name, lang, text) in CORPUS {
        let html = format!(
            "<!doctype html><style>body{{margin:0;font:16px sans-serif}}#w{{width:90px}}</style><p id=p lang={lang}>{text}</p><p id=w lang={lang}>{text}</p>"
        );
        let mut d = Document::parse(&html).unwrap();
        d.render(600, 800, 1.0).unwrap();
        let wide = d.viewport_rect(d.node("p").unwrap()).unwrap();
        let narrow = d.viewport_rect(d.node("w").unwrap()).unwrap();
        assert!(wide.2 > 0.0 && wide.3 > 0.0, "{name}: no box");
        // Every script breaks into several lines at 90 px (dictionary breaks for Thai, ideographs).
        assert!(
            narrow.3 >= wide.3 * 2.0,
            "{name}: did not wrap ({wide:?} vs {narrow:?})"
        );
        // The copied text is the logical text.
        assert_eq!(
            d.text_rects(Some("p"), None, None, None).unwrap()["text"],
            text,
            "{name}"
        );
        if !missing_glyph(&d) {
            covered.push(name);
        }
    }
    eprintln!("F0905 corpus: scripts with glyphs on this host: {covered:?}");
    assert!(covered.contains(&"latin"), "the bundled font covers Latin");
    // The detection itself: a private-use code point no font has is reported.
    let mut d = Document::parse("<!doctype html><p>\u{10FFFD}</p>").unwrap();
    d.render(200, 100, 1.0).unwrap();
    assert!(missing_glyph(&d), "{:?}", d.diagnostics());
}

#[test]
fn rtl_selection_geometry_and_vertical_text_keep_logical_offsets() {
    let mut d = Document::parse("<!doctype html><style>body{margin:0;font:20px sans-serif}#v{writing-mode:vertical-rl;height:200px}</style><p id=r dir=rtl>שלום עולם abc</p><div id=v lang=ja>縦書きの文章です</div>").unwrap();
    d.render(600, 400, 1.0).unwrap();
    // The first word of the right-to-left paragraph is drawn at its right end.
    let first = d.text_rects(Some("r"), None, Some(0), Some(4)).unwrap();
    let all = d.text_rects(Some("r"), None, None, None).unwrap();
    let x0 = |v: &Value| v["rects"][0]["x"].as_f64().unwrap();
    let right = |v: &Value| {
        v["rects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["x"].as_f64().unwrap() + r["width"].as_f64().unwrap())
            .fold(0.0, f64::max)
    };
    assert!(
        x0(&first) > x0(&all),
        "RTL: logical start is on the right: {first} {all}"
    );
    assert!((right(&first) - right(&all)).abs() < 1.0);
    // Vertical text: a column taller than wide.
    let v = d.viewport_rect(d.node("v").unwrap()).unwrap();
    let t = d.text_rects(Some("v"), None, None, None).unwrap();
    let r = &t["rects"][0];
    assert!(
        r["height"].as_f64().unwrap() > r["width"].as_f64().unwrap(),
        "vertical column {t}"
    );
    assert!(v.3 >= 199.0);
}
