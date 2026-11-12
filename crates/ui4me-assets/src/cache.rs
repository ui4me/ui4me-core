//! owner: 007-resources (T028, T069, T080) — the runtime-wide cache.
//!
//! Key: canonical URL (no fragment) plus content identity (size + mtime for files, generation for
//! bundles, SHA-256 of the payload for `data:`). One decode is shared by many requesters;
//! per-document reference counts keep entries alive; unreferenced entries are evicted LRU beyond
//! `cache_budget`; a hard decoded-memory budget bounds the sum of entries and in-flight decodes.
//! Entries are immutable after publication and handed out as `Arc`s, so eviction never changes a
//! snapshot a consumer already holds.

use crate::fs_guard::FileIdentity;
use crate::view::ImageRaster;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use ui4me_engine_types::ids::DocumentId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentKey {
    File(FileIdentity),
    Data([u8; 32]),
    Bundle(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    pub url: String,
    pub content: ContentKey,
}

/// Natural size in CSS px before density correction (`None` = no natural dimension).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalSize {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub ratio: Option<(f32, f32)>,
}

/// The published, immutable result of one decode.
#[derive(Debug)]
pub struct CachedImage {
    pub key: ResourceKey,
    pub raster: Arc<ImageRaster>,
    /// Original oriented size in image pixels (a downscaled raster keeps its original natural size).
    pub natural_px: (u32, u32),
    pub downscaled: bool,
    pub animated_first_frame: bool,
    pub icc_ignored: bool,
    pub encoded_bytes: u64,
    pub decode_ms: u64,
    /// Changes whenever the pixels for this key are replaced.
    pub generation: u64,
    /// A parsed SVG (T049): the entry then has only a one-pixel placeholder raster and is drawn at
    /// device scale by `svg::raster_cache`.
    pub vector: Option<Arc<crate::svg::VectorImage>>,
    /// `UNSUPPORTED_FEATURE` keys found while loading (`svg.text`), reported once per request.
    pub reports: Vec<&'static str>,
    /// Non-fatal canonical codes (a `RESOURCE_DENIED` per refused SVG href, ...).
    pub warnings: Vec<ui4me_engine_types::codes_generated::Code>,
}

impl CachedImage {
    pub fn decoded_bytes(&self) -> u64 {
        self.raster.data.len() as u64 + self.vector.as_ref().map_or(0, |v| v.est_bytes)
    }
}

struct Slot {
    image: Arc<CachedImage>,
    refs: HashMap<DocumentId, u32>,
    /// Completions published but not yet applied or discarded: such entries are not evictable.
    pins: u32,
    last_use: u64,
}

/// Cache counters for diagnostics (`ResourceDiagnosticRecord` runtime stats).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: u64,
    pub referenced_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub reserved: u64,
}

pub struct Cache {
    map: HashMap<ResourceKey, Slot>,
    // Eviction removes lookup ownership, but a committed scene may still own
    // the raster. Weak accounting keeps those bytes charged until it releases.
    retired: Vec<(Weak<[u8]>, u64)>,
    retired_vectors: Vec<(Weak<crate::svg::VectorImage>, u64)>,
    clock: u64,
    next_generation: u64,
    pub cache_budget: u64,
    pub memory_budget: u64,
    reserved: u64,
    key_locks: HashMap<ResourceKey, Arc<Mutex<()>>>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("stats", &self.stats())
            .finish()
    }
}

/// Scope of `resources_purge`.
#[derive(Clone, Debug)]
pub enum PurgeScope {
    Runtime,
    Document(DocumentId),
}

