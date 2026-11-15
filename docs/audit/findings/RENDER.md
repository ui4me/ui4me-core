# RENDER audit — crates/ui4me-renderer (excluding a11y/, interaction/)

> **Coordinator note (post-review verification).** RENDER-02 is now a *confirmed defect* on the published Windows binary: 30 nested `<div>` render, 40 abort `ui4me-host.exe` with `thread 'main' has overflowed its stack` (experiment E7 in [../01-scope-and-method.md](../01-scope-and-method.md)). Corrections from the adversarial review: RENDER-02's alternative says "winit and AccessKit are main-thread bound"; the vendored winit supports `with_any_thread` on Win32 and Wayland, and running the event loop on an explicitly sized thread is now the *recommended* fix (roadmap 1.1). RENDER-02's "16 MiB gives ~2× headroom" holds for `<div>` only, not `<fieldset>`. RENDER-10's "no deep-nesting generators" is wrong: `fuzz_gate.rs:172-178` already inserts nesting bombs up to 600 levels; what is missing is image-SVG and move-chain generators and a small worker stack. RENDER-05 is re-rated High for the long-log use case (F-16a). RENDER-01 is consolidated as F-04, RENDER-02 as F-01 (re-rated Critical) in [../02-findings-register.md](../02-findings-register.md). Paths written `<scratch>/` were session-temporary; key scripts are retained in [../evidence/scripts/render-harness](../evidence/scripts/render-harness/).


Audited commit c6fd71d (1.0.1). Main tree left clean (`git status` empty, no worktrees).

## 1. Scope and method

Read (targeted): `lib.rs`, `document.rs` (construction, `render_into`, `validate_document`, DomOp application), `pipeline/{state,update,read,frame,dirty}.rs`, `text/fonts/environment.rs`, `text/paragraph/{shaped,source}.rs`, `text/metrics.rs`, `layout/coordinator/{replay,host_tree,atomic}.rs`, `layout/box_tree/{incremental,build}.rs` (recursion entry points), `raster/lower/vello_cpu.rs` (contexts, glyph runs), `style/adapter/{handle,element}.rs`, `fuzz_gate.rs`, `regression/long_document.rs`, `ui4me-document/src/journal.rs`, host `host/{native,document_native,documents}.rs`, `docs/concepts/html-css-scope.md`, `patches/README.md` + patch headers, `tools/verify.py`, CI workflow. Third-party sources read: fontique 0.12 `source_cache.rs`, parley 0.12 `context.rs`, linebender_resource_handle `blob.rs`, vello_cpu 0.3 `text.rs`, glifo 0.4 caches.

Experiments (all under `<scratch>/exp/RENDER/`):

