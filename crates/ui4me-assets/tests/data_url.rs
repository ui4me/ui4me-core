// owner: 007-resources (T036 data part, T041) — the Fetch "data: URL processor" through the manager
// (acceptance 2): payload limit -> SIZE_LIMIT, malformed -> URI_MALFORMED, disabled ->
// RESOURCE_DENIED, content-keyed sharing, and nothing outside `data:` is read.

mod support;

use support::*;
use ui4me_assets::policy::{ResourcePolicy, ResourcePolicyPatch};
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::{ImageState, PaintImage};
use ui4me_engine_types::codes_generated::Code;
use url::Url;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(b: &[u8]) -> String {
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn fake_url(w: u32, h: u32) -> String {
    let bytes = ui4me_assets::decode::fake::FakeImage::new(w, h).bytes();
    format!("data:image/png;base64,{}", base64(&bytes))
}

struct Page {
    mgr: ui4me_assets::ResourceManager,
    doc: ui4me_engine_types::ids::DocumentId,
    base: Url,
    _tmp: TempDir,
}

fn page(deterministic: bool, defaults: ResourcePolicy) -> Page {
    let tmp = TempDir::new("data-url");
    let mut mgr = manager_with(deterministic, defaults);
    let doc = doc_id(1);
    let f = tmp.write("index.html", b"<html></html>");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    Page {
        mgr,
        doc,
        base,
        _tmp: tmp,
    }
}

impl Page {
    fn load(&mut self, node: u32, src: &str) -> ImageState {
        let r = self
            .mgr
            .request_image(
                self.doc,
                Requester::ImgElement(node_id(node)),
                &self.base,
                src,
                1.0,
            )
            .unwrap();
        settle(&mut self.mgr, self.doc);
        self.mgr.natural_size(&r)
    }
}

fn limit(n: u64) -> ResourcePolicy {
    ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            data_url_max_bytes: Some(n),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn a_data_url_image_decodes_and_is_shared_by_content() {
    for det in [false, true] {
        let mut p = page(det, ResourcePolicy::default());
        let src = fake_url(6, 4);
        assert_eq!(
            p.load(1, &src),
            ImageState::Ready {
                width_css: Some(6.0),
                height_css: Some(4.0),
                ratio: Some((6.0, 4.0))
            }
        );
        let r2 = p
            .mgr
            .request_image(p.doc, Requester::ImgElement(node_id(2)), &p.base, &src, 1.0)
            .unwrap();
        settle(&mut p.mgr, p.doc);
        assert!(matches!(
            p.mgr.paint_image(&r2, (6, 4)),
            PaintImage::Raster { .. }
        ));
        assert_eq!(
            p.mgr.runtime_diagnostics().cache.entries,
            1,
            "one cache entry for identical payloads (det={det})"
        );
    }
}

#[test]
fn payload_over_the_limit_is_size_limit_and_malformed_is_uri_malformed() {
    for det in [false, true] {
        let mut p = page(det, limit(16));
        assert_eq!(
            p.load(1, &fake_url(6, 4)),
            ImageState::Broken {
                code: Code::SizeLimit
            },
            "23-byte payload over a 16-byte limit"
        );
        assert_eq!(
            p.load(2, "data:image/png;base64,@@@@"),
            ImageState::Broken {
                code: Code::UriMalformed
            }
        );
        assert_eq!(
            p.load(3, "data:image/png;base64"),
            ImageState::Broken {
                code: Code::UriMalformed
            }
        );
    }
}

#[test]
fn disabled_data_urls_are_resource_denied() {
    let mut p = page(true, limit(0));
    assert_eq!(
        p.load(1, &fake_url(2, 2)),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
}

#[test]
fn a_data_url_that_is_not_an_image_is_unsupported_media_type() {
    let mut p = page(true, ResourcePolicy::default());
    assert_eq!(
        p.load(1, "data:text/plain,hello"),
        ImageState::Broken {
            code: Code::UnsupportedMediaType
        }
    );
}

// This observation uses /proc socket monitoring; the other cases also run on Windows.
#[cfg(unix)]
#[test]
fn data_urls_read_nothing_from_the_file_system() {
    // Sockets inherited from the launching process do not count; none may be opened here (as policy_unix.rs).
    let sockets_before = support::monitor::sockets();
    let mut p = page(false, ResourcePolicy::default());
    let _ = p.load(1, &fake_url(3, 3));
    assert_eq!(
        support::monitor::sockets(),
        sockets_before,
        "no network path was exercised"
    );
    // the data: path has no file-system code at all: the processor module names no I/O API
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/data_url.rs"),
    )
    .unwrap();
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .take_while(|l| !l.contains("#[cfg(test)]"))
        .collect::<Vec<_>>()
        .join("\n");
    for needle in ["std::fs", "std::net", "File::open"] {
        assert!(
            !code.contains(needle),
            "data_url.rs must not use `{needle}`"
        );
    }
}
