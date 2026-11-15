# F-14 / INPUTSEC-03: SVG filter cost (Linux, release)

`stall_measure` (content_stalls.rs): a 300x300 `<rect>` with one filter, as inline `<svg>` and as
an `<img>` `data:` image, `parse_hosted` + render until resources settle. Every row includes the
~200 ms per-document text set-up (control: one div = 200-300 ms).

| case | before ms (inline / img) | after ms (inline / img) | after: filters |
|---|---|---|---|
| feTurbulence numOctaves=4 | 235 / 227 | 224 / 218 | run |
| numOctaves=100 | 694 / 685 | 194 / 192 | skipped (est. 590 ms) |
| numOctaves=1000 | 4,829 / 4,803 | 193 / 192 | skipped |
| numOctaves=10000 (audit turbulence.html) | 46,813 / 47,003 | 199 / 191 | skipped |
| feMorphology radius=10 | 230 / 214 | 220 / 208 | run |
| radius=100 / 1000 | 1,083 / 3,771 (inline) | 207 / 200 | skipped |
| feConvolveMatrix order=3 | 207 / 213 | 207 / 203 | run |
| order=30 | 456 / 491 | 200 / 196 | skipped |
| 10 chained feGaussianBlur(1) | 457 / 469 | 194 / 196 | skipped (est. 290 ms) |
| 1500 chained feGaussianBlur(1) | 37,652 / 37,945 | 203 / 196 | skipped |

## Fix (`crates/ui4me-assets/src/svg/{filter_cost,usvg_adapter,raster_cache}.rs`, `policy.rs`)

* `filter_cost::filter_work` estimates the filter work of one rasterisation (ns, release): for
  every filtered group, including content pulled in by clip paths, masks, patterns, `feImage` and
  nested SVG images (once per reference), the filter region in device pixels (layer box, clamped as
  resvg clamps it to 5x the canvas per side) times a per-pixel weight per primitive; turbulence x
  octaves, morphology x kernel area (radius clamped to the region), convolution x kernel cells.
  Weights from `tests/svg_filter_cost.rs` `calibrate` (estimate/measured 0.8-1.5 on the expensive
  primitives, `../scratch` table below).
* A document with filters is parsed a second time with `* { filter: none !important }` injected
  (usvg's user style sheet; important wins over attributes, author rules and `style`), for nested
  `data:` SVG images too.
* `SvgRenderer::rasterize_bounded` draws from that tree when the estimate exceeds
  `SVG_FILTER_WORK_BUDGET` = 100 ms; the raster cache counts it (`RasterStats::filters_skipped`,
  `SvgPaint::filters_skipped`). Inline `<svg>` goes through the same path. A document whose
  unfiltered form cannot be parsed is refused (`BUDGET_EXCEEDED`, broken image) rather than run.

Calibration (400x400 canvas, best of 3; estimate / measured):

```
blur 1 (iir) 48.5 ms est 53.7 (1.11) | blur 8 (box) 12.8 / 13.2 | turbulence 4 44.3 / 44.4
turbulence 50 439 / 523 | morphology 5 12.0 / 11.2 | morphology 40 300 / 458 | convolve 15 124 / 159
diffuse lighting 5.2 / 16.7 | displacement 21.8 / 21.8 | blend 7.3 / 7.4 | blur chain 20 884 / 1074
```

Tests: `ui4me-assets/tests/svg_filter_cost.rs` (pathological filters skipped within 250 ms and
drawn unfiltered; ordinary filters bit-identical to plain resvg; budget depends on raster size;
attribute/style/!important/group/mask/pattern/nested-image filters all disabled in the fallback;
estimates do not undercount); renderer `content_stalls.rs`
`pathological_svg_filters_render_in_bounded_time` (fails on the baseline: "turbulence
numOctaves=10000 inline=true: no result after 1s"). Fuzz: `fuzz_resources::harness_svg_filters`
(new, T6): 90 s, 1,476 cases, 0 findings, slowest 307 ms.

Trade-off: the budget is per raster, so an image with moderately expensive filters drawn large
(e.g. 10 IIR blurs at 300x300, ~290 ms) is now drawn without its filters. The estimate assumes the
reference machine; on a 2x slower machine the bound is ~200 ms per raster. Several images each
under the budget can still add up in one frame (no frame-level SVG budget).