* **In-process harness** `harness/` (separate Cargo project depending on the repo's `crates/ui4me-renderer` by path, repo `Cargo.lock` copied, built with `CARGO_TARGET_DIR=<repo>/target cargo build -j 4 --offline`; renderer at opt-level 3 / codegen-units 1 for the stack tests, opt-level 2 for the rest; a counting global allocator reports live heap). Binary: `target/debug/render-audit-harness`. Modes: `leak`, `issue2`, `settext`, `deep`, `movedeep`, `profile` (see `harness/src/main.rs`).
* **Headless host** `headless/{run.py,backend.py}` against `target/debug/ui4me-host run --headless` (UI4ME_TEST_MODE=1), sampling host `VmRSS`.
* **Windowed host** `windowed/` — issue #2 `replay.py` (copied from the coordinator's `exp/main/leak/`) with the published 1.0.1 wheel venv, in the coordinator's private nested compositor `WAYLAND_DISPLAY=ui4me-audit-0`.
* heaptrack on the in-process issue #2 replay (`ht/issue2.zst`, `ht/peaks.txt`); gdb stack-pointer measurement of recursion frames; gdb poor-man's sampling of a cold layout (perf is disabled by `perf_event_paranoid`).
* `patches/stylo/verify.sh` and `patches/taffy/verify.sh` (both OK against the cached pristine checkouts).
* PE header of the published Windows `ui4me-host.exe` (`pe_stack.py`).

Key results (exact numbers in each finding):

| Experiment | Result |
| --- | --- |
| Issue #2 replay in-process (`issue2 7500`, `.u4-badge{font-weight:600}`) | live heap +50 MiB linear; same run without CSS flat |
| heaptrack, same scenario 2000 ticks | 13.5 MB peak from 22 calls `Arc<[u8]>::copy_from_slice` ← `FontEnvironment::blobs_for_layout` ← `TextServices::shape_with_face_defaults` |
| Discriminating CSS variants (in-process, NORENDER) | 600 grows; 700 (same face as the bold ticker) flat; 600 on both badge+ticker flat; monospace badge grows; monospace badge + monospace-bold ticker grows; monospace badge + monospace-normal ticker flat |
| Same variants windowed (published 1.0.1 host, nested KWin) at 31/63/94/126 s | 600: 58.5→68.4→77.6→88.2 MiB; 700: 48.1→48.4→48.5→49.4; 600 both: 48.8→49.0→49.0→49.0 |
| Headless host, 10 000 `set_text` | +3.8 MiB (0.38 KiB/op) without reads; with a `get_rect` every 50 ops: plateau |
| Depth test (`deep N 1024`, 1 MiB thread, renderer O3/cgu1) | depth 35 OK, 40 overflow (stack overflow abort); 23.6 KiB stack per nesting level |
| Windows exe PE header | `SizeOfStackReserve = 1 048 576` |
| `movedeep 3 200 8192` (cold render after moves, 8 MiB thread) | depth ~603 accepted by `move_element`; stack overflow abort |
| 2000-row doc, `set_text` on a status `<p>` + render (1280×800) | ~27–31 ms per update (style 1.6, layout 15, scene 5, raster 3–5 ms) |
| Same doc, 8th unrelated update then append one row | 1.0–1.8 s (all rows reshaped); with ≤7 prior updates: ~70 ms |
| Full raster, simple text page | 1280×800 ≈ 9 ms; 2560×1440 ≈ 11 ms; 3840×2160 ≈ 13–16 ms; set_text+render at 4K ≈ 17–19 ms |

## 2. Strengths to preserve

* **Bounded diagnostics everywhere I looked**: `Ring` stores (`ERROR_RING` 256, `INVALIDATION_RING` 16), `ShapedCache` (4096 entries LRU), `ContentCache` (8-pass aging + 4096 reserve), metrics memo and decoration-font caches (clear at limit), glifo glyph/outline/hint caches (age-evicted; `maintain` runs after each vello render). Issue #2 is *not* a cache-bound omission in these.
* **Raster memory guard**: filter-layer stack bounded (32 Mpx / 512 MiB estimate, `raster/lower/vello_cpu.rs` `Layers::push`), u16 surface overflow checked; `font-size` 0/huge and huge glyph coordinates skipped (`LOCAL_LIMIT`).
* **Oracle-based incremental design**: the cold pipeline is the oracle for incremental box build, layout replay, scene reuse and damage raster (`regression/incremental_vs_cold.rs`, `no_layout_reuse`, `no_damage_raster`, `UI4ME_FULL_RASTER`). This is the right way to keep an incremental engine honest; keep it.
* **Failure isolation**: layout runs under `catch_unwind` (`layout/coordinator/mod.rs:179/300/389`); a failed update discards the draft, records an error and retries cold (`pipeline/state.rs` state machine). Committed sets are immutable `Arc`s.
* **Damage raster** with exact-equality tests vs full raster; measured partial frames are cheap.
* **Vendored-dependency discipline**: base revision + numbered patches + `verify.sh` that reproduces the tree byte-for-byte (both pass). The Stylo delta is small (19 patches, ~250 changed lines, almost all "enable a Gecko-only property in servo mode behind a pref").
* **Style handle soundness design** (`style/adapter/handle.rs`): one-word handle with compile-time size/align asserts for Stylo's sharing-cache transmute, TLS-bound pass with nesting assert and unwind-reset guard, `!Send`. Sound as far as I can see.
* **Documented CSS scope matches the runtime profile** for every feature I cross-checked (`css_profile()` dump in `profile.tsv`: position-area/try, anchor-size, line-clamp, text-wrap-pretty, zoom, font-size-adjust, initial-letter, ruby, shape-outside, counter-style, custom-media, view-transition, container style queries all off/unavailable as documented; subgrid, backdrop-filter, -webkit-line-clamp on).

## 3. Findings

### RENDER-01: Issue #2 root cause — system-font blobs re-copied (≈0.5 MiB each) on every reload after parley's source-cache prune; `system_blobs` never pruned
* **Classification**: Confirmed defect (reproduced in-process, with heaptrack stack, and in the windowed published 1.0.1 host with discriminating variants).
* **Component**: `text/fonts/environment.rs` (`FontEnvironment::system_blobs`, `blobs_for_layout`), interaction with parley/fontique.
* **Evidence**:
  * `environment.rs:60` `system_blobs: HashMap<u64, FontBlob>` keyed by the *parley blob id*; `environment.rs:301-335` `blobs_for_layout`: `self.system_blobs.entry(published_blob_id(data.id())).or_insert_with(|| FontBlob { bytes: Arc::from(data.as_ref()), .. })` — a full heap copy of the font file per new id, entries never removed. Called for every shaped paragraph (`paragraph/source.rs:1277`, `paragraph/synthetic.rs:135`).
  * `environment.rs:267-270`: `FontContext { collection, source_cache: SourceCache::default() }` (non-shared cache).
  * parley 0.12 `src/context.rs:109/135/177`: every `ranged_builder`/`style_run_builder`/`tree_builder` calls `fcx.source_cache.prune(128, false)`; fontique `source_cache.rs:150` drops entries not accessed in the last 128 prunes. A reload goes through `load_blob` → `Blob::new` → fresh id from the process-global `ID_COUNTER` (`linebender_resource_handle blob.rs:90`).
  * So a face that is not shaped for ~128 builder calls (the bold badges while the region is absent ~1 s at 50 ticks/s) is evicted; the next insertion reloads it with a new id, `blobs_for_layout` copies the whole file again (NotoSans-* ≈ 530 KB here) and keeps it forever. The base face and the always-ticking `<strong>` face are touched every pass and never reload — exactly the issue's "non-base face" correlation.
  * heaptrack (`ht/peaks.txt`): 13.51 MB peak "over 22 calls" from `Arc<[u8]>::copy_from_slice` ← `blobs_for_layout` ← `shape_with_face_defaults` ← `shape` ← `InlineContainer::new` ← `HostTree::add_node` (2000 ticks, 20 insertions).
  * Fine-grained trace: +≈1.2 MiB at each insertion, −≈0.5 MiB at each removal, net ≈ +650 KiB per cycle; native issue rate 0.3 MiB/s = one cycle per 2 s. Exact match.
  * Variants: weight 700 (same face as the ticking `<strong>`) flat; 600 on both badge and ticker flat; PERIOD=10 (face absent < 128 prunes) flat — all predicted by the mechanism.
* **Reproduction**: `LEAK_CSSTEXT='.u4-badge { font-weight: 600; }' NORENDER=1 target/debug/render-audit-harness issue2 4000` (needs `exp/RENDER/{doc,region}.html`); windowed: `exp/RENDER/windowed/w600.log` vs `w700.log` vs `w600both.log`.
* **Severity**: High; Likelihood: High for long-running apps that re-create content in a face not continuously in use (headings, bold, code). Unbounded (issue: 65 → 609 MiB in 30 min).
* **Impact**: unbounded heap growth; also churn: every reload re-mmaps and re-parses the font, gives new published ids → vello/glifo outline and atlas cache misses and re-rasterized glyphs.
* **Root cause**: identity of system font data is taken from parley's per-load blob id, which is not stable because parley prunes its source cache on every builder; the side table that copies bytes is keyed by that unstable id and has no eviction.
* **Remediation** (implementation-ready):
  1. In `FontEnvironment::new` use `SourceCache::new_shared()` instead of `SourceCache::default()`, and in `blobs_for_layout` keep the fontique `Blob<u8>` itself alive (e.g. `system_sources: HashMap<u64, parley::fontique::Blob<u8>>` next to `system_blobs`). With a shared cache, a locally pruned entry is re-served from the shared weak entry (`Shared::get` upgrades the weak ref), so the id stays stable while the env holds the blob. Ideally make `FontBlob.bytes` able to wrap the blob without copying (`Arc<dyn AsRef<[u8]> + Send + Sync>` or a `Blob`), removing the 0.5 MiB copy entirely.
  2. Defence in depth: bound `system_blobs` — record a last-used layout pass per entry and drop entries unused for N passes (they are re-created on demand), or rebuild the map from the fonts referenced by the committed layout each pass.
  3. Tests: (a) unit test in `environment.rs`: shape a bold paragraph, force `env.fcx.source_cache.prune(0, false)` (or shape 200 other paragraphs), shape the bold paragraph again; assert the published blob id is unchanged and `system_blobs.len()` did not grow. (b) soak regression (counting allocator, like `regression/long_document.rs`): the issue #2 op stream for 40 insert/remove cycles with ≥150 shaping passes between them; assert live-heap growth after warm-up < 1 MiB. Requires a system font with a separate bold file (DejaVu/Noto) — skip gracefully when system fonts are unavailable.
* **Alternatives/tradeoffs**: keying `system_blobs` by content hash dedupes copies but keeps the id churn (glyph cache misses) and RSS from re-mmaps retained by cached layouts; pruning alone bounds memory but keeps churn. Option 1 fixes the cause.
* **Complexity**: S (1) / M (if `FontBlob` type changes cross the `ui4me-assets` boundary).
* **Confidence**: High.

### RENDER-02: Hosted documents nested ~40 levels overflow the 1 MiB Windows main-thread stack (host abort); documented limit is 256
* **Classification**: Reproducible risk (reproduced on Linux with a 1 MiB thread at release-like optimisation; Windows stack size read from the published exe; not run on Windows).
* **Component**: `layout/box_tree/build.rs` recursion `Builder::element` (709) → `element_dispatch` (911) → `block_boxed` (1072) → `children` (561); host runs the renderer on the main thread (`crates/ui4me-host/src/host/mod.rs:246-252` `EventLoop::new` / `run_app`); no stack-size setting anywhere (`crates/ui4me-host/build.rs`, no `.cargo/config.toml`, no linker args in workflows).
* **Evidence**: published `UI4ME.Core/1.0.1/runtimes/win-x64/ui4me_core/_bin/ui4me-host.exe` has `SizeOfStackReserve = 1 MiB` (`pe_stack.py`). `deep N 1024` (nested `<div>`s, renderer opt-level 3/cgu 1): N=35 renders, N=40/50/60 "has overflowed its stack … aborting". gdb: one nesting level = 0x5C30 = 23.6 KiB of stack (opt-2: 19.7 KiB), dominated by by-value `BoxStyle` locals (`fn element(&mut self, node, style: BoxStyle, …)`). At 8 MiB (Linux main thread) depth 252 renders. Docs (`html-css-scope.md` Limits) promise nesting depth 256; `document.rs:41` `MAX_DEPTH = 256`.
* **Reproduction**: `target/debug/render-audit-harness deep 40 1024` (crash), `deep 35 1024` (ok), `deep 252 8192` (ok).
* **Severity**: High; Likelihood: Medium (generated UIs — layout wrappers, `display:contents` regions, tables add 4 levels each — can reach 40; one such document kills the Windows host with STATUS_STACK_OVERFLOW, no error reported to the backend).
* **Impact**: native host crash on Windows for valid documents inside the documented limits; later recursive stages (Taffy, fragments, scene, a11y) were not individually measured and may add to it.
* **Root cause**: deep recursion with ~20 KiB frames per DOM level, plus the MSVC default 1 MiB main-thread reserve; fuzzing runs cases on an 8 MiB worker thread (`fuzz_gate.rs:268`) so it cannot see this.
* **Remediation**: (1) Immediate: in `crates/ui4me-host/build.rs`, for `target_env = "msvc"` emit `cargo:rustc-link-arg-bins=/STACK:16777216` (16 MiB reserve; commit stays small) — depth 252 needed < 8 MiB here, so 16 MiB gives ~2× headroom. (2) Shrink frames: pass `&BoxStyle`/`Arc<BoxStyle>` (the style maps already hold `Arc<BoxStyle>`), move large temporaries into `Box`, or convert `Builder::element` recursion to an explicit work stack. (3) Test: a renderer unit test that renders a 255-deep mixed document (block, flex, table, inline, display:contents) on a thread whose stack equals the configured reserve ÷ 2, run in the release-profile CI job on Windows and Linux; plus a fuzz worker with 1 MiB stack.
* **Alternatives**: run the engine on a dedicated large-stack thread (bigger refactor: winit and AccessKit are main-thread bound); lower the documented depth (honest but weak).
* **Complexity**: S for (1), M for (2), S for (3). **Confidence**: High for the mechanism and the Linux measurement; Medium for the exact Windows threshold (MSVC codegen and thin LTO may shift frame sizes either way).

### RENDER-03: The 256 depth limit is bypassable (`move_element`, `insert_html` fragment depth), giving unbounded depth → stack overflow on any platform
* **Classification**: Confirmed defect (move path reproduced; insert_html depth accounting proven from code).
* **Component**: `document.rs` `validate_document` (2473-2539) and `apply_op` arms (2814-2870).
* **Evidence**: `DomOp::MoveElement` performs no depth/size validation. `DomOp::InsertHtml` calls `validate_document(d, None, Some(f), hosted)`: with `replacing == None` the `replacement_depth` stays 0, so the fragment is validated as if at the root (a 250-deep fragment inserted at depth 250 passes). `ReplaceChildrenHtml` (passes `Some(n)`) is correct. Once the tree is deeper than 256, every later `insert_html`/`replace_children_html` anywhere fails with SIZE_LIMIT (observed: "insert_html after deep move: SIZE_LIMIT"), i.e. the document is no longer editable by markup.
* **Reproduction**: `NOFIRST=1 target/debug/render-audit-harness movedeep 3 200 8192` → depth ~603 accepted, cold render on an 8 MiB thread aborts with stack overflow. With an initial render before the moves the incremental box build (iterative take-over) survived, so the crash needs a full box build: first render, `font_context_pending`, a failed update's cold retry, or a journal window overflow (`box_damage` returns `None` when > 4096 entries since the last build).
* **Severity**: Medium; Likelihood: Low (needs backend calls; the backend is trusted application code, but a buggy app can kill the host).
* **Impact**: host abort; also a latent, data-dependent crash after an unrelated cold rebuild.
* **Remediation**: track depth incrementally — check `depth(target) + subtree_height(moved)` in `MoveElement` and `depth(insertion parent) + fragment height` in `InsertHtml` (compute the insertion parent's depth by walking ancestors, O(depth)); add tests `move_element_rejects_depth_over_limit`, `insert_html_counts_insertion_depth`. Independently fix RENDER-02 so the limit is survivable.
* **Complexity**: S. **Confidence**: High.

### RENDER-04: Headless/flushless growth (~0.38 KiB per DOM op) — the journal epoch segment and dirty records accumulate until an update runs; dirty coalescing is O(n²)
* **Classification**: Confirmed defect (reproduced; mechanism from code).
* **Component**: `ui4me-document/src/journal.rs:181-232` (`Journal::segment`, appended per entry, cleared only by `take_epoch` at 292), consumed only in `pipeline/update.rs:290` (`run_update`); `pipeline/dirty.rs:31-43` (`DirtySets::note` linear search over all records), fed per entry by `note_journal_since` (`update.rs:168-226`, ChildList builds a `Vec` subject per entry).
* **Evidence**: headless host (no frames, no reads): 10 000 `set_text` → RSS 51.9 → 55.7 MiB, linear (0.38 KiB/op). Same with a `get_rect` every 50 ops: 52.3 → 53.3 MiB plateau after 4000 ops (`headless/out-set_text-10000-{0,50}.jsonl`). Matches the issue's "0.3–0.5 KiB per DOM op headless".
* **Reproduction**: `cd exp/RENDER/headless && python3 run.py set_text 10000 0` vs `... 10000 50`.
* **Severity**: Low (headless is CI/test mode) — Medium if any native path stops updating while the backend keeps mutating (a minimized/occluded Wayland window gets no frame callbacks; the host's accessibility refresh currently flushes per pump batch (`document_native.rs:381-384` → `accessibility_tree_titled` → `pipeline.flush`), which masks it — plausible concern needing verification). Likelihood: Certain in headless soak tests.
* **Impact**: unbounded memory in headless soaks; CPU grows quadratically with unflushed mutations (each `note` scans all records).
* **Remediation**: (1) Bound the segment: when `segment.len()` exceeds `WINDOW`, drop it and set an `overflowed` flag on the epoch; `StyleBridge::restyle` then restyles the document conservatively (the cold path already exists) and `box_damage` builds in full. (2) Make `DirtySets` coalesce with a `HashMap<(Reason, Subject), usize>` index and cap per-reason subjects (widen to `Subject::Document` past a threshold). (3) Optionally run a cheap update on an idle timer in headless. Tests: in `ui4me-document`, 100 000 mutations without `take_epoch` keep `segment.len() <= WINDOW`; in the pipeline, a soak with no flush keeps the dirty record count bounded and the first flush after it equals the cold oracle.
* **Complexity**: M. **Confidence**: High.

### RENDER-05: Performance cliff — after 8 unrelated updates, a structural change to a large list re-shapes every row (~1 s per 2000 rows)
* **Classification**: Reproducible risk (measured); exact root cause needs verification.
* **Component**: `layout/coordinator/replay.rs` (`ifc_seeds`, `seed` requiring `InlineContainer::same_input`), `text/paragraph/shaped.rs:187` `RETAIN_PASSES = 8` (`ContentCache::next_pass`).
* **Evidence**: `settext 2000 N` then append one row to the list and render: N ≤ 7 → ~70 ms ("replayed 2001"); N ≥ 8 → 1.0–1.8 s ("replayed 1", `ifc_new` 818–1590 ms = every row reshaped). The threshold equals `RETAIN_PASSES`: rows replayed (not shaped) during unrelated passes are never looked up in the content cache, so their entries age out; when the list's parent is rebuilt the IFC seeds do not apply and every row misses the cache.
* **Reproduction**: `INSERTS=1 UI4ME_PROFILE=1 target/debug/render-audit-harness settext 2000 8 1280 800 1.0` vs `... settext 2000 7 ...`.
* **Severity**: Medium; Likelihood: High for chat/log apps with any other ticking text (exactly the documented long-log use case, `regression/long_document.rs`).
* **Impact**: ~1 s main-thread stall per appended log line once the list is large; frames and input stall.
* **Remediation**: mark the content-cache entries of replayed/seeded inline containers as used in each pass (`ContentCache::touch(hash)` from `seed`/`adopt_results`), or keep the shaped paragraph inside the host snapshot and reuse it by content hash without the cache; then investigate why `seed` fails for unchanged rows when the parent's child list changed (likely `ifc_seeds` only covering `same_ids` ranges). Test: long-document regression — 20 updates of an unrelated element, append one row, assert the update shaped ≤ 2 paragraphs (expose a shaping counter in `TextServices`).
* **Complexity**: M. **Confidence**: Medium (cliff measured; mechanism inferred).

### RENDER-06: Cold shaping dominated by `resolve_families` expanding generics to the full fontconfig fallback list (165 families here), several times per paragraph
* **Classification**: Confirmed performance problem (sampled), remediation impact estimated.
* **Component**: `text/fonts/environment.rs:412-458` `resolve_families`; callers `paragraph/source.rs:894, 1009, 1186`, `text/metrics.rs:82`.
* **Evidence**: 4 of 6 stack samples during the cold layout of a 2000-row document were inside `resolve_families` (BTreeSet<String> inserts, String allocations). In system mode the generic `sans-serif` expands through `system_generics` (fontconfig fallback order; `fc-match -s sans-serif | wc -l` = 165), allocating a lowercase key and a clone per family per call, and parley then receives a 165-entry `FontFamily::List`. First render of 2000 short rows: 1.1–1.4 s (`ifc_new` ≈ 1040 ms, ≈0.5 ms per paragraph).
* **Reproduction**: `target/debug/render-audit-harness settext 2000 1 1280 800 1.0` (first_render), sampling loop in the log of this audit.
* **Severity**: Medium; Likelihood: High (every document load, font change, and the RENDER-05 cliff).
* **Remediation**: memoize `resolve_families` per `(families, face-table generation)` → `Arc<[String]>` in `FontEnvironment`; truncate system generic expansions to the first few families (fontique already does per-script fallback) or pass the generic to parley. Bench test: cold layout of 2000 paragraphs under a time budget in the release long-document harness.
* **Complexity**: S–M. **Confidence**: High that it dominates; Medium on the achievable speed-up (estimate 3–5× for cold shaping).

### RENDER-07: Every incremental update is O(document) — ~27–31 ms per `set_text` in a 2000-row document
* **Classification**: Design tradeoff with measurable cost.
* **Component**: `pipeline/update.rs` `compute_draft` (comment "S2–S4 (conservative full rebuild)"), `layout/coordinator/host_tree.rs` (`layout_boxes` rebuilds the whole `HostTree` and fragments), scene build.
* **Evidence** (`UI4ME_PROFILE=1`, opt-2): restyle 1.3 ms, style bridge 2.2 ms (lang/dir over all nodes), box build 2.5 ms, `tree.add` 4 ms (2004 nodes), `build_fragments` 3.6 ms, scene prepare+build 4.6–7.5 ms (6004 items, 1 subtree reused), raster 3–5 ms; mutation itself 0.9 ms. In native mode the host additionally rebuilds the full accessibility tree per refresh (`document_native.rs:381-384`, a11y specialist's area).
* **Severity**: Medium; Likelihood: High for large documents with frequent updates (a 50 Hz ticker in a 2000-row document saturates the main thread).
* **Remediation**: profile and make the per-pass O(N) walks subtree-skippable (dirty-ancestor sets already exist: `dom_dirty` in `update.rs:1118`): skip lang/dir recomputation for clean subtrees, keep `HostTree` nodes of untouched subtrees instead of re-adding, patch fragments for replayed roots, reuse scene subtrees for the remaining 2000 unchanged rows (today only 1 subtree is reused). Add a long-document benchmark gate (time per small update vs row count must be sublinear).
* **Complexity**: L. **Confidence**: High (measured).

### RENDER-08: Every frame converts and presents the full surface; damage tracking stops at the vello raster
* **Classification**: Optional improvement / performance.
* **Component**: `document.rs:1514-1565` `render_into` (per-pixel premultiplied→0RGB conversion of the whole surface, then `paint_selection`/`paint_editing_caret`), host `host/native.rs:471-494` (`buffer.present()` without damage).
* **Evidence**: at 3840×2160 a trivial `set_text` frame costs ~17–19 ms although the damaged region is tiny; full raster of the same page is 13–16 ms. vello_cpu runs single-threaded at `Level::baseline()` (SSE2) by design (`raster/caps.rs:52-73`, determinism).
* **Severity**: Low–Medium (HiDPI/4K desktops). **Remediation**: use the raster receipt's damage rects: convert only damaged rows/rects into the softbuffer buffer (respect `buffer.age()`), `present_with_damage`; optionally allow runtime SIMD detection outside qualification/golden runs. **Complexity**: M. **Confidence**: Medium (conversion/present share not separately timed).

### RENDER-09: `insert_html`/`replace_children_html` validate the entire document on every call
* **Classification**: Performance concern (measured, modest).
* **Component**: `document.rs:2473-2539` (`validate_document` walks from the document node with a `BTreeSet` of all ids) called from `apply_op` (2826, 2833).
* **Evidence**: `insert_html` (no render) median 1.3 ms on a ~6 000-node document; scales O(N log N) toward the 65 536-node limit (~15 ms estimated) per insert, multiplied in 1024-op batches.
* **Remediation**: validate only the fragment (script/attribute/id rules) and check id uniqueness against the DOM's id index (`element_by_id`) and depth via the insertion parent (shares the fix with RENDER-03). **Complexity**: S. **Confidence**: High.

### RENDER-10: Fuzz gate is not run in CI and cannot see stack exhaustion
* **Classification**: Test/maintenance gap.
* **Component**: `fuzz_gate.rs` (3 `#[ignore]` harnesses: HTML+pipeline, CSS, DOM sequences; xorshift mutation fuzzer, default 2000 iterations, per-case watchdog), `tools/verify.py` (no `--ignored` run), `.github/workflows/ci.yml` (only `verify.py`).
* **Evidence**: harness workers use an 8 MiB stack (`fuzz_gate.rs:268`); dictionary has no deep-nesting or move-chain generators; markup is capped at 120 kB; no record in CI of a fuzz budget.
* **Remediation**: nightly CI job running each harness for a fixed seed and 120 s in release, worker stack 1 MiB; add generators for deep nesting (≥ 256 via move/insert chains), huge tables (`<td>` × 10⁴, `colspan=1000`), 10⁵ siblings, extreme numeric CSS combined with `calc()` NaN paths. **Complexity**: S. **Confidence**: High.

### RENDER-11: Production code hidden behind crate-wide `#[allow(dead_code)]`; paged-media pipeline compiled but unreachable
* **Classification**: Maintenance problem / simplification opportunity.
* **Component**: `lib.rs:5-20` (every internal module `#[allow(dead_code)]`); `pipeline/paged.rs` (912 lines), `layout/fragmentation/paged.rs` (247), `layout_pages` in `host_tree.rs` (~300), reachable only from `#[cfg(test)] document::paged_facade` (`document.rs:2973-2974`); the CSS profile marks `@page` off (`lib.rs:30`).
* **Remediation**: remove the blanket allows and let the compiler list dead code; gate paged output behind a cargo feature or delete it (with Stylo patch P-006-9 if `@page` parsing is not needed). **Complexity**: S–M. **Confidence**: High.

### RENDER-12: Taffy patch delta is large and expensive to rebase
* **Classification**: Design tradeoff / expensive-to-reverse decision.
* **Evidence**: Taffy base `fca167c` + 6 patches, ~1 800 added lines (P-008-4 subgrid alone 1 431 added, 108 removed, new `compute/grid/subgrid.rs`; P-008-5 fragmentation hooks 220). Stylo: 19 small patches (~250 changed lines). Both reproduce exactly (`verify.sh` OK). Neither is run in CI (`verify.py` has no patch stage) and the README states no upstream submission.
* **Remediation**: add `patches/*/verify.sh` to CI (they need the pristine checkout: vendor it as a cached artifact or fetch by revision); upstream the Taffy hooks or isolate subgrid behind Taffy's `LayoutPartialTree` extension points to shrink the rebase surface. **Complexity**: M (CI) / L (upstreaming). **Confidence**: High.

### RENDER-13: Raw `text_ptr` aliasing in the host tree (sound today, fragile)
* **Classification**: Maintenance risk (unsafe review).
* **Component**: `layout/coordinator/host_tree.rs:203, 3102-3104, 3491-3493`; deref at `atomic.rs:465` (`unsafe { self.tree.text_ptr.as_mut() }`).
* **Evidence**: a `*mut TextServices` is stored in `HostTree` for the duration of `compute_root_layout` and handed out as `&mut` on demand (`ifc/container.rs:926`). Sound only while no other `&mut TextServices` is live at the same time and no caller holds the returned reference across a nested `layout_atomic` call; not reset on unwind (harmless because the tree is dropped). Other unsafe in scope (`style/adapter/handle.rs:82/89`, `style/adapter/element.rs:463-482`, `style/traverse.rs:85/91`) follows Stylo's single-threaded `TElement` contract with `pool = None`; I found no unsoundness.
* **Remediation**: thread `&mut TextServices` through the Taffy tree as a field borrowed for the pass (`HostTree<'t> { text: &'t mut TextServices }`) instead of a raw pointer; add a debug-assert re-entrancy flag meanwhile. **Complexity**: M. **Confidence**: Medium.

## 4. Test gaps and highest-value tests

1. **Font identity soak** (RENDER-01): unit test of stable blob id across `source_cache.prune(0)`; allocation-counting soak of the issue #2 op stream; assert `system_blobs.len()` bounded.
2. **Stack budget test** (RENDER-02/03): release-profile test rendering a 255-deep mixed document on a thread with the production stack ÷ 2, on Windows and Linux CI; negative tests for `move_element` / `insert_html` depth.
3. **Flushless mutation soak** (RENDER-04): 100 k mutations without a read keep journal segment and dirty records bounded; first flush equals the cold oracle.
4. **Long-document performance gates** (RENDER-05/06/07): in `regression/long_document.rs` (already present but `#[ignore]` for timing), add counters (paragraphs shaped, host nodes rebuilt, scene subtrees built) and assert them instead of wall time, e.g. "append 1 row after 20 unrelated updates shapes ≤ 2 paragraphs".
5. **Fuzz in CI** (RENDER-10) with 1 MiB worker stacks and deep/huge generators.
6. **Browser oracle breadth**: `tools/browser_reference.py` has only 4 fixtures and is not a default `verify.py` stage, while the documented scope lists ~100 "on" features. Add fixtures for the features most likely to regress (tables with colspan/rowspan, sticky, multi-column, writing modes, subgrid, text-overflow) and run the stage at least before releases.

## 5. Recommend NOT changing

* The cold-oracle architecture and the `no_layout_reuse` / `no_damage_raster` / `UI4ME_FULL_RASTER` switches — they are what makes the incremental paths verifiable.
* Bounded `Ring` diagnostics and the existing cache bounds (do not "fix" issue #2 by shrinking them; they are not the cause).
* The deterministic single-threaded `Level::baseline()` raster for qualification/goldens (a runtime-SIMD path, if added, should be an opt-in outside qualification).
* The `StyleNode` handle design and Stylo integration via small, verified patches.
* The filter-layer memory guard and glyph-size guards in the raster.
* `catch_unwind` around layout with cold retry (extend it to style/scene rather than removing it).

## 6. Open questions

* Exact reason `replay::seed` fails for unchanged rows once `RENDER-05` triggers (seeds missing vs `same_input` mismatch); needs instrumentation in `ifc_seeds`.
* Stack usage of the later recursive stages (Taffy, `fragment::builder::Build::node`, scene, a11y build) at depth 256 under MSVC + thin LTO; only the box-tree builder was measured (it overflows first on Linux). Needs a Windows run (none available to me).
* Whether a minimized/occluded native window can stop updates long enough for RENDER-04 to matter in production (currently masked by the accessibility refresh flush).
* Which fraction of the 4K frame time is the full-surface conversion vs Wayland `present` (not separately timed).
* Remaining resource use after RENDER-01 is fixed: retained mmaps referenced by cached parley layouts and glyph-cache entries per reload were not quantified separately.

## Environments exercised / assets left

* Local Linux only (no Docker, no Windows, no Proxmox). Used the coordinator's nested KWin compositor read-only as a client; started no compositor.
* Temporary assets: scratch directory `exp/RENDER/` (harness sources, logs, heaptrack data); `target/debug/render-audit-harness` plus its dependency artifacts in the shared target dir (separate hashes from the workspace build). No processes left running.
