# INPUTSEC: input processing, resource policy and dependency risk (UI4ME Core 1.0.1 @ c6fd71d)

> **Coordinator note (post-review verification).** INPUTSEC-02 was re-run on the published 1.0.1 artifacts: 500 nested `<g>` pass and 700 abort the host on **both** Linux and Windows. The statement below that Windows asset workers overflow "far shallower" is not correct: Rust gives spawned threads a 2 MiB stack on every OS; only the main thread inherits the 1 MiB PE reserve. Severity is unchanged. INPUTSEC-01/07 are consolidated with RENDER-02 as F-01. Two further corrections from the adversarial review: (1) in INPUTSEC-01, "~6.4 KiB per element" is per stack *frame*; with ~4 frames per element the cost is ~24 KiB per `<div>` level and ~32 KiB per `<fieldset>` level; (2) in INPUTSEC-02, the docs do not call image sources "explicitly untrusted" (they speak only of trusted markup and text operations); read it as "a source the docs do not require to be trusted". The register rates F-03 High severity, Low likelihood. `<scratch>/` paths were session-temporary.


## 1. Scope and method

Read (targeted): `crates/ui4me-assets/src/{policy,uri,request,worker,text,data_url,bundles}.rs`, `fs_guard/{mod,unix,windows}.rs`,
`decode/image_rs.rs`, `fonts/woff2.rs` (header, directory, Brotli), `svg/{prescan,usvg_adapter}.rs`;
`crates/ui4me-document/src/{limits,parse/mod,parse/sink,batch/decode,constraint}.rs`;
`crates/ui4me-renderer/src/{document.rs (limits, check_html, validate_document, parse_profile), fuzz_gate.rs, fuzz_resources.rs, pipeline/inline_svg.rs, raster/validate.rs, layout/coordinator/atomic.rs}`;
`crates/ui4me-host/src/{main.rs, host/native.rs::draw}`; build scripts; `Cargo.toml`, workflows; `patches/README.md`, `patches/*/verify.sh`;
`THIRD_PARTY_NOTICES.md`; docs `html-css-scope.md`, `python-api.md`, `architecture.md`.

Binary under test: the published 1.0.1 Linux host from the PSGallery package (sha256 `be08dd88…e9136`, matches
`HOST-SOURCE-MANIFEST.json`), copied to `…/scratchpad/exp/INPUTSEC/bin/ui4me-host-1.0.1`. Experiments use its headless
`render --html X --output Y` subcommand (`crates/ui4me-host/src/snapshot.rs:48`: `Document::parse_hosted` + resources +
layout + paint on the main thread). `render` does not exercise accessibility, hit testing or the backend protocol.

Reproduction kit (all under `<scratch>/exp/INPUTSEC/`):
`gen_cases.py <dir>` writes the test documents, `run_cases.sh <host> <dir>` runs them; last output in `run_cases.out`:

| Case | Result |
| --- | --- |
| 30 nested `<div>`, `ulimit -s 1024` | rc 0 |
| 40 nested `<div>`, `ulimit -s 1024` | rc 134, `thread 'main' has overflowed its stack` |
| 252 nested `<fieldset>`, default 8 MiB | rc 134, main-thread stack overflow |
| SVG image, 700 nested `<g>` (file, or 10.6 KB `data:` URL in `<img>`/CSS background) | rc 134, `thread 'ui4me-assets-0' has overflowed` |
| SVG image, 2000 chained `clip-path` | rc 134, asset-worker stack overflow |
| inline `<svg>` `feTurbulence numOctaves=10000`, 300x300 | rc 0 after 47-57 s on the main thread |
| 40,000 nested `<div>` (200 KB) | `SIZE_LIMIT` after 4.6-6.3 s |
| 26 nested `display:inline-block` | 8-11 s |
| gradient with 5000 stops / `filter` with 65 functions / 129 nested `overflow:hidden` | rc 1, `INTERNAL_INVARIANT` / `BUDGET_EXCEEDED` for the whole frame |
| 3000 `box-shadow`s / 5000 `text-shadow`s on 2000 chars | 8.9 s / 29.5 s |

