//! owner: 007-resources (T052) — device-scale rasterisation of SVG images (FR-040, FR-041, R-07).
//!
//! `paint_image(ref, device_size_px)` rasterises synchronously on the owner thread at the used
//! size times the scale, the first time a size is asked for; the result is cached per
//! (entry generation, size). A size or scale change re-rasterises; the previous raster stays
//! available as the fallback only when the new one cannot be made, and a raster made for one size
//! is never returned for another. Each side is clamped to 65,535 px with a `SIZE_LIMIT` warning,
//! and a raster that would exceed the decoded-memory budget is refused (`BUDGET_EXCEEDED`).

use crate::cache::CachedImage;
use crate::policy::MAX_RASTER_SIDE;
use crate::view::ImageRaster;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use ui4me_engine_types::codes_generated::Code;

/// Sizes kept per entry; older ones are dropped first.
const MAX_SIZES_PER_ENTRY: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterStats {
    pub rasterisations: u64,
    pub hits: u64,
    /// Requests whose size was clamped to 65,535 px per side (a `SIZE_LIMIT` warning each).
    pub clamped: u64,
    pub bytes: u64,
}

/// What a paint request got.
#[derive(Clone, Debug)]
pub struct SvgPaint {
    pub raster: Arc<ImageRaster>,
    /// Changes whenever the pixels handed out for this entry change.
    pub generation: u64,
    /// The size asked for was clamped (`SIZE_LIMIT` warning).
    pub clamped: bool,
    /// The new raster could not be made; this is the previous one.
    pub stale: bool,
}

/// (entry generation, device size)
type SizeKey = (u64, (u32, u32));
/// A raster and the generation it was handed out under.
type Made = (Arc<ImageRaster>, u64);

#[derive(Default)]
struct Inner {
    map: HashMap<SizeKey, Made>,
    order: VecDeque<SizeKey>,
    last: HashMap<u64, (Weak<ImageRaster>, u64)>,
    retired: Vec<(Weak<[u8]>, u64)>,
    next_generation: u64,
    stats: RasterStats,
}

impl Inner {
    fn live_bytes(&self) -> u64 {
        self.stats.bytes
            + self
                .retired
                .iter()
                .filter(|(pixels, _)| pixels.strong_count() > 0)
                .map(|(_, bytes)| *bytes)
                .sum::<u64>()
    }

    fn remove(&mut self, key: &SizeKey) {
        if let Some((raster, _)) = self.map.remove(key) {
            let bytes = raster.data.len() as u64;
            self.stats.bytes = self.stats.bytes.saturating_sub(bytes);
            self.retired.push((Arc::downgrade(&raster.data), bytes));
        }
    }

    fn prune(&mut self) {
        self.last.retain(|_, (raster, _)| raster.strong_count() > 0);
        self.retired.retain(|(pixels, _)| pixels.strong_count() > 0);
    }
}

pub struct SvgRasterCache {
    budget: u64,
    inner: Mutex<Inner>,
}

impl SvgRasterCache {
    /// `budget` bounds one raster and the sum of all cached rasters (the decoded-memory budget).
    pub fn new(budget: u64) -> Self {
        SvgRasterCache {
            budget,
            inner: Mutex::new(Inner {
                next_generation: 1,
                ..Default::default()
            }),
        }
    }

    pub fn stats(&self) -> RasterStats {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.prune();
        RasterStats {
            bytes: inner.live_bytes(),
            ..inner.stats
        }
    }

