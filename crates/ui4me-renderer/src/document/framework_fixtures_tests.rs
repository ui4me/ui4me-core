//! 0.9 F0910: framework-style fixtures without a browser runtime. Hand-written markup and CSS in
//! the style of Bootstrap (components with custom-property themes), Tailwind (utility classes
//! with `min-width` breakpoints) and Mantine (CSS-variable driven app shell), rendered at phone,
//! tablet and desktop widths with geometry checks. No framework files are vendored; no script
//! runs. Unsupported-feature diagnostics are collected and must be in the documented exception
//! list (none today).
use super::*;

const WIDTHS: [u32; 3] = [375, 768, 1280];

/// Exceptions the fixtures may report (feature ids). Kept empty unless documented.
const EXCEPTIONS: &[&str] = &[];

fn render(html: &str, width: u32) -> Document {
    let mut d = Document::parse(html).unwrap();
    d.render(width, 900, 1.0).unwrap();
    d
}

fn rect(d: &Document, id: &str) -> (f64, f64, f64, f64) {
    d.viewport_rect(d.node(id).unwrap())
        .unwrap_or_else(|| panic!("{id} has no box"))
}

fn unsupported(d: &Document) -> Vec<String> {
    d.diagnostics()
        .iter()
        .filter(|e| e["code"] == "UNSUPPORTED_FEATURE")
        .filter_map(|e| e["feature"].as_str().map(str::to_owned))
        .filter(|f| !EXCEPTIONS.contains(&f.as_str()))
        .collect()
}

