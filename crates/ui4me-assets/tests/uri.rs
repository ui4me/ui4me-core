// owner: 007-resources (T007) — URL resolution vectors (contracts/uri-resolution.md §1–§5).

use ui4me_assets::policy::{self, Decision, ResourcePolicy};
use ui4me_assets::uri::{self, Scheme};
use ui4me_engine_types::codes_generated::{Code, SubReason};
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn normative_vectors_section_5() {
    // (document URL, <base href>, reference) -> resolved
    let rows = [
        (
            "file:///app/ui/index.html",
            None,
            "img/a.png",
            "file:///app/ui/img/a.png",
        ),
        (
            "file:///app/ui/index.html",
            None,
            "../secret.txt",
            "file:///app/secret.txt",
        ),
        (
            "file:///app/ui/index.html",
            Some("http://cdn/"),
            "a.png",
            "http://cdn/a.png",
        ),
        (
            "ui4me-res://app/index.html",
            None,
            "../img/a.png",
            "ui4me-res://app/img/a.png",
        ),
        (
            "file:///app/ui/css/site.css",
            None,
            "../img/bg.jpg",
            "file:///app/ui/img/bg.jpg",
        ),
        (
            "file:///C:/app/ui/index.html",
            None,
            "%2E%2E/%2e%2e/Windows/win.ini",
            "file:///C:/Windows/win.ini",
        ),
    ];
    for (doc, base_href, reference, want) in rows {
        let base = uri::document_base_url(&u(doc), base_href);
        assert_eq!(
            uri::resolve(&base, reference).unwrap().as_str(),
            want,
            "{doc} + {reference}"
        );
    }
}

#[test]
fn http_base_is_denied_by_policy() {
    let r = uri::resolve(
        &uri::document_base_url(&u("file:///app/ui/index.html"), Some("http://cdn/")),
        "a.png",
    )
    .unwrap();
    assert_eq!(
        policy::check(&r, &ResourcePolicy::default()),
        Decision::Deny(Code::ResourceDenied)
    );
}

#[test]
fn about_blank_with_relative_reference_is_no_base_url() {
    let e = uri::resolve(&uri::about_blank(), "a.png").unwrap_err();
    assert_eq!(e.code, Code::UriMalformed);
    assert_eq!(e.sub_reason, Some(SubReason::NoBaseUrl));
    // an absolute reference still works with no base
    assert_eq!(
        uri::resolve(&uri::about_blank(), "file:///x/a.png")
            .unwrap()
            .as_str(),
        "file:///x/a.png"
    );
    assert_eq!(
        uri::resolve(&u("file:///a/b.html"), "http://[::1")
            .unwrap_err()
            .code,
        Code::UriMalformed
    );
}

#[test]
fn document_url_rules() {
    assert_eq!(
        uri::document_url_from_string(None).unwrap().as_str(),
        "about:blank"
    );
    assert_eq!(
        uri::document_url_from_string(Some("file:///app/"))
            .unwrap()
            .as_str(),
        "file:///app/"
    );
    assert_eq!(
        uri::document_url_from_string(Some("no scheme"))
            .unwrap_err()
            .code,
        Code::UriMalformed
    );
    let dir = std::env::temp_dir();
    let f = dir.join(format!("ui4me-uri-doc-{}.html", std::process::id()));
    std::fs::write(&f, b"<p>").unwrap();
    let url = uri::document_url_from_file(&f).unwrap();
    assert_eq!(url.scheme(), "file");
    assert!(url.path().ends_with(".html"));
    std::fs::remove_file(&f).unwrap();
    assert_eq!(
        uri::document_url_from_file(&f).unwrap_err().code,
        Code::FileNotFound
    );
}

