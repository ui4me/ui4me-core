# F-15 / INPUTSEC-04 paint budgets, and F-14 CSS filter cost (Linux, release)

## Before (base): a paint budget overflow fails the whole frame

`content_stalls.rs` F-15 tests with the scene pass stashed:

```
a_filter_with_too_many_functions_keeps_the_first_ones: the frame failed: INTERNAL_INVARIANT: scope 0: invalid scope payload
a_gradient_with_too_many_stops_is_thinned:             the frame failed: INTERNAL_INVARIANT: item 1: invalid FillRect
scopes_nested_too_deep_draw_in_their_deepest_allowed_scope: the frame failed: BUDGET_EXCEEDED: item 1: scope nesting deeper than 128
expensive_css_filters_are_bounded: 64 x blur(50px): no result after 1s
```

(Live, the host turns a failed `render_into` into a fatal `document.error`: workstream G's file.)

## After

`crates/ui4me-renderer/src/scene/paint_budget.rs`, run at the end of every `build_scene_with`
(a function of the built list alone, so incremental and cold scenes degrade identically):

| overflow | degradation | report (`BUDGET_EXCEEDED`, stage scene) |
|---|---|---|
| gradient > 1,024 stops (5,000) | keeps 1,024: first, last, evenly spaced | `paint.capability.gradient-stops` |
| `filter` / `backdrop-filter` > 64 functions (65) | keeps the first 64 | `paint.capability.filter-functions` |
| mask > 64 layers / > 8,192 ops per layer | keeps the first ones | `paint.capability.mask-layers` |
| item under > 128 nested scopes (129 x `overflow:hidden`) | drawn in its ancestor scope at depth 128 | `paint.capability.scope-depth` |
| CSS filter work > 100 ms estimated | blur / drop-shadow functions of the costliest filters removed until under | `paint.capability.filter-budget` |

All three audit cases now render (pixel checks in the tests) with the report in `diagnostics()`.

## CSS filter cost (stall_measure, 400x400 box, incl. ~200 ms document set-up)

| case | before ms | after ms |
|---|---|---|
| blur(4px) | 200 | 295 (first case of the run: +~90 ms first-use) |
| blur(1000px) | 223 | 302 (kept, same first-use effect) |
| 64 x blur(50px) | 6,224 | 188 (budget) |
| 64 x drop-shadow(0 0 50px red) | 7,429 | 185 (budget) |
| 64 x hue-rotate | 243 | 238 |
| backdrop-filter blur(128px) | 225 | 216 |
| 64 nested blur(50px) boxes | 25,177 | 197 (budget) |

Model (warm-document sweep, first frame after the change): a filtered layer is the scope's content box
grown by the blur extent of its filters and of the filters inside it, the growth capped at 600 px per
side (vello draws large blurs downsampled: blur(200..1000px) all cost ~25 ms at 400 px), blur 15 and
drop-shadow 20 ns per pixel. Kept as intended: one blur(1000px) at 400 px (29 ms), blur(200px) and
blur(1000px) on a 1200 px viewport (84 ms), drop-shadow(0 0 200px) (41 ms), 50 shadowed cards on a
1200 px viewport (182 ms). Removed: drop-shadow(0 0 500px) on a 1200 px box (measured ~210 ms).

Limits of the model: device scale is not known to the scene and 1 is assumed (cost grows with
scale squared on HiDPI); many small filtered layers are underestimated (~3x for the 50 cards).

Renderer suite after: 546 lib tests (pixel, incremental-vs-cold oracle) + stack_depth + content_stalls pass.