Other commands: PE header of the published `ui4me-host.exe` (`SizeOfStackReserve = 1048576`); gdb backtraces and
stack-pointer deltas on the release binary; `cargo deny --config <scratch>/deny.toml --locked check advisories` and
`--offline check licenses bans sources` (config and outputs `deny.toml`, `deny_advisories.txt`, `deny_lic.txt`,
`deny_list.txt` in the kit dir); `patches/stylo/verify.sh` and `patches/taffy/verify.sh` with `TMPDIR` in scratch.
Not run: anything on Windows (conclusions there are from code plus the PE header), workspace unit tests (a full
`verify.py` build held cargo), cargo-fuzz.

Threat model note: the docs say HTML-inserting APIs take **trusted markup only** (`docs/reference/python-api.md:230`,
`docs/concepts/html-css-scope.md:157`) and that there is no network access (`html-css-scope.md:97`,
`architecture.md:80`). Findings below are rated accordingly: issues reachable with trusted markup (deep app layouts,
user-supplied image bytes shown through a bundle or `data:` URL) rate higher than those needing HTML injection.

## 2. Strengths to preserve

* Open-then-verify file guard (`fs_guard/mod.rs:109-144`): the opened handle's final path is compared component-wise
  with canonical roots, then `fstat` regular file + size, then `read_limited`. Symlink and junction escapes are denied
  (Linux red-team test passes; Windows junction test exists). Missing files outside roots report `RESOURCE_DENIED`,
  not `FILE_NOT_FOUND`.