const BOOTSTRAP_STYLE: &str = r#"<!doctype html><html><head><style>
:root{--bs-primary:#0d6efd;--bs-body-font-size:1rem;--bs-gutter-x:1.5rem;--bs-border-radius:.375rem}
*,*::before,*::after{box-sizing:border-box}
body{margin:0;font-family:system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;font-size:var(--bs-body-font-size);line-height:1.5}
.container{width:100%;padding-right:calc(var(--bs-gutter-x)*.5);padding-left:calc(var(--bs-gutter-x)*.5);margin-right:auto;margin-left:auto}
@media (min-width:576px){.container{max-width:540px}}
@media (min-width:768px){.container{max-width:720px}}
@media (min-width:1200px){.container{max-width:1140px}}
.navbar{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;padding:.5rem 1rem;background:var(--bs-primary)}
.navbar-brand{font-size:1.25rem;color:#fff;text-decoration:none}
.navbar-nav{display:flex;flex-direction:column;padding-left:0;margin:0;list-style:none}
@media (min-width:992px){.navbar-nav{flex-direction:row}.nav-link{padding:0 .5rem}}
.nav-link{display:block;color:rgba(255,255,255,.75);text-decoration:none}
.row{display:flex;flex-wrap:wrap;margin-right:calc(-.5*var(--bs-gutter-x));margin-left:calc(-.5*var(--bs-gutter-x))}
.row>*{flex-shrink:0;width:100%;max-width:100%;padding-right:calc(var(--bs-gutter-x)*.5);padding-left:calc(var(--bs-gutter-x)*.5)}
@media (min-width:768px){.col-md-4{flex:0 0 auto;width:33.33333333%}.col-md-8{flex:0 0 auto;width:66.66666667%}}
.form-control{display:block;width:100%;padding:.375rem .75rem;font-size:1rem;border:1px solid #dee2e6;border-radius:var(--bs-border-radius)}
.btn{display:inline-block;padding:.375rem .75rem;border:1px solid transparent;border-radius:var(--bs-border-radius);background:var(--bs-primary);color:#fff}
.table{width:100%;border-collapse:collapse}.table td,.table th{padding:.5rem;border-bottom:1px solid #dee2e6}
.d-none{display:none!important}@media (min-width:768px){.d-md-block{display:block!important}}
.custom .btn{background:#198754}
</style></head><body>
<nav class=navbar id=nav><a class=navbar-brand id=brand href=#>Brand</a><ul class=navbar-nav id=links><li><a class=nav-link id=l1 href=#>Home</a></li><li><a class=nav-link id=l2 href=#>Docs</a></li></ul></nav>
<div class=container id=container><div class=row>
<aside class=col-md-4 id=side><div class="d-none d-md-block" id=wide-only>Sidebar</div></aside>
<main class=col-md-8 id=main><form><input class=form-control id=email placeholder=Email><button class=btn id=go>Go</button></form>
<table class=table id=table><tr><th>Name</th><th>Role</th></tr><tr><td>Ada</td><td>Engineer</td></tr></table>
<div class=custom><button class=btn id=custom>Override</button></div></main></div></div>
</body></html>"#;

#[test]
fn bootstrap_style_layout_survives_breakpoints() {
    for w in WIDTHS {
        let d = render(BOOTSTRAP_STYLE, w);
        assert!(unsupported(&d).is_empty(), "{w}: {:?}", unsupported(&d));
        let (container, side, main) = (rect(&d, "container"), rect(&d, "side"), rect(&d, "main"));
        match w {
            375 => {
                assert_eq!(container.2, 375.0, "fluid container");
                assert!(main.1 >= side.1 + side.3, "stacked columns at {w}");
                assert!(
                    d.viewport_rect(d.node("wide-only").unwrap()).is_none(),
                    "d-none below md"
                );
            }
            768 => {
                assert_eq!(container.2, 720.0, "md container");
                assert!(
                    (main.1 - side.1).abs() < 1.0 && main.0 > side.0,
                    "side by side at {w}"
                );
                assert!(
                    (side.2 / (side.2 + main.2) - 1.0 / 3.0).abs() < 0.01,
                    "4/8 split"
                );
                assert!(
                    rect(&d, "wide-only").3 > 0.0,
                    "d-md-block overrides d-none (!important order)"
                );
            }
            _ => assert_eq!(container.2, 1140.0, "xl container"),
        }
        // Nav links stack below lg, run in a row from lg.
        let (l1, l2) = (rect(&d, "l1"), rect(&d, "l2"));
        if w >= 992 {
            assert!((l1.1 - l2.1).abs() < 1.0 && l2.0 > l1.0, "row nav at {w}");
        } else {
            assert!(l2.1 > l1.1, "column nav at {w}");
        }
        // The form control fills its column; the table spans it.
        assert!(
            (rect(&d, "email").2 - (main.2 - 24.0)).abs() < 1.0,
            "form-control width at {w}"
        );
        assert!((rect(&d, "table").2 - (main.2 - 24.0)).abs() < 1.0);
        // A later, more specific rule overrides the component colour.
        let mut d = d;
        let bg = d
            .computed_style(Some("custom"), None, &["background-color".into()])
            .unwrap();
        assert_eq!(
            bg["properties"]["background-color"]["value"],
            "rgb(25, 135, 84)"
        );
    }
}

const TAILWIND_STYLE: &str = r#"<!doctype html><html><head><style>
*,::before,::after{box-sizing:border-box;border:0 solid #e5e7eb}
body{margin:0;font-family:ui-sans-serif,system-ui,sans-serif;line-height:1.5}
.flex{display:flex}.grid{display:grid}.hidden{display:none}.block{display:block}
.flex-col{flex-direction:column}.shrink-0{flex-shrink:0}.items-center{align-items:center}.justify-between{justify-content:space-between}.gap-4{gap:1rem}
.grid-cols-1{grid-template-columns:repeat(1,minmax(0,1fr))}.p-4{padding:1rem}.px-6{padding-left:1.5rem;padding-right:1.5rem}
.w-full{width:100%}.max-w-7xl{max-width:80rem}.mx-auto{margin-left:auto;margin-right:auto}.rounded-lg{border-radius:.5rem}
.shadow{box-shadow:0 1px 3px 0 rgb(0 0 0/.1),0 1px 2px -1px rgb(0 0 0/.1)}.bg-white{--tw-bg-opacity:1;background-color:rgb(255 255 255/var(--tw-bg-opacity))}
.text-sm{font-size:.875rem;line-height:1.25rem}.truncate{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
@media (min-width:640px){.sm\:grid-cols-2{grid-template-columns:repeat(2,minmax(0,1fr))}}
@media (min-width:768px){.md\:flex-row{flex-direction:row}.md\:block{display:block}.md\:w-64{width:16rem}}
@media (min-width:1024px){.lg\:grid-cols-4{grid-template-columns:repeat(4,minmax(0,1fr))}}
</style></head><body>
<header class="flex items-center justify-between px-6 p-4 shadow" id=header><span id=logo>Logo</span><button id=menu>Menu</button></header>
<div class="flex flex-col md:flex-row" id=shell>
<nav class="hidden md:block md:w-64 shrink-0 p-4" id=sidebar>Nav</nav>
<main class="w-full max-w-7xl mx-auto p-4" id=content>
<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4" id=cards>
<div class="bg-white rounded-lg shadow p-4" id=c1><p class="text-sm truncate" id=t1>A very long card title that will not fit in a narrow card</p></div>
<div class="bg-white rounded-lg shadow p-4" id=c2>Two</div><div class="bg-white rounded-lg shadow p-4" id=c3>Three</div><div class="bg-white rounded-lg shadow p-4" id=c4>Four</div>
</div></main></div></body></html>"#;

#[test]
fn tailwind_style_utilities_respond_to_breakpoints() {
    for w in WIDTHS {
        let d = render(TAILWIND_STYLE, w);
        assert!(unsupported(&d).is_empty(), "{w}: {:?}", unsupported(&d));
        let (c1, c2, c4) = (rect(&d, "c1"), rect(&d, "c2"), rect(&d, "c4"));
        let columns = match w {
            375 => 1,
            768 => 2,
            _ => 4,
        };
        let row_of = |r: (f64, f64, f64, f64)| (r.1 - c1.1).abs() < 1.0;
        let in_first_row = [c1, c2, rect(&d, "c3"), c4]
            .iter()
            .filter(|r| row_of(**r))
            .count();
        assert_eq!(in_first_row, columns, "grid columns at {w}");
        let sidebar = d.viewport_rect(d.node("sidebar").unwrap());
        if w >= 768 {
            let s = sidebar.expect("sidebar shown from md");
            assert_eq!(s.2, 256.0, "md:w-64");
            assert!(
                rect(&d, "content").0 >= s.0 + s.2 - 0.5,
                "content beside the sidebar"
            );
        } else {
            assert!(sidebar.is_none(), "hidden below md");
        }
        // The truncated title stays one line inside its card.
        let t1 = rect(&d, "t1");
        assert!(t1.3 <= 21.0, "truncate keeps one line at {w}: {t1:?}");
        assert!(t1.2 <= c1.2, "inside the card");
        let header = rect(&d, "header");
        assert_eq!(header.2, w as f64, "header spans the viewport");
    }
}

const MANTINE_STYLE: &str = r#"<!doctype html><html><head><style>
:root{--mantine-spacing-md:1rem;--mantine-color-body:#fff;--mantine-color-text:#000;--app-shell-navbar-width:300px;--app-shell-header-height:60px}
@media (prefers-color-scheme:dark){:root{--mantine-color-body:#242424;--mantine-color-text:#c9c9c9}}
body{margin:0;background:var(--mantine-color-body);color:var(--mantine-color-text);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}
.app-header{position:fixed;top:0;left:0;right:0;height:var(--app-shell-header-height);border-bottom:1px solid #dee2e6;background:inherit;z-index:100}
.app-navbar{position:fixed;top:var(--app-shell-header-height);bottom:0;left:0;width:var(--app-shell-navbar-width);border-right:1px solid #dee2e6;overflow-y:auto}
.app-main{padding-top:var(--app-shell-header-height);padding-left:var(--app-shell-navbar-width);min-height:100vh;box-sizing:border-box}
@media (max-width:48em){.app-navbar{display:none}.app-main{padding-left:0}}
.stack{display:flex;flex-direction:column;gap:var(--mantine-spacing-md);padding:var(--mantine-spacing-md)}
.group{display:flex;flex-wrap:wrap;gap:.5rem;align-items:center}
.input{width:100%;height:2.25rem;padding:0 .75rem;border:1px solid #ced4da;border-radius:.25rem;box-sizing:border-box}
.paper{padding:var(--mantine-spacing-md);border-radius:.5rem;box-shadow:0 1px 3px rgba(0,0,0,.05)}
</style></head><body>
<header class=app-header id=hdr>Header</header><nav class=app-navbar id=navbar><div style="height:2000px">Links</div></nav>
<main class=app-main id=appmain><div class=stack><div class=group id=group><button id=b1>One</button><button id=b2>Two</button></div>
<input class=input id=search placeholder=Search><div class=paper id=paper>Card</div></div></main></body></html>"#;

#[test]
fn mantine_style_app_shell_fixed_regions_and_variables() {
    for w in WIDTHS {
        let mut d = render(MANTINE_STYLE, w);
        assert!(unsupported(&d).is_empty(), "{w}: {:?}", unsupported(&d));
        let hdr = rect(&d, "hdr");
        // 60 px content box + the 1 px bottom border (content-box sizing).
        assert_eq!(
            (hdr.1, hdr.2, hdr.3),
            (0.0, w as f64, 61.0),
            "fixed header at {w}"
        );
        let main = rect(&d, "appmain");
        let search = rect(&d, "search");
        if w > 768 {
            let nav = rect(&d, "navbar");
            assert_eq!(
                (nav.0, nav.1, nav.2),
                (0.0, 60.0, 301.0),
                "fixed navbar (+ its right border)"
            );
            assert!(
                search.0 >= 300.0 + 16.0 - 0.5,
                "content right of the navbar"
            );
        } else {
            assert!(
                d.viewport_rect(d.node("navbar").unwrap()).is_none(),
                "navbar hidden at {w}"
            );
            assert!((search.0 - 16.0).abs() < 0.5, "content uses the full width");
        }
        assert!(main.3 >= 900.0, "min-height: 100vh");
        let (b1, b2) = (rect(&d, "b1"), rect(&d, "b2"));
        assert!(
            (b1.1 - b2.1).abs() < 1.0 && b2.0 - (b1.0 + b1.2) >= 7.5,
            "flex gap"
        );
        // The page scrolls; the fixed header stays.
        d.scroll_to(None, None, Some(300.0)).unwrap();
        d.render(w, 900, 1.0).unwrap();
        assert_eq!(rect(&d, "hdr").1, 0.0);
    }
    // The dark scheme switches the custom-property theme.
    let html = MANTINE_STYLE.replace(
        "<header class=app-header",
        "<p id=probe>t</p><header class=app-header",
    );
    let mut d = Document::parse(&html).unwrap();
    d.render(1280, 900, 1.0).unwrap();
    let color = |d: &mut Document| {
        d.computed_style(Some("probe"), None, &["color".into()])
            .unwrap()["properties"]["color"]["value"]
            .clone()
    };
    assert_eq!(color(&mut d), "rgb(0, 0, 0)");
    d.set_system_color_scheme(Some(true)).unwrap();
    d.render(1280, 900, 1.0).unwrap();
    assert_eq!(color(&mut d), "rgb(201, 201, 201)");
}

#[test]
fn tailwind_v4_style_layers_registered_properties_and_modern_colours() {
    let html = r#"<!doctype html><style>
@layer theme, base, components, utilities;
@property --tw-shadow-color{syntax:"*";inherits:false}
@layer theme{:root{--color-blue-500:oklch(62.3% 0.214 259.815);--spacing:.25rem}}
@layer utilities{.p-4{padding:calc(var(--spacing)*4)}.bg-blue-500{background-color:var(--color-blue-500)}.text-mix{color:color-mix(in oklab,var(--color-blue-500) 50%,transparent)}}
@layer components{.card{padding:2px;background-color:red}}
</style><div class="card p-4 bg-blue-500 text-mix" id=card>x</div>"#;
    let mut d = render(html, 768);
    assert!(unsupported(&d).is_empty(), "{:?}", unsupported(&d));
    // Later layers win over earlier ones regardless of source order.
    let s = d
        .computed_style(
            Some("card"),
            None,
            &[
                "padding-top".into(),
                "background-color".into(),
                "color".into(),
            ],
        )
        .unwrap();
    assert_eq!(
        s["properties"]["padding-top"]["value"], "16px",
        "utilities beat components"
    );
    let bg = s["properties"]["background-color"]["value"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(bg.starts_with("oklch(") || bg.starts_with("rgb("), "{bg}");
    assert_ne!(bg, "rgb(255, 0, 0)");
    let color = s["properties"]["color"]["value"].as_str().unwrap();
    assert!(
        color.contains("oklab") || color.starts_with("rgba(") || color.contains("/ 0.5"),
        "{color}"
    );
}