#[test]
fn first_base_href_wins_and_bad_hrefs_are_ignored() {
    let doc = u("file:///app/ui/index.html");
    assert_eq!(
        uri::document_base_url(&doc, Some("sub/")).as_str(),
        "file:///app/ui/sub/"
    );
    assert_eq!(
        uri::document_base_url(&doc, Some("  ../x/ ")).as_str(),
        "file:///app/x/"
    );
    assert_eq!(
        uri::document_base_url(&doc, Some("http://[bad")).as_str(),
        doc.as_str()
    );
    assert_eq!(uri::document_base_url(&doc, None).as_str(), doc.as_str());
    // a relative <base href> against about:blank cannot form a base: the document URL stays
    assert_eq!(
        uri::document_base_url(&uri::about_blank(), Some("x/")).as_str(),
        "about:blank"
    );
}

#[test]
fn fragments_are_ignored_for_loading_but_kept() {
    let r = uri::resolve(&u("file:///app/index.html"), "a.png#frag").unwrap();
    assert_eq!(r.fragment(), Some("frag"));
    assert_eq!(uri::without_fragment(&r).as_str(), "file:///app/a.png");
}

#[test]
fn scheme_dispatch() {
    assert_eq!(Scheme::of(&u("file:///a")), Scheme::File);
    assert_eq!(Scheme::of(&u("data:,x")), Scheme::Data);
    assert_eq!(Scheme::of(&u("ui4me-res://a/b")), Scheme::UiRes);
    assert_eq!(Scheme::of(&u("https://a/b")), Scheme::Other);
}

#[test]
fn unix_file_url_to_path() {
    assert_eq!(
        uri::unix_path(&u("file:///app/ui/a%20b.png"))
            .unwrap()
            .to_str(),
        Some("/app/ui/a b.png")
    );
    assert_eq!(
        uri::unix_path(&u("file://localhost/app/a.png"))
            .unwrap()
            .to_str(),
        Some("/app/a.png")
    );
    // a host means a network share: denied on Unix
    assert_eq!(
        uri::unix_path(&u("file://server/share/a.png")),
        Err(Code::ResourceDenied)
    );
    // encoded separators and NUL cannot smuggle a traversal
    assert_eq!(
        uri::unix_path(&u("file:///app/..%2f..%2fetc/passwd")),
        Err(Code::ResourceDenied)
    );
    assert_eq!(
        uri::unix_path(&u("file:///app/a%00.png")),
        Err(Code::ResourceDenied)
    );
    // dot segments are removed by the parser, never reaching the path
    assert_eq!(
        uri::unix_path(&u("file:///app/ui/%2e%2e/%2E%2E/etc/passwd"))
            .unwrap()
            .to_str(),
        Some("/etc/passwd")
    );
}

#[test]
fn windows_file_url_mapping() {
    let w = |s: &str| uri::windows_path(&u(s));
    let ok = w("file:///C:/app/ui/a.png").unwrap();
    assert_eq!((ok.path.as_str(), ok.unc), (r"C:\app\ui\a.png", false));
    let ok = w("file:///c:/App/A%20B.png").unwrap();
    assert_eq!(ok.path, r"C:\App\A B.png");
    let unc = w("file://server/share/dir/a.png").unwrap();
    assert_eq!(
        (unc.path.as_str(), unc.unc),
        (r"\\server\share\dir\a.png", true)
    );
    // denied lexically
    for bad in [
        "file:///C:/app/a.png:stream",
        "file:///C:/app/NUL",
        "file:///C:/app/COM1.txt",
        "file:///C:/app/a%5C..%5Cb.png",
        "file:///C:/app/a%2F..%2Fb.png",
        "file:///app/no-drive.png",
        "file://./pipe/x",
        "file:///C:/app/a.png.",
    ] {
        assert_eq!(w(bad), Err(Code::ResourceDenied), "{bad}");
    }
    // traversal that survives parsing is denied by the root check later; the dot segments are gone
    assert_eq!(
        w("file:///C:/app/ui/%2e%2e/%2e%2e/Windows/win.ini")
            .unwrap()
            .path,
        r"C:\Windows\win.ini"
    );
}
