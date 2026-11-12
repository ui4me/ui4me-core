// owner: 007-resources — shared test helpers (temp dirs, ids, settling).
#![allow(dead_code, clippy::collapsible_if, clippy::unnecessary_min_or_max)]

pub mod bombs;
pub mod monitor;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use ui4me_assets::ResourceManager;
use ui4me_assets::decode::fake::FakeDecoder;
use ui4me_assets::policy::{ResourcePolicy, RuntimeResourceModes};
use ui4me_assets::view::AppliedBatch;
use ui4me_engine_types::ids::{DocumentId, NodeId, RawId, Slots};
use url::Url;

pub const IMAGES_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-images/fake-corpus.json"
));
pub const BUNDLES_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-bundles/bundle-corpus.json"
));
pub const FONTS_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-fonts/synthetic-fonts.json"
));

fn corpus_entries(corpus: &str) -> Vec<(String, Vec<u8>)> {
    let v: serde_json::Value = serde_json::from_str(corpus).expect("corpus json");
    v["files"]
        .as_object()
        .expect("files")
        .iter()
        .map(|(k, e)| {
            let bytes = match (e.get("hex"), e.get("text")) {
                (Some(h), _) => {
                    let h = h.as_str().unwrap();
                    (0..h.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                        .collect()
                }
                (_, Some(t)) => t.as_str().unwrap().as_bytes().to_vec(),
                _ => panic!("corpus entry {k}"),
            };
            (k.clone(), bytes)
        })
        .collect()
}

/// The entries of bundle `bundle` in the resources-bundles corpus (`<bundle>/<path>` keys, optional `mime`).
pub fn bundle_inputs(bundle: &str) -> Vec<ui4me_assets::bundles::BundleEntryInput> {
    let v: serde_json::Value = serde_json::from_str(BUNDLES_CORPUS).expect("corpus json");
    let prefix = format!("{bundle}/");
    v["files"]
        .as_object()
        .expect("files")
        .iter()
        .filter_map(|(k, e)| {
            let path = k.strip_prefix(&prefix)?;
            let bytes: Vec<u8> = match (e.get("hex"), e.get("text")) {
                (Some(h), _) => {
                    let h = h.as_str().unwrap();
                    (0..h.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                        .collect()
                }
                (_, Some(t)) => t.as_str().unwrap().as_bytes().to_vec(),
                _ => panic!("corpus entry {k}"),
            };
            Some(ui4me_assets::bundles::BundleEntryInput::new(
                path,
                bytes,
                e.get("mime").and_then(|m| m.as_str()).map(str::to_string),
            ))
        })
        .collect()
}

/// Write every file of a fixture corpus (fake-corpus.json / synthetic-fonts.json) into `t`.
pub fn materialise(t: &TempDir, corpus: &str) {
    for (rel, bytes) in corpus_entries(corpus) {
        t.write(&rel, &bytes);
    }
}

/// One synthetic font container from the resources-fonts corpus.
pub fn synthetic_font(name: &str) -> Vec<u8> {
    corpus_entries(FONTS_CORPUS)
        .into_iter()
        .find(|(k, _)| k == name)
        .unwrap_or_else(|| panic!("no synthetic font {name}"))
        .1
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A unique temporary directory removed on drop. The path is canonical (symlinks resolved).
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("ui4me-res-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(std::fs::canonicalize(&p).expect("canonicalize temp dir"))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&p, bytes).expect("write file");
        p
    }
    pub fn url(&self, rel: &str) -> Url {
        Url::from_file_path(self.0.join(rel)).expect("file url")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn doc_id(n: u32) -> DocumentId {
    let mut s = Slots::new();
    let mut id = s.alloc();
    for _ in 0..n {
        id = s.alloc();
    }
    DocumentId::from_raw(RawId {
        index: id.index,
        generation: id.generation,
    })
}

pub fn node_id(n: u32) -> NodeId {
    let mut s = Slots::new();
    let mut id = s.alloc();
    for _ in 0..n {
        id = s.alloc();
    }
    NodeId::from_raw(RawId {
        index: id.index,
        generation: id.generation,
    })
}

pub fn manager(deterministic: bool) -> ResourceManager {
    manager_with(deterministic, ResourcePolicy::default())
}

/// A decoder wrapper that counts real decodes (to prove sharing).
pub struct CountingDecoder {
    pub decodes: std::sync::atomic::AtomicUsize,
    pub inner: FakeDecoder,
}

impl ui4me_assets::decode::RasterDecoder for CountingDecoder {
    fn formats(&self) -> &'static [ui4me_assets::decode::ImageFormat] {
        self.inner.formats()
    }
    fn probe(
        &self,
        f: ui4me_assets::decode::ImageFormat,
        b: &[u8],
    ) -> Result<ui4me_assets::decode::ImageHeader, ui4me_engine_types::status::Status> {
        self.inner.probe(f, b)
    }
    fn decode(
        &self,
        b: &[u8],
        h: &ui4me_assets::decode::ImageHeader,
        c: &ui4me_assets::worker::CancelToken,
    ) -> Result<ui4me_assets::decode::DecodedImage, ui4me_engine_types::status::Status> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        self.inner.decode(b, h, c)
    }
}

pub fn counting() -> Arc<CountingDecoder> {
    Arc::new(CountingDecoder {
        decodes: std::sync::atomic::AtomicUsize::new(0),
        inner: FakeDecoder,
    })
}

pub fn manager_counting(deterministic: bool, d: Arc<CountingDecoder>) -> ResourceManager {
    let modes = RuntimeResourceModes {
        deterministic_resources: deterministic,
        ..Default::default()
    };
    ResourceManager::new(modes, ResourcePolicy::default(), d).expect("manager")
}

/// Threads of this process (Linux), for "no worker thread was created" assertions.
pub fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|r| r.count())
        .unwrap_or(0)
}

/// Wait (test side) until `n` completions are queued, without applying them.
pub fn wait_queued(mgr: &ResourceManager, n: usize) {
    let end = Instant::now() + Duration::from_secs(20);
    while mgr.queued_completions() < n {
        assert!(Instant::now() < end, "completions never arrived");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn manager_with(deterministic: bool, defaults: ResourcePolicy) -> ResourceManager {
    let modes = RuntimeResourceModes {
        deterministic_resources: deterministic,
        ..Default::default()
    };
    ResourceManager::new(modes, defaults, Arc::new(FakeDecoder)).expect("manager")
}

/// Drain until nothing is pending (polling, never blocking the engine): the test side waits.
pub fn settle(mgr: &mut ResourceManager, doc: DocumentId) -> Vec<AppliedBatch> {
    let mut out = Vec::new();
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        out.extend(mgr.process_completions());
        if mgr.pending(doc).unwrap_or(0) == 0 {
            return out;
        }
        assert!(Instant::now() < end, "resources did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