impl Cache {
    pub fn new(cache_budget: u64, memory_budget: u64) -> Self {
        Cache {
            map: HashMap::new(),
            retired: Vec::new(),
            retired_vectors: Vec::new(),
            clock: 0,
            next_generation: 1,
            cache_budget,
            memory_budget,
            reserved: 0,
            key_locks: HashMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The per-key decode lock: workers that miss on the same key serialise on it, so one decode
    /// is shared by every requester (the second worker re-checks the cache after acquiring it).
    pub fn key_lock(&mut self, key: &ResourceKey) -> Arc<Mutex<()>> {
        self.key_locks.entry(key.clone()).or_default().clone()
    }

    /// Forget a key lock nobody else holds.
    pub fn drop_key_lock(&mut self, key: &ResourceKey) {
        if self
            .key_locks
            .get(key)
            .is_some_and(|l| Arc::strong_count(l) == 1)
        {
            self.key_locks.remove(key);
        }
    }

    pub fn next_generation(&mut self) -> u64 {
        let g = self.next_generation;
        self.next_generation += 1;
        g
    }

    /// Look up a published entry (counts a hit or a miss and refreshes its LRU position).
    pub fn lookup(&mut self, key: &ResourceKey) -> Option<Arc<CachedImage>> {
        let t = self.tick();
        match self.map.get_mut(key) {
            Some(s) => {
                s.last_use = t;
                self.hits += 1;
                Some(s.image.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Publish an entry (replacing any entry under the same key; references carry over).
    pub fn insert(&mut self, image: Arc<CachedImage>) {
        self.insert_with_pin(image, false);
    }

    /// Publish an entry and pin it for the completion that will carry it to the owner thread.
    pub fn insert_with_pin(&mut self, image: Arc<CachedImage>, pin: bool) {
        let t = self.tick();
        let old = self.map.remove(&image.key);
        let (refs, pins) = old
            .map(|s| {
                if !Arc::ptr_eq(&s.image, &image) {
                    self.track_retired(&s.image);
                }
                (s.refs, s.pins)
            })
            .unwrap_or_default();
        self.map.insert(
            image.key.clone(),
            Slot {
                image,
                refs,
                pins: pins + pin as u32,
                last_use: t,
            },
        );
        self.evict_unreferenced_beyond_budget();
    }

    /// A cache hit for a completion: counts the hit and pins the entry until the completion is applied.
    pub fn lookup_pinned(&mut self, key: &ResourceKey) -> Option<Arc<CachedImage>> {
        let hit = self.lookup(key)?;
        if let Some(s) = self.map.get_mut(key) {
            s.pins += 1;
        }
        Some(hit)
    }

    /// Release the pin of an applied or discarded completion.
    pub fn unpin(&mut self, key: &ResourceKey) {
        if let Some(s) = self.map.get_mut(key) {
            s.pins = s.pins.saturating_sub(1);
        }
        self.evict_unreferenced_beyond_budget();
    }

    pub fn add_ref(&mut self, doc: DocumentId, key: &ResourceKey) {
        if let Some(s) = self.map.get_mut(key) {
            *s.refs.entry(doc).or_insert(0) += 1;
        }
    }

    pub fn release_ref(&mut self, doc: DocumentId, key: &ResourceKey) {
        if let Some(s) = self.map.get_mut(key) {
            if let Some(n) = s.refs.get_mut(&doc) {
                *n -= 1;
                if *n == 0 {
                    s.refs.remove(&doc);
                }
            }
        }
        self.evict_unreferenced_beyond_budget();
    }

    /// Drop every reference a closed document holds.
    pub fn release_document(&mut self, doc: DocumentId) {
        for s in self.map.values_mut() {
            s.refs.remove(&doc);
        }
        self.evict_unreferenced_beyond_budget();
    }

    fn total_bytes(&self) -> u64 {
        self.map
            .values()
            .map(|s| s.image.decoded_bytes())
            .sum::<u64>()
            + self
                .retired
                .iter()
                .filter(|(raster, _)| raster.strong_count() > 0)
                .map(|(_, bytes)| *bytes)
                .sum::<u64>()
            + self
                .retired_vectors
                .iter()
                .filter(|(vector, _)| vector.strong_count() > 0)
                .map(|(_, bytes)| *bytes)
                .sum::<u64>()
    }

    fn track_retired(&mut self, image: &Arc<CachedImage>) {
        self.retired.retain(|(raster, _)| raster.strong_count() > 0);
        self.retired_vectors
            .retain(|(vector, _)| vector.strong_count() > 0);
        let raster = Arc::downgrade(&image.raster.data);
        if !self.retired.iter().any(|(old, _)| old.ptr_eq(&raster)) {
            self.retired.push((raster, image.raster.data.len() as u64));
        }
        if let Some(vector) = &image.vector {
            let weak = Arc::downgrade(vector);
            if !self
                .retired_vectors
                .iter()
                .any(|(old, _)| old.ptr_eq(&weak))
            {
                self.retired_vectors.push((weak, vector.est_bytes));
            }
        }
    }

    fn remove_slot(&mut self, key: &ResourceKey) {
        if let Some(slot) = self.map.remove(key) {
            self.track_retired(&slot.image);
        }
    }

    fn unreferenced_bytes(&self) -> u64 {
        self.map
            .values()
            .filter(|s| s.refs.is_empty() && s.pins == 0)
            .map(|s| s.image.decoded_bytes())
            .sum()
    }

    fn evict_lru_unreferenced(&mut self) -> bool {
        let victim = self
            .map
            .iter()
            .filter(|(_, s)| s.refs.is_empty() && s.pins == 0)
            .min_by_key(|(_, s)| s.last_use)
            .map(|(k, _)| k.clone());
        match victim {
            Some(k) => {
                self.remove_slot(&k);
                self.evictions += 1;
                true
            }
            None => false,
        }
    }

    /// LRU eviction of unreferenced entries beyond `cache_budget`.
    pub fn evict_unreferenced_beyond_budget(&mut self) {
        while self.unreferenced_bytes() > self.cache_budget {
            if !self.evict_lru_unreferenced() {
                break;
            }
        }
    }

    /// Reserve `bytes` for an upcoming decode against the hard decoded-memory budget, evicting
    /// unreferenced entries first. `false` = `BUDGET_EXCEEDED`.
    pub fn try_reserve(&mut self, bytes: u64) -> bool {
        self.retired.retain(|(raster, _)| raster.strong_count() > 0);
        self.retired_vectors
            .retain(|(vector, _)| vector.strong_count() > 0);
        while self.total_bytes() + self.reserved + bytes > self.memory_budget {
            if !self.evict_lru_unreferenced() {
                return false;
            }
        }
        self.reserved += bytes;
        true
    }

    pub fn release_reservation(&mut self, bytes: u64) {
        self.reserved = self.reserved.saturating_sub(bytes);
    }

    /// `resources_purge`: drop unreferenced entries of the scope. Referenced entries stay and are
    /// re-requested on next use (callers re-issue the request). Returns the number dropped.
    pub fn purge_unreferenced(&mut self, url: Option<&str>) -> usize {
        let doomed: Vec<ResourceKey> = self
            .map
            .iter()
            .filter(|(k, s)| s.refs.is_empty() && s.pins == 0 && url.is_none_or(|u| k.url == u))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &doomed {
            self.remove_slot(k);
        }
        doomed.len()
    }

    /// Remove entries of a URL regardless of references (used by a purge that must force a re-read
    /// of referenced entries: their next request misses and decodes again).
    pub fn invalidate(&mut self, scope: &PurgeScope, url: Option<&str>) -> usize {
        let doomed: Vec<ResourceKey> = self
            .map
            .iter()
            .filter(|(k, s)| {
                url.is_none_or(|u| k.url == u)
                    && match scope {
                        PurgeScope::Runtime => true,
                        PurgeScope::Document(d) => s.refs.contains_key(d),
                    }
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in &doomed {
            self.remove_slot(k);
            self.evictions += 1;
        }
        doomed.len()
    }

    /// Drop every entry whose URL starts with `prefix` (a replaced or removed `ui4me-res:` bundle),
    /// referenced or not; dependents re-request. Returns the number dropped.
    pub fn invalidate_url_prefix(&mut self, prefix: &str) -> usize {
        let doomed: Vec<ResourceKey> = self
            .map
            .keys()
            .filter(|k| k.url.starts_with(prefix))
            .cloned()
            .collect();
        for k in &doomed {
            self.remove_slot(k);
            self.evictions += 1;
        }
        doomed.len()
    }

    pub fn contains(&self, key: &ResourceKey) -> bool {
        self.map.contains_key(key)
    }

    pub fn ref_count(&self, key: &ResourceKey) -> u32 {
        self.map
            .get(key)
            .map(|s| s.refs.values().sum())
            .unwrap_or(0)
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.map.len(),
            bytes: self.total_bytes(),
            referenced_bytes: self.total_bytes() - self.unreferenced_bytes(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            reserved: self.reserved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::ids::{RawId, Slots};

    fn doc(i: u32) -> DocumentId {
        let mut s = Slots::new();
        let mut id = s.alloc();
        for _ in 0..i {
            id = s.alloc();
        }
        DocumentId::from_raw(RawId {
            index: id.index,
            generation: id.generation,
        })
    }

    fn entry(name: &str, bytes: usize) -> Arc<CachedImage> {
        Arc::new(CachedImage {
            key: ResourceKey {
                url: name.into(),
                content: ContentKey::Bundle(1),
            },
            raster: Arc::new(ImageRaster {
                width: 1,
                height: (bytes / 4) as u32,
                stride: 4,
                data: Arc::from(vec![0u8; bytes]),
            }),
            natural_px: (1, (bytes / 4) as u32),
            downscaled: false,
            animated_first_frame: false,
            icc_ignored: false,
            encoded_bytes: 10,
            decode_ms: 0,
            generation: 1,
            vector: None,
            reports: Vec::new(),
            warnings: Vec::new(),
        })
    }

    #[test]
    fn lru_evicts_only_unreferenced_beyond_budget() {
        let mut c = Cache::new(200, 10_000);
        let (a, b, d) = (entry("a", 100), entry("b", 100), entry("d", 100));
        let (ka, kb, kd) = (a.key.clone(), b.key.clone(), d.key.clone());
        c.insert(a);
        c.add_ref(doc(0), &ka);
        c.insert(b);
        assert!(c.lookup(&kb).is_some()); // b is most recently used
        c.insert(d); // unreferenced: b (100) + d (100) = 200 <= 200, a is referenced
        assert!(c.contains(&ka) && c.contains(&kb) && c.contains(&kd));
        let e = entry("e", 100);
        let ke = e.key.clone();
        c.insert(e); // unreferenced would be 300 > 200: evict the LRU unreferenced one (d: never looked up since insert)
        assert!(c.contains(&ka), "referenced entries are never evicted");
        assert_eq!(c.stats().entries, 3);
        assert!(!c.contains(&kd) || !c.contains(&kb));
        assert!(c.contains(&ke));
        assert_eq!(c.stats().evictions, 1);
    }

    #[test]
    fn release_makes_entries_evictable() {
        let mut c = Cache::new(0, 10_000);
        let a = entry("a", 100);
        let ka = a.key.clone();
        c.insert(a.clone());
        // cache_budget 0 evicts an unreferenced entry immediately
        assert!(!c.contains(&ka));
        let mut c = Cache::new(0, 10_000);
        c.insert_referenced_for_test(a, doc(0));
        assert!(c.contains(&ka));
        c.release_ref(doc(0), &ka);
        assert!(!c.contains(&ka));
    }

    impl Cache {
        fn insert_referenced_for_test(&mut self, image: Arc<CachedImage>, d: DocumentId) {
            let k = image.key.clone();
            // referenced insert: add the ref before eviction can run
            let t = self.tick();
            self.map.insert(
                k.clone(),
                Slot {
                    image,
                    refs: HashMap::new(),
                    pins: 0,
                    last_use: t,
                },
            );
            self.add_ref(d, &k);
        }
    }

    #[test]
    fn memory_budget_reservation() {
        let mut c = Cache::new(1000, 300);
        assert!(c.try_reserve(200));
        assert!(!c.try_reserve(200));
        c.release_reservation(200);
        assert!(c.try_reserve(300));
        assert!(!c.try_reserve(1));
    }

    #[test]
    fn an_evicted_raster_is_charged_until_the_last_frame_releases_it() {
        let mut cache = Cache::new(200, 300);
        let image = entry("frame", 200);
        let pixels = image.raster.data.clone();
        cache.insert(image);
        assert_eq!(cache.purge_unreferenced(None), 1);
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().bytes, 200);
        assert!(
            !cache.try_reserve(101),
            "eviction must not hide live scene pixels"
        );
        drop(pixels);
        assert_eq!(cache.stats().bytes, 0);
        assert!(cache.try_reserve(300));
    }
}