    /// The raster of `image` (a vector entry) at `device` pixels.
    pub fn raster(&self, image: &CachedImage, device: (u32, u32)) -> Result<SvgPaint, Code> {
        let Some(vector) = image.vector.as_ref() else {
            return Err(Code::InvalidState);
        };
        let (mut w, mut h) = device;
        if w == 0 || h == 0 {
            return Err(Code::InvalidArgument);
        }
        let clamped = w > MAX_RASTER_SIDE || h > MAX_RASTER_SIDE;
        w = w.min(MAX_RASTER_SIDE);
        h = h.min(MAX_RASTER_SIDE);
        let key = (image.generation, (w, h));
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.prune();
        if clamped {
            g.stats.clamped += 1;
        }
        if let Some((r, gen_)) = g.map.get(&key).cloned() {
            g.stats.hits += 1;
            g.last.insert(image.generation, (Arc::downgrade(&r), gen_));
            return Ok(SvgPaint {
                raster: r,
                generation: gen_,
                clamped,
                stale: false,
            });
        }
        let bytes = w as u64 * h as u64 * 4;
        let mut admitted = bytes <= self.budget;
        while admitted && g.live_bytes().saturating_add(bytes) > self.budget {
            if let Some(old) = g.order.pop_front() {
                g.remove(&old);
            } else {
                // Scene pixel Arcs can outlive the lookup entry. Eviction
                // cannot release their charge or justify a fresh allocation.
                admitted = false;
            }
        }
        let made = if !admitted {
            Err(Code::BudgetExceeded)
        } else {
            vector
                .renderer
                .rasterize(&vector.tree, w, h)
                .map_err(|s| s.code)
        };
        match made {
            Ok(img) => {
                let raster = Arc::new(ImageRaster {
                    width: img.width,
                    height: img.height,
                    stride: img.width * 4,
                    data: Arc::from(img.data),
                });
                let gen_ = g.next_generation;
                g.next_generation += 1;
                g.stats.rasterisations += 1;
                g.stats.bytes += bytes;
                g.map.insert(key, (raster.clone(), gen_));
                g.order.push_back(key);
                g.last
                    .insert(image.generation, (Arc::downgrade(&raster), gen_));
                // The byte budget was checked before allocation; retain only
                // a small number of sizes for each vector entry as well.
                while g.order.iter().filter(|k| k.0 == image.generation).count()
                    > MAX_SIZES_PER_ENTRY
                {
                    let Some(old) = g.order.pop_front() else {
                        break;
                    };
                    if old == key {
                        g.order.push_back(old);
                        break;
                    }
                    g.remove(&old);
                }
                Ok(SvgPaint {
                    raster,
                    generation: gen_,
                    clamped,
                    stale: false,
                })
            }
            Err(code) => match g
                .last
                .get(&image.generation)
                .and_then(|(raster, generation)| {
                    raster.upgrade().map(|raster| (raster, *generation))
                }) {
                Some((r, gen_)) => Ok(SvgPaint {
                    raster: r,
                    generation: gen_,
                    clamped,
                    stale: true,
                }),
                None => Err(code),
            },
        }
    }

    /// Forget every raster of an entry that was replaced or evicted.
    pub fn forget(&self, generation: u64) {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let keys: Vec<_> = g
            .map
            .keys()
            .filter(|k| k.0 == generation)
            .copied()
            .collect();
        for k in keys {
            g.remove(&k);
        }
        g.order.retain(|k| k.0 != generation);
        g.last.remove(&generation);
        g.prune();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{ContentKey, ResourceKey};
    use crate::decode::{SvgOptions, SvgRenderer};

    #[test]
    fn scene_pixels_remain_charged_after_svg_cache_retirement() {
        let renderer: Arc<dyn SvgRenderer> = Arc::new(crate::svg::usvg_adapter::UsvgRenderer);
        let parsed = renderer.parse(b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'><rect width='16' height='16' fill='red'/></svg>", &SvgOptions::default()).unwrap();
        let image = CachedImage {
            key: ResourceKey {
                url: "test.svg".into(),
                content: ContentKey::Bundle(1),
            },
            raster: Arc::new(ImageRaster {
                width: 1,
                height: 1,
                stride: 4,
                data: Arc::from([0; 4]),
            }),
            natural_px: (16, 16),
            downscaled: false,
            animated_first_frame: false,
            icc_ignored: false,
            encoded_bytes: 100,
            decode_ms: 0,
            generation: 1,
            vector: Some(Arc::new(crate::svg::VectorImage {
                tree: parsed.tree,
                natural: parsed.natural,
                renderer,
                est_bytes: 100,
            })),
            reports: Vec::new(),
            warnings: Vec::new(),
        };
        let cache = SvgRasterCache::new(1024);
        let pixels = cache.raster(&image, (16, 16)).unwrap().raster.data.clone();
        cache.forget(image.generation);
        assert_eq!(cache.stats().bytes, 1024);
        assert!(matches!(
            cache.raster(&image, (16, 16)),
            Err(Code::BudgetExceeded)
        ));
        drop(pixels);
        assert_eq!(cache.stats().bytes, 0);
        assert!(cache.raster(&image, (16, 16)).is_ok());
    }
}
