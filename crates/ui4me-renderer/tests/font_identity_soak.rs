//! T4 soak (F-04 / RENDER-01, gate R4a): the issue #2 operation stream (a ticking counter in the base
//! face plus a region of `font-weight: 600` badges inserted and removed again) must not grow the live
//! heap. Before the fix each reload of the bold file after Parley's source-cache prune copied the whole
//! file into a table that was never pruned (~0.5 MiB per cycle).
//!
//! A counting global allocator measures the live heap of this test binary (one test only, so no other
//! test allocates concurrently). The stream runs 2,000 operations of warm-up, then 20,000 more; the live
//! heap at the end of the run, sampled at the same phase of the cycle (region just removed), must be
//! less than 1 MiB above the first sample after warm-up.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use ui4me_renderer::{Document, DomOp};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards to the system allocator; only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const DOC: &str = include_str!("data/issue2/doc.html");
const REGION: &str = include_str!("data/issue2/region.html");
/// The issue's variant: the badges in a weight no other text uses.
const CSS: &str = ".u4-badge { font-weight: 600; }";
/// Ticks between a region insertion and its removal, and between a removal and the next insertion
/// (the issue's stream: 50 ticks per second, one toggle per second).
const PERIOD: usize = 50;
const WARMUP_OPS: usize = 2_000;
const MEASURED_OPS: usize = 20_000;
const LIMIT_BYTES: isize = 1024 * 1024;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

fn region(generation: usize) -> String {
    let mut html = REGION.to_string();
    for k in (9..=20).rev() {
        html = html.replace(
            &format!("id=\"u4-{k}\""),
            &format!("id=\"g{generation}-u4-{k}\""),
        );
    }
    html
}

#[test]
fn issue2_op_stream_keeps_the_live_heap_flat() {
    let mut d = Document::parse_hosted(DOC, None, 500, 400).expect("parse");
    d.add_stylesheet(CSS).expect("stylesheet");
    let (mut ops, mut generation, mut current) = (0usize, 0usize, None::<String>);
    let mut samples: Vec<(usize, isize)> = Vec::new();
    let mut tick = 0usize;
    while ops < WARMUP_OPS + MEASURED_OPS {
        tick += 1;
        let mut batch = vec![DomOp::SetText {
            element: "u4-6".into(),
            text: tick.to_string(),
        }];
        if current.is_some() {
            batch.push(DomOp::SetText {
                element: format!("g{generation}-u4-11"),
                text: format!("region tick {tick}"),
            });
        }
        ops += batch.len();
        d.apply_batch(&batch).expect("tick");
        let mut removed = false;
        if tick.is_multiple_of(PERIOD) {
            if let Some(c) = current.take() {
                d.apply_batch(&[DomOp::RemoveElement { element: c }])
                    .expect("remove");
                removed = true;
            } else {
                generation += 1;
                d.apply_batch(&[DomOp::InsertHtml {
                    element: "u4-8".into(),
                    position: "beforeend".into(),
                    html: region(generation),
                }])
                .expect("insert");
                current = Some(format!("g{generation}-u4-9"));
            }
            ops += 1;
        }
        // Lay out (the windowed host does this every frame; the growth is in shaping during layout).
        d.element_rect("u4-6").expect("layout");
        let _ = d.drain_events();
        if removed && ops >= WARMUP_OPS {
            samples.push((ops, live()));
        }
    }
    let (first_ops, first) = samples[0];
    let (last_ops, last) = *samples.last().expect("samples");
    let peak = samples.iter().map(|s| s.1).max().unwrap_or(first);
    eprintln!(
        "live heap: {first} B at op {first_ops}, {last} B at op {last_ops} (growth {} B, peak +{} B, {} cycles)",
        last - first,
        peak - first,
        samples.len()
    );
    assert!(
        last - first < LIMIT_BYTES,
        "live heap grew {} KiB over {} operations (limit 1 MiB): {:?}",
        (last - first) / 1024,
        last_ops - first_ops,
        samples
            .iter()
            .map(|s| (s.0, s.1 / 1024))
            .collect::<Vec<_>>()
    );
}
