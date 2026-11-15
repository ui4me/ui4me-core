# F-14 / INPUTSEC-06: shrink-to-fit atomic layout (Linux, release, 16 cores shared)

Tool: `UI4ME_STALL_MEASURE=1 UI4ME_STALL_DEPTHS=... UI4ME_STALL_ONLY=... cargo test --release
-p ui4me-renderer --test content_stalls -- --ignored --nocapture stall_measure`
(each case: `parse_hosted` + render until resources settle, on the engine stack).

Every document pays a fixed ~200 ms first-use text set-up (`ifc_new` in `UI4ME_PROFILE`; the
`control` rows; text services are workstream C's). The regression tests therefore time a mutation
plus render in an already rendered document (`assert_bounded_change` in `content_stalls.rs`).

## Before (integration base db914b2)

| case | ms |
|---|---|
| control: one div | 302 |
| inline-block depth 16 / 20 / 22 / 24 / 26 | 231 / 459 / 1004 / 2702 / 7747 |
| span/inline-block depth 48 (24 inline-blocks) | 2605 |
| nested buttons (insert chain) 12 / 14 / 16 levels, render only | 51 / 128 / 227 |
| inline-block + table cell, 10 / 12 levels | 11 / 40 (x2 per level) |
| inline-block + multicol pairs, 24 / 28 / 32 levels | 37 / 145 / 563 (x2 per pair) |
| depth fuzzer with inline-block, button, multicol units | every case hangs (> 10 s watchdog) |

## After

| case | ms |
|---|---|
| control: one div | 200 |
| inline-block depth 16 / 24 / 26 / 64 / 128 / 250 | 200 / 204 / 200 / 210 / 228 / 296 |
| span/inline-block depth 250 | 278 |
| multicol depth 250 | 211 |
| inline-block depth 250, mutation + render in a warm document | 77 |
| nested buttons 16 levels, render only | 28 |
| inline-block/multicol, button/multicol, button/table pairs, 128 levels | < 60 each |
| fuzz `harness_depth_chains` 120 s, inline-block/button/multicol units added | 151 cases, 0 findings, slowest 2.4 s (24 mutations + renders) |

T1 (`stack_depth.rs`) now includes `inline-block` and `button` (button + table cell units) at depth
256 on half the engine stack: pass.

## Root cause and fix

`layout_atomic_child` cleared the whole cached subtree of an atomic before each `Final` layout, so
every container that lays an atomic out more than once per pass (its parent's intrinsic and final
runs, a button's content box, table cells, flex/grid passes, multicol balancing) re-laid the levels
below on every call: exponential in the nesting depth. Fix (`layout/coordinator/atomic.rs`,
`host_tree.rs`):

1. intrinsic min/max-content widths of an atomic are memoised per node per pass;
2. a `Measure` layout is memoised per node per pass (key: available width);
3. the `Final` layout invalidates only the final-layout (PerformLayout) cache entries of the
   subtree; intrinsic and measure entries do not depend on positions and survive;
4. a `Final` layout repeated with the same input while no node of the subtree stored a result
   since is reused without invalidating;
5. repeated invalidation of an already invalidated subtree is skipped.

Memo keys carry the context the result depends on: the containing-block fallback width
(`container_width`, read only by ratio-only replaced boxes) when the subtree holds a replaced or
control box; no memo during size-containment probes or for breakable floats in a fragmentation root.

Baseline failure of the new tests (`content_stalls.rs`, layout files stashed):
`nested_inline_blocks_lay_out_in_bounded_time`, `inline_blocks_inside_inline_boxes_...`,
`nested_buttons_...` FAILED ("no result after 1s"). Renderer suite (546 lib tests incl. pixel and
incremental-vs-cold oracle tests, stack_depth, content_stalls) passes after.

Residual: the work is still O(atomic depth x subtree) in places (~53 ms for a 250-deep chain).

## T1 stack cost of the new kinds (`UI4ME_STACK_MEASURE=div,inline-block,button ... stack_measure`)

| kind | min stack @64 (KiB) | min stack @256 (KiB) | per DOM level (KiB) |
|---|---|---|---|
| div | 799 | 2959 | 11.2 |
| inline-block | 799 | 3007 | 11.5 |
| button (button + table cell units) | 799 | 2959 | 11.2 |

Well inside the 16 MiB half-engine stack T1 uses.

## Full verification (Linux, after all h-stalls commits)

`python3 tools/verify.py --keep-going --jobs 3`: fmt, check, test, python, headless, renderer, faults
all PASS. `cargo clippy --locked --workspace --all-targets -- -D warnings` clean.
`tools/windows_source_check.py` PASS.
