# F-03: SVG pre-scan cap and asset-worker stack (Linux, release)

SVG_MAX_DEPTH = 128 effective levels; ASSET_WORKER_STACK_BYTES = 8 MiB.
Smallest worker-thread stack (KiB) on which each at-cap case of crates/ui4me-assets/tests/svg_depth.rs
parses and rasterises (child-process binary search with the svg_probe entry):

| case | min stack (KiB) |
|---|---|
| groups-at-cap | 463 |
| clip-chain-at-cap | 167 |
| mask-chain-at-cap | 175 |
| use-chain-at-cap | 503 |
| css-clip-chain-at-cap | 167 |
| nested-image-at-cap | 463 |

Worst 503 KiB: 8 MiB is 16x, the 4 MiB half-stack test 8x. Baseline (2 MiB default worker stack, no
nesting cap): 700 nested <g> and 2000 chained clip-path abort the host (see host-level.md).

Fuzz finding during this work (fixed in the same commit series): a mask chain in which every mask is
masked by the previous one *and* holds a path masked by it has depth n+2 but makes usvg convert mask 0
2^n times (> 30 s at n = 25, harness_svg_depth seed 1 cases 26/128). The pre-scan now also bounds the
reference-expanded element count (SVG_MAX_EXPANDED = SVG_MAX_NODES = 1,000,000).