* Windows lexical rejection of device/verbatim paths, ADS colons, reserved names, trailing dot/space and wildcards
  (`fs_guard/windows.rs:38-69`); percent-decoded `..`, `/`, `\`, NUL segments rejected (`uri.rs:144-150`).
* `data:` size bounded before decoding (`data_url.rs`), raster decode limits set from the probed header with a bounded
  `max_alloc` (`decode/image_rs.rs:86-104`), pixel budget checked before reservation (`worker.rs:281-300`).
* WOFF2: declared sizes capped at 64 MiB before allocation, Brotli output bounded by `take(sum_stored + 1)` and must
  match exactly (`fonts/woff2.rs:157, 218-270`). WOFF1 equivalent.
* SVG: DTDs refused, every external `href`/`url()` refused, only `data:` rasters admitted (`usvg_adapter.rs`); inline
  `<svg>` capped at 2000 elements / depth 48 / 512 KiB (`inline_svg.rs:29-37`), and those caps worked in tests.
* Asset worker panics are contained with `catch_unwind` and reservations released by `Drop` (`worker.rs:104-115, 386-404`).
* Bundles are exact-key in-memory lookups, with no file-system fall-through (`bundles.rs` header).
* `pattern` uses the linear-time `regex` crate with a size limit (`constraint.rs:214-218`).
* Stylo and cssparser nesting limits held: 20,000-deep `calc()`, `:is()`, `:not()`, `@media`, `@supports`, `@layer`,
  `@container`, blocks and CSS nesting all rendered in under 1 s.
* Builds use `--locked` (setup.py, verify.py, clippy); toolchain pinned; no git sources (`cargo deny check sources` ok).
* Binary licence bundle (`THIRD_PARTY_LICENSES.txt`) lists all 380 packages with texts, including MPL-2.0 and the Noto
  OFL notice.

## 3. Findings

### INPUTSEC-01: Deeply nested HTML overflows the main-thread stack and aborts the host
- Classification: confirmed defect (reproduced).
- Component: `crates/ui4me-renderer` box-tree builder (`layout::box_tree::build::Builder::{children,block_boxed,element_dispatch,element}`), reached through the render/layout path; depth gate `crates/ui4me-renderer/src/document.rs:41` (`MAX_DEPTH = 256`) and `validate_document` at `:2487`.
- Evidence: the DOM depth gate rejects only at **256** (`document.rs:2487` `if count > MAX_NODES || depth > MAX_DEPTH`). Box-tree construction recurses ~4 frames per element. gdb on the release binary at `ulimit -s 1024` (the Windows reserve) shows a pure `children -> block_boxed -> element_dispatch -> element` cycle; stack-pointer delta ~6.4 KiB per element. 40 nested `<div>` overflow at 1 MiB; at the Linux 8 MiB default ~254 `<div>` reach the gate, but some elements cost far more per level: 252 nested `<fieldset>` overflow at **8 MiB**, and at 1 MiB `<fieldset>` overflows by depth ~30.
- Reproduction: `gen_cases.py` -> `div40.html` (`ulimit -s 1024`), `fieldset252.html` (default). Both rc 134, `thread 'main' has overflowed its stack`, `fatal runtime error: stack overflow, aborting` (SIGABRT). Also reproduced live: the same layout runs on the GUI thread via `host/native.rs::draw` (`render_into`), which has **no** `catch_unwind`, so an overflow there aborts the whole host process, not one document.
- Severity: High. Likelihood: Medium (needs a document ~250 deep, or ~30 deep on Windows with `<fieldset>`; deeply nested real layouts and generated markup reach this, and `insert_html` subtrees are only depth-gated at 256).
- Impact: whole-process abort (`panic=unwind` cannot catch a guard-page hit; it is a hard `abort()`), taking every window and the backend connection with it. On the published Windows build the main-thread reserve is **1 MiB** (PE `SizeOfStackReserve = 1048576`), so the safe depth is far below 256 and varies by element, making the 256 gate ineffective there.
- Root cause: `MAX_DEPTH = 256` was chosen for the parser/DOM, not for the recursive layout builder's per-element stack cost, and it is not scaled to the 1 MiB Windows main-thread stack. The builder recurses on the native call stack.
- Remediation: (a) set the host main thread (and the winit/event-loop thread on Windows) to an explicit large stack via `std::thread::Builder::stack_size` the way the fuzz worker already does (`fuzz_gate.rs:267` uses 8 MiB) — `main.rs` currently calls `execute()` straight on the default main stack; the Windows PE reserve must also be raised (linker `/STACK` or a spawned worker). (b) Lower `MAX_DEPTH` to a value proven safe at 1 MiB with the most expensive element (measure `<fieldset>`/table/flex), or convert the hottest builder recursion (`children`/`block_boxed`) to an explicit work stack. (c) Add a layout-time depth guard independent of the DOM gate. Test: a `parse_hosted` unit test at the chosen max depth for `div`, `fieldset`, `table`, `flex`, run with the production stack size, asserting no abort.
- Alternatives/tradeoffs: raising the stack is cheap and localised but only moves the ceiling; an explicit work stack is the durable fix but is a larger refactor of `build.rs`. A lower `MAX_DEPTH` is the quickest safety win and costs expressiveness.
- Complexity: M (stack-size + lower gate) / L (iterative builder). Confidence: High.

### INPUTSEC-02: SVG with deep element nesting overflows the asset-worker stack and aborts the host
- Classification: confirmed defect (reproduced).
- Component: `ui4me-assets` SVG pipeline on worker threads; usvg `convert_element`/`convert_children`/`convert_group` recursion, reached from `worker.rs::decode_and_publish` -> `svg::usvg_adapter::UsvgRenderer::parse`. Worker stack set at `worker.rs` supervisor spawn (no custom `stack_size`, so OS default — on Windows 1 MiB).
- Evidence: gdb backtrace on `g700.svg` shows ~2350 frames of `convert_element -> convert_children -> convert_element::{closure#0} -> convert_group` before overflow on thread `ui4me-assets-0`. The owned pre-scan (`svg/prescan.rs`) counts elements and rejects `>SVG_MAX_NODES` and caps `SVG_MAX_NESTING = 4` (`policy.rs:21`) — but that nesting cap is **not applied to generic container `<g>` nesting**; 700 nested `<g>` pass the pre-scan and reach usvg, which recurses with no depth limit. Threshold on Linux 8 MiB: overflow between depth 500 and 600 nested `<g>`; at depth 1024 usvg's own node cap turns it into a clean `DECODE_FAILED`, so only 512-1023 overflow — but Windows at 1 MiB overflows far shallower.
- Reproduction: `gen_cases.py` -> `g700_file.html` (SVG file under the asset root), `g700_data.html` (10.6 KB `data:` URL in an `<img>`; also reproduced via CSS `background-image`), `clip2000.html` (2000 chained `clip-path`, a different unbounded usvg recursion). All rc 134, `thread 'ui4me-assets-N' has overflowed its stack`, SIGABRT of the whole process.
- Severity: High. Likelihood: Medium-High. This is the highest-impact finding against the stated threat model: a `data:` SVG in an `<img src>` is reachable through `insert_html` of a user value that was *not* HTML-escaped into an attribute, and more importantly a **user-chosen image file** shown in an `<img>`/CSS background (an explicitly untrusted source per the docs) is a plain SVG the app renders. ~10 KB of bytes aborts the host.
- Impact: hard process abort from decoding one untrusted image; the worker's `catch_unwind` (`worker.rs:388-404`) cannot catch a stack-overflow abort.
- Root cause: the pre-scan enforces an element *count* and a 4-deep cap on specific constructs but not on arbitrary element nesting, and usvg/resvg recurse on the native stack; the asset workers use the default (small on Windows) stack.
- Remediation: in `svg/prescan.rs`, reject documents whose maximum element nesting exceeds a safe bound (roxmltree already walks the tree, so track depth) before handing bytes to usvg; pick the bound against the worker stack size. Independently, give the asset workers an explicit large `stack_size` at spawn in `worker.rs` (as the fuzz worker does). Test: feed `g<N>.svg` and `clip<N>.svg` at the bound through the public image path and assert `DECODE_FAILED`, not abort.
- Alternatives/tradeoffs: a depth cap may reject a legitimate but deeply grouped icon; set it generously (e.g. 100-200) and still well under the stack ceiling. Bumping only the stack size is insufficient on its own because usvg nesting is unbounded.
- Complexity: M. Confidence: High.

### INPUTSEC-03: SVG filter primitives run uncapped on the main thread (CPU/time DoS)
- Classification: reproducible risk.
- Component: resvg filter rasterisation (`resvg::filter::turbulence`, `feMorphology`, `feConvolveMatrix`, chained primitives) via `svg::raster_cache::SvgRasterCache::raster` -> `ResourceManager::paint_image`, called from the renderer paint path (`pipeline/resources.rs::lookup_at`) **on the main/GUI thread** (SVG raster happens at paint time, not on the asset worker).
- Evidence: gdb shows the main thread spending the whole time in `resvg::filter::turbulence::noise2` for a 300x300 `<rect>` with `feTurbulence numOctaves="1000000"` (killed at 60 s). Time scales with the octave count: inline `<svg>` `numOctaves=10000` took 47 s and `=1000` took 4.8 s at 300x300, all rc 0 (no limit hit). `feMorphology radius=1000` took 4 s; 1500 chained `feGaussianBlur` 8.3 s. The decode deadline (`policy.decode_deadline_ms`, default 10 s, max 60 s) is a worker-pipeline deadline and does **not** bound this main-thread paint cost; `inline_svg.rs` caps element count/depth but not per-filter cost; usvg does not clamp `numOctaves`.
- Reproduction: `gen_cases.py` -> `turbulence.html` (inline `<svg>`, octaves 10000 => ~47 s main-thread stall). Raise octaves for an effectively unbounded stall.
- Severity: Medium. Likelihood: Medium (needs an SVG, either inline — passing the inline caps, which bound structure not filter cost — or a user-chosen image file).
- Impact: a single small SVG freezes the UI thread for tens of seconds to effectively forever (no wall-clock cap on paint). On the live host this blocks all input, frames and backend servicing for that time.
- Root cause: no bound on filter-primitive work (octaves, kernel area, morphology radius, primitive count), and filter rasterisation is synchronous on the paint thread with no time budget.
- Remediation: clamp dangerous filter parameters in the SVG prescan/adapter before rasterising — cap `numOctaves` (browsers effectively cap near ~5-10), `feConvolveMatrix` order, `feMorphology` radius and total primitive count; and/or move SVG rasterisation off the GUI thread or wrap it in a wall-clock budget. Test: assert bounded time for `turbulence.html` and friends.
- Alternatives/tradeoffs: clamping changes rendering of pathological inputs only; browsers already do this. A time budget is more general but harder to make deterministic.
- Complexity: M. Confidence: High (timings reproduced; thread identity from gdb).

### INPUTSEC-04: Paint-budget rejections (deep scopes, too many gradient stops / filter functions) drop the whole frame, and live they drop the document fatally
- Classification: confirmed defect (reproduced) for the error behaviour; design-tradeoff for the thresholds.
- Component: `crates/ui4me-renderer/src/raster/validate.rs` (`MAX_STOPS = 1024`, `MAX_FILTER_FUNCTIONS = 64`, scope nesting cap 128); consumed by the host at `host/native.rs::draw` which on any render error takes `record.document`, sets a failure and emits `document.error` with `"fatal":true` (`native.rs:545-574`).
- Evidence: a gradient with 5000 stops fails the *entire* render with `INTERNAL_INVARIANT: item 1: invalid FillRect`; `filter` with 65 functions -> `INTERNAL_INVARIANT: scope 0: invalid scope payload`; 129 nested `overflow:hidden`/`transform` -> `BUDGET_EXCEEDED: item 1: scope nesting deeper than 128`. In `render` these are rc 1 (nothing drawn). In the live host the same `Err` from `render_into` triggers the fatal `document.error` path and the document is dropped. So one over-limit CSS value (reachable via `insert_html`, or a `<style>`) takes down the whole document's rendering, not just the offending element.
- Reproduction: `gen_cases.py` -> `stops5000.html`, `filters65.html`, `overflow129.html`.
- Severity: Medium. Likelihood: Medium.
- Impact: denial of rendering for a whole screen from a single CSS property; the `INTERNAL_INVARIANT` code mislabels author-input-driven limits as an internal bug (it is a validation failure of a bounded scene), and the host escalates it to a fatal document error.
- Root cause: the scene validator rejects the whole built scene instead of degrading the one offending display item (clamp stop count, drop excess filter functions, flatten scopes past the cap); and the host treats any `render_into` error as fatal.
- Remediation: in `raster/validate.rs` / scene building, clamp rather than reject (cap stops to `MAX_STOPS`, drop filter functions past `MAX_FILTER_FUNCTIONS`, stop applying scopes past the nesting cap) and map genuinely unavoidable cases to a non-`INTERNAL_INVARIANT` code; in `host/native.rs::draw`, treat a render error as a skipped frame / per-item degrade, not a document kill. Test: each case renders (rc 0) with the degraded output.
- Alternatives/tradeoffs: clamping changes output for pathological values only. Keeping the hard limit but making it non-fatal and correctly coded is the minimum.
- Complexity: M. Confidence: High (render behaviour reproduced; live fatal path read from source, not run).

### INPUTSEC-05: Parser is quadratic on deeply nested unclosed block elements
- Classification: reproducible risk.
- Component: html5ever tree builder (`close_p_element_in_button_scope` -> `in_scope_named`, scope scan is O(open-elements)) driven by `ui4me-document` sink; the DOM node gate (`document.rs:40` `MAX_NODES = 65_536`) is the only bound.
- Evidence: gdb on 65,000 nested `<div>` repeatedly samples inside html5ever `in_scope_named`/`elem_name` (an O(depth) scan on each of O(nodes) steps). Wall time grows super-linearly: 5k `<div>` 0.17 s, 10k 0.63 s, 20k 2.8 s, 40k 6.3 s. All end in `SIZE_LIMIT` (the node gate), but only after the quadratic parse; a 200 KB input (well under the 1 MiB `MAX_HTML`) burns 4-6 s before rejection.
- Reproduction: `gen_cases.py` -> `divs40k.html`.
- Severity: Low-Medium. Likelihood: Medium.
- Impact: a small (<=1 MiB) document can cost several seconds of CPU in the parser before the node limit fires; on the live host parsing is on a worker (`host/documents.rs:587`) so it mainly wastes CPU/time rather than blocking the UI, but it delays the document and is an asymmetric-work vector.
- Root cause: `MAX_NODES = 65_536` combined with per-node O(depth) scope scans; nothing caps *open-element depth* early during tokenising.
- Remediation: enforce an open-element depth cap in the sink during parsing (reject once the open-element stack passes a bound, e.g. a few hundred) so the quadratic blowup is cut off early, consistent with INPUTSEC-01's layout depth bound. Test: assert `divs40k.html` is rejected quickly.
- Complexity: S-M. Confidence: Med (quadratic shape reproduced; exact html5ever cost not profiled line-by-line).

### INPUTSEC-06: Shrink-to-fit inline-block layout is exponential in nesting depth
- Classification: reproducible risk.
- Component: `crates/ui4me-renderer/src/layout/coordinator/atomic.rs::layout_atomic_child` / `atomic_intrinsic_width` — an `auto`-width atomic does a min-content and a max-content intrinsic pass (`:206-207, :221-222`), each of which recurses into children that each do the same two passes.
- Evidence: render time for N nested `display:inline-block` roughly doubles per level: depth 20 ~0.4 s, 22 ~2 s, 23 ~3.7 s, 24 ~4.4 s, 25 ~6.1 s, 26 ~11.5 s; `label`/`span` wrappers the same. Depth is bounded only by `MAX_DEPTH = 256`, far beyond the point this becomes minutes/hours. `span-ifb` (alternating span/inline-block) at depth 24 also 3.3 s.
- Reproduction: `gen_cases.py` -> `ifb26.html` (~8-11 s); increase depth to make it unbounded.
- Severity: Medium. Likelihood: Low-Medium (needs ~20+ nested shrink-to-fit atomics; achievable in generated UIs).
- Impact: superlinear/exponential CPU and time from a tiny document; combined with INPUTSEC-03 and -05, several independent "small input, huge work" vectors exist with no global layout time budget.
- Root cause: intrinsic-size passes are recomputed per level without memoising the child's min/max-content across the two parent passes (the final pass does `clear_subtree_cache`, but intrinsic passes re-descend).
- Remediation: memoise min-content/max-content intrinsic widths per node within one layout (Taffy has a cache; ensure the intrinsic passes consult it rather than recomputing), and/or add a layout work/time budget. Test: assert near-linear scaling of nested inline-block depth.
- Alternatives/tradeoffs: caching is the correct fix and also speeds normal pages; a depth cap is a cheaper stopgap.
- Complexity: M. Confidence: Med (exponential timing reproduced; root cause inferred from `atomic.rs`, not instrumented).

### INPUTSEC-07: `MAX_DEPTH`/`SVG_MAX_NESTING` are not tuned to the 1 MiB Windows stack and no worker/main stacks are sized
- Classification: plausible concern needing verification on Windows (Linux parts reproduced).
- Component: `crates/ui4me-host/src/main.rs` (runs `execute()` on the default main stack; no `stack_size`), `worker.rs` supervisor (default worker stack), the published Windows PE (`SizeOfStackReserve = 0x100000 = 1 MiB`).
- Evidence: the Windows host reserves only 1 MiB for the main thread; INPUTSEC-01/-02 overflow well under the depth gates at 1 MiB. I could not run on Windows, so the exact safe depth there is unverified, but the PE reserve and the Linux thresholds make overflow at modest depth near-certain.
- Severity: High (Windows). Likelihood: Medium. Impact/Root cause/Remediation as in INPUTSEC-01/-02 (size the main and worker and event-loop thread stacks explicitly; raise the PE `/STACK` reserve; scale the depth gates).
- Complexity: S (stack sizing). Confidence: Med (Windows not executed).

### INPUTSEC-08: Four unmaintained-dependency advisories; no `deny.toml` / advisory gate in CI
- Classification: maintenance problem (confirmed).
- Component: dependency tree; CI.
- Evidence: `cargo deny check advisories` reports exactly 4 `unmaintained` errors, **0 vulnerabilities**: `ttf-parser 0.25.1` (RUSTSEC-2026-0192, via `fontdue`, the diagnostic probe rasteriser), `zvariant 4`/`zvariant_derive`/`zbus_names` (RUSTSEC-2026-0370/0372/0371, via `accesskit_unix`/`ashpd`/`zbus` on Linux AT-SPI). All "No safe upgrade available". No `deny.toml` exists in the repo and no workflow runs `cargo deny`/`cargo audit`, so these are invisible in CI. (Advisory DB as of 2026-11-13.)
- Severity: Low. Likelihood: n/a (no known exploit; `ttf-parser` is only the diagnostic probe font path, and the zbus stack is Linux desktop IPC). Impact: slow-rotting transitive deps with no security SLA; a future *vulnerability* advisory in the same tree would also go unnoticed without a gate.
- Remediation: add a committed `deny.toml` and a CI job running `cargo deny check advisories licenses sources`; `ignore` the four unmaintained IDs with a dated rationale so a *new* advisory still fails the build. Track `fontdue`->`skrifa` (already a direct dep) to drop `ttf-parser`, and the zbus 5/6 `names` migration. Complexity: S. Confidence: High.

### INPUTSEC-09: Vendored Stylo (MPL-2.0) / Taffy — provenance verified, but no CI drift gate and large carried Taffy changes
- Classification: maintenance problem / design tradeoff.
- Component: `patches/stylo/stylo` (servo/stylo `37c1ed3` + P-006-1..20), `patches/taffy/taffy` (DioxusLabs/taffy `fca167c` + P-008-1..6), `patches/*/verify.sh`.
- Evidence: both `verify.sh` scripts pass against the pristine cached checkouts, so the vendored trees are exactly upstream + the listed patches (good). Stylo patches are small (most <50 changed lines). Taffy carries a large change: `P-008-4` (CSS Grid subgrid) is ~1539 changed lines across 10 files, `P-008-5` ~221 lines — a substantial local fork of layout. **No CI job runs `verify.sh`** (grep of workflows and `tools/*.py` finds no invocation), so upstream drift and unreviewed edits to the vendored trees are only caught by running the scripts manually, and they need a pristine cargo checkout present (a hard prerequisite, per `patches/README.md`). Stylo is MPL-2.0 (file-level copyleft); the binary licence bundle includes the full MPL text and states the modified source is in the sdist (`patches/stylo`), which satisfies MPL §3.2 as long as the sdist ships those files — the published package's `licenses/patches/` ships only `MPL-2.0.txt`, not the patched sources, but the sdist (the actual source distribution) does via `graft patches` in `MANIFEST.in`. Confirm the PyPI sdist and the PSGallery source payload both contain `patches/stylo/stylo`.
- Severity: Low-Medium. Likelihood: Medium (drift at the next upstream security fix; a subgrid bug is now the project's to fix).
- Impact: security fixes in Stylo/Taffy must be re-applied by hand with no automated detection of base/patch drift; the large subgrid patch is long-term maintenance liability; an MPL compliance gap if a binary-only channel ever ships without pointing at the modified source.
- Remediation: run both `verify.sh` in CI (provision the pristine checkout, or vendor a hash of it); record the upstream base revisions and a cadence for pulling security fixes; verify the published sdist actually contains the patched Stylo sources (MPL-2.0 §3.2); consider whether the subgrid fork can be upstreamed to shed the maintenance burden. Complexity: S (CI gate) / L (subgrid upstreaming). Confidence: High.

### INPUTSEC-10: Windows DLL/dlopen search-path exposure (not verified on Windows)
- Classification: plausible concern needing verification.
- Component: Windows host DLL loading; `fontconfig-dlopen`/`wayland-dlopen` features (Linux only, low risk); `build.rs` embeds a manifest.
- Evidence: the published `ui4me-host.exe` imports system DLLs by bare name (`dwrite.dll`, `uiautomationcore.dll`, `dwmapi.dll`, `uxtheme.dll`, `ole32.dll`, `VCRUNTIME140.dll`, etc.). I found no `SetDefaultDllDirectories`/`SetDllDirectory` hardening and no `DEPENDENTLOADFLAG` in `build.rs`/manifest (`crates/ui4me-host/build.rs` only compiles `host.rc`). Rust binaries dynamically link the MSVC runtime (`VCRUNTIME140.dll`) rather than `crt-static` (no `crt-static` in config). If the host is ever launched from an attacker-writable working directory, bare-name loads of non-KnownDLLs could be hijacked. This is standard Windows guidance; I could not test it (no Windows access), and the backend/app normally controls the launch directory.
- Severity: Low. Likelihood: Low. Remediation: call `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` early in Windows startup and/or set the `DEPENDENTLOADFLAG` in the embedded manifest; consider `crt-static` to drop the `VCRUNTIME140.dll` dependency. Verify on the Windows lab VM. Complexity: S. Confidence: Low (not executed).

### INPUTSEC-11: batch/decode.rs — bounds look correct; one near-miss to confirm
- Classification: GOOD, with one check.
- Component: `crates/ui4me-document/src/batch/decode.rs`.
- Evidence: the decoder checks `BatchBytes`/`BatchCommands` limits first, requires a 16-byte header, validates magic/version/flags, and every read goes through `need(n)` before indexing. Lengths are validated (`length < 8 || !multiple_of(4) || start+length > buf.len()`), local ids must be `< defined` (no forward refs), string lengths are `need`-checked and zero-padded, padding must be zero. The whole buffer is decoded before any mutation (comment at top; `batch/exec.rs` applies after), so there is no partial-batch application from a decode error. One item to confirm: `need(n)` computes `self.pos + n` without `checked_add` (`decode.rs:141`); `pos` and `n` are bounded small (`pos <= buf.len() <= batch_bytes <= 1 GiB`, `n <= 8` or a validated `len`), so overflow is not reachable on 64-bit, but using `checked_add`/`self.pos.saturating_add(n)` would make it robust against future 32-bit targets. No defect reproduced.
- Severity: Low (hardening only). Complexity: S. Confidence: High.

## 4. Test gaps and highest-value tests

1. **Stack-safety tests at production stack size** for deep HTML (`div`/`fieldset`/`table`/`flex`/`grid`), deep SVG (`<g>`, chained `clip-path`/`mask`), run with the Windows 1 MiB reserve emulated (`ulimit -s 1024` on Linux CI, plus a real Windows run). These are the single biggest gap: today nothing asserts "a deep document does not abort the process". (covers INPUTSEC-01/-02/-07)
2. **Resource-cost ceilings**: a test that a 300x300 SVG with high `feTurbulence numOctaves`, big `feConvolveMatrix`/`feMorphology`, or thousands of chained primitives renders within a bounded time. (INPUTSEC-03)
3. **Graceful degradation of paint budgets**: gradient stops > 1024, filter functions > 64, scope nesting > 128 should render (clamped), not fail the frame or kill the document; assert non-`INTERNAL_INVARIANT` behaviour. (INPUTSEC-04)
4. **Parser/layout asymmetric-work caps**: deep unclosed blocks and deep inline-block are rejected or linearised quickly. (INPUTSEC-05/-06)
5. **Fuzzing in CI, even tiny**: the `#[ignore]`d harnesses (`fuzz_gate`, `fuzz_resources`) have good corpora and a per-case watchdog but **never run in CI** (grep of workflows: no `--ignored`, no `UI4ME_FUZZ_*`). Run them with a small `UI4ME_FUZZ_SECS` budget (e.g. 60-120 s) in CI, saving failing cases as artefacts. Note: the fuzz workers use an 8 MiB stack (`fuzz_gate.rs:267`), which is *larger* than the production main/worker stacks — so the fuzzer would not even reproduce INPUTSEC-01/-02 as currently configured; add a low-stack fuzz mode. The `mutate` dictionary already includes nesting bombs and `<svg viewBox='0 0 1e9 1e9'>`, so a low-stack run would likely find the overflows immediately.
6. **`cargo deny` gate** with a committed config. (INPUTSEC-08)
7. **`patches/*/verify.sh` in CI** to catch vendored-source drift. (INPUTSEC-09)

### Minimal high-value fuzz plan
- Targets (reuse existing harnesses, add CI budget): `harness_html_pipeline` (parse + exercise + paginate), `harness_css`, `harness_dom_sequences`, `harness_images` (decode + render). Entry = `Document::parse_hosted` and the public resource/image API, which the harnesses already drive.
- Add a **low-stack worker mode** (e.g. `UI4ME_FUZZ_STACK_KIB=1024`) so the fuzz worker matches the production/Windows stack; this is what turns the nesting-bomb corpus into real crash findings.
- Corpus: existing `corpus()` (examples/fixtures/tests) + `builtin_docs()` is good; add the SVG filter/nesting bombs from this kit.
- CI budget: one job, `UI4ME_FUZZ_SECS=90 UI4ME_FUZZ_TIMEOUT_MS=10000`, `--test-threads=1`, upload `UI4ME_FUZZ_OUT` on failure. Cheap, deterministic (seeded xorshift), and would have caught INPUTSEC-01..06.
- Optional: a real `cargo-fuzz`/libfuzzer target for `woff2_to_sfnt`, `data_url::process`, `batch::decode::decode` and `svg::prescan` (pure byte->result functions, ideal for coverage-guided fuzzing) if the toolchain pin is relaxed for a nightly fuzz job.

## 5. Considered and recommend NOT changing
- The open-then-verify guard and the Windows lexical rejection list (INPUTSEC strengths): correct and well-tested; leave as is.
- `data:`/WOFF2/WOFF1/raster size-and-pixel bounds: decompression-bomb protections are in place and effective (WOFF2 Brotli `take(sum_stored+1)` + exact-length check, raster `max_alloc` from the probe). No change.
- Refusing all SVG external references and DTDs (`usvg_adapter.rs`, `prescan.rs`): good; keep.
- The `ui4me-res:` bundle design (exact-key, in-memory, bounded): no path traversal possible; keep.
- `panic=unwind` + worker `catch_unwind`: correct for decoder *panics*; it just cannot catch *stack-overflow aborts*, which is why INPUTSEC-01/-02 need a structural fix rather than more `catch_unwind`.

## 6. Open questions
- Exact safe depth on the real Windows build (1 MiB reserve) for each element type — needs a Windows run (no access here).
- Whether the live GUI thread (winit event loop) has the same small default stack as the raw main thread on Windows, and whether raising the PE `/STACK` reserve is enough or a dedicated large-stack layout thread is required.
- Whether the published PyPI sdist actually contains `patches/stylo/stylo/**` (MPL-2.0 §3.2 source availability); `MANIFEST.in` grafts it, but the built sdist was not inspected here.
- Whether `render_into` errors should ever be fatal to a document in `host/native.rs::draw`, or always a skipped frame (confirm intended host policy).
