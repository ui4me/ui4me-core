# 3. Architecture assessment

## 3.1 What the system is

```
backend process (Python / PowerShell)            ui4me-host (Rust, one process)
┌──────────────────────────────┐   JSON-RPC    ┌─────────────────────────────────────────────┐
│ ui4me_core / UI4ME.Core      │◄────────────►│ transport (reader/writer threads, bounded)   │
│  reader thread → queues      │ Unix socket / │ Host (GUI thread): lifecycle, requests,      │
│  callback worker(s)          │ named pipe,   │   documents, accessibility, presentation     │
│  app code                    │ token auth    │ renderer: DOM → Stylo → box tree → Taffy →   │
└──────────────────────────────┘               │   parley → scene → vello_cpu → softbuffer    │
                                               │ assets: decode workers (images, fonts, SVG)  │
                                               │ winit (patched) · AccessKit (patched)        │
                                               └─────────────────────────────────────────────┘
```

The host owns the platform event loop and everything that must be synchronous with it: layout, painting, text editing, IME, focus, default actions and the accessibility tree. The backend only changes the document through requests and observes it through notifications. Default actions that the backend wants to cancel are declared in advance (`prevent_default` rules) instead of being decided synchronously.

## 3.2 Decisions that are strong and should stay

| Decision | Why it is right | Evidence |
| --- | --- | --- |
| **Two processes, host-owned event loop** | The UI cannot be frozen by backend code; the backend can be any language; the IPC boundary is a natural fault boundary. This is the project's central idea and it holds up. | HOST strengths; BIND: the Python reader never blocks on user code |
| **Declarative default actions instead of synchronous `preventDefault`** | Avoids GUI-thread waits on IPC and the deadlocks that come with them. | INTERACT §2 |
| **Strict, bounded protocol framing** | Length checked before allocation, strict JSON-RPC subset, malformed frames close only the offending connection (12 malformed cases tested). | HOST §1 table |
| **Generation-bound handles** (`Document`, `Node`, live bitmaps) | A late callback can never write into a replacement page. Checked on both sides. | BIND §2, HOST |
| **Process supervision** (systemd-run + `KillMode=control-group`; Windows job object created atomically) | No orphaned backends after host death; verified by HOST and BIND under SIGKILL/SIGABRT/SIGTERM/Ctrl+C. | HOST §2, BIND §2 |
| **Cold pipeline as the oracle for the incremental pipeline** | The incremental box build, layout replay, scene reuse and damage raster are tested against a full rebuild. This is what keeps an incremental engine honest. | RENDER §2 |
| **Layout under `catch_unwind` with cold retry; immutable committed sets** | Content bugs become a retry or a recorded error, not a crash (except stack overflow, see F-01). | RENDER §2 |
| **Deterministic single-threaded CPU raster** | Reproducible pixels for tests, no GPU driver surface, minimal runtime linkage (libc, libm, libgcc_s; Wayland/xkb/fontconfig dlopened). | RENDER §2, RELEASE §2 |
| **Vendoring discipline**: pristine commit, then small documented patches, `verify.sh` reproducing the tree byte-for-byte | Makes every deviation from upstream reviewable and reversible. All four `third_party` crates match crates.io; Stylo and Taffy stacks verify. | RELEASE §1, RENDER-12 |
| **Promote-not-rebuild for the PowerShell module** | The module ships exactly the host binaries already released. Extend this to PyPI (F-11). | RELEASE §2 |
| **A CSS profile that is documented and matches the runtime** | Every feature the renderer reviewer cross-checked matched `css_profile()`. | RENDER §2 |

## 3.3 Decisions that create risk now

### A. "Every DOM event goes to the backend" (protocol v1 event contract)

The renderer installs root capture listeners for every event type and the host forwards all of them; subscriptions exist only inside the bindings. Combined with a depth-based disconnect on a 128-message queue, ordinary UI activity terminates applications (F-02). It also wastes GUI-thread time serialising events nobody wants, and it makes any future "drop when saturated" policy semantically dangerous because the host cannot tell important events from noise (draft PR #3, register §2.3).

**What is needed, in order.** The 1.0.2 fix is small: bound the outbound queue by bytes instead of by a 128-message count (an 8 MiB byte cap already exists), write each frame in one syscall, and make both bindings never drop control notifications (roadmap 1.2–1.3). Nothing is dropped by the host after that, so no new protocol is needed to stop the failures. Host-side subscription by event type is the one protocol change worth making in 1.1, because it removes the wasted GUI-thread and binding work (PowerShell spends ~0.65 ms per unsubscribed event). The fuller design below is the target **only if** a measured case later forces the host to drop discrete events:

1. *Host-side subscription.* The client declares event types (optionally per element) in `host.hello` capabilities and through a `set_event_types` command; the bindings already hold the handler set. Without the capability, v1 behaviour continues.
2. *Two lanes.* Responses, host→backend requests and lifecycle notifications (`app.shutdown`, window `closed`/`close_requested`, `request.cancelled`, `document.error`, `app.activated`) are reliable and never dropped. DOM events travel in a separate lane.
3. *Coalescing* for continuous events (pointer moves, wheel, scroll, resize, preference changes): latest-wins per (document, type, target).
4. *Stall-based disconnect plus explicit loss signalling.* Disconnect only when the writer makes no progress for a deadline, never because of queue depth. If discrete events must be dropped, the host sends a reliable `document.events_lost` marker with sequence numbers so applications can resynchronise.

HOST-01 contains an implementation-level design for all four (data structures, tests). The adversarial review argued, and this report accepts, that items 2–4 should wait for evidence of need ([09-adversarial-review.md](09-adversarial-review.md), D4).

### B. Unbounded native recursion over content depth

Box-tree construction recurses on the native stack at ~24 KiB per nesting level for `<div>` and ~32 KiB for `<fieldset>` (about four frames of ~6 KiB per element). usvg recurses on the asset-worker stack with no depth limit. The documented DOM depth limit (256) was chosen without reference to either stack, and none of the threads have an explicit stack size. Result: content-triggered process aborts (F-01, F-03, F-06), which `catch_unwind` cannot intercept.

**Fix the invariant, not one site (Phase 1).** Establish a single rule: *every recursive stage must survive the documented maximum depth on the production stack, with 2× headroom, on every platform*, and enforce it with a test that runs at the documented maximum on a thread whose stack equals half the production size. A larger Windows PE reserve alone does not meet this invariant: it does nothing on Linux, where `<fieldset>` already overflows the 8 MiB main stack at ~252 levels. The recommended fix is to run the event loop on one explicitly sized thread on both platforms (the vendored winit supports `with_any_thread` on Win32 and Wayland), keep a matching PE reserve as a backstop, give asset workers an explicit stack, add an SVG nesting cap in the pre-scan, and shrink the hottest frames (pass `&BoxStyle` instead of by value). Converting the recursion to an explicit work stack is the durable fix if stack costs keep rising.

### C. Five independent content→window geometry computations

Hit testing and painting apply transforms and fixed positioning correctly, but caret placement, `<select>` option picking, the IME candidate area, accessibility bounds and `get_rect` each compute positions separately, from the scroll chain and without transforms (F-07). PR #4 patches one of the five.

**Introduce one geometry service (Phase 2).** One function maps a fragment's local coordinates to window coordinates, accounting for scroll containers, the nearest fixed ancestor and transforms. All five consumers call it. A property test asserts that hit-testing the centre of any element's reported rectangle returns that element. This removes a whole class of bugs rather than one instance.

### D. Headless mode is not the same engine loop

Headless mode never advances the animation/transition clock and only flushes pending updates when a read forces it (F-12, RENDER-04). It is the only automated end-to-end mode, so whole defect classes (F-02, the windowed half of F-04, frame-driven cost) are invisible to CI.

**Make headless a deterministic test backend (Phase 2).** Run the same frame pump without a surface, driven by a virtual clock that tests can advance (`test.advance_time` behind the test-mode flag). For frame-driven behaviour (animations, transitions, timers, event volume) this is far cheaper than lab qualification; it cannot replace native checks of input, IME, accessibility or DPI.

### E. Incremental pipeline is incremental only at the edges

Every update walks the whole document in style bridging, host-tree rebuild, fragment building and scene preparation (~27–31 ms per `set_text` at 2,000 rows); a cache aging rule causes a cliff after 8 passes; the accessibility tree is rebuilt on every repaint, even with no assistive technology connected (F-16a, F-16b). This is proportionate for small forms and the primary use case, but the documented long-log and table use cases hit it.

**Do not redesign yet; make it measurable first (Phase 3).** Add work counters (paragraphs shaped, host nodes rebuilt, scene subtrees rebuilt, a11y nodes rebuilt) and assert them in long-document regression tests. Fix the two cheap root causes (cache touch on replay, memoised family resolution; gate a11y rebuilds on adapter activity and semantic change). Only then decide whether subtree-skipping in the remaining O(N) walks is worth its complexity.

### F. Font identity owned by a third-party cache

The renderer keys its own font side-table by parley's per-load blob id, which is not stable because parley prunes its source cache every 128 builder calls. This is the root cause of issue #2 (F-04). The fix is small (shared source cache, keep the blob alive, bound the table) but the lesson is architectural: *identities that the engine relies on for caching must be owned by the engine*, not borrowed from a dependency's eviction policy. Apply the same check to image and SVG identities.

### G. Work requested by the backend runs unbounded on the GUI thread

Each pump processes up to 64 messages regardless of how long they take, and nothing re-wakes the loop when the budget is exhausted (HOST-05, HOST-06). Combined with the superlinear paths in F-14, one request can freeze all windows for seconds. The architecture docs promise that rendering keeps working "while the backend is busy", which is true for waiting and false for work. A time-budgeted pump (process until ~8 ms, then yield and re-wake) and per-command duration logging are cheap and should come with the Phase 2 work.

## 3.4 Expensive-to-reverse decisions to make deliberately

| Decision | Cost of reversing later | Recommendation |
| --- | --- | --- |
| v1 "all events" semantics | Clients may start depending on receiving unsubscribed events. | Introduce type-level subscription as a negotiated capability in 1.1, while there are only two first-party clients. Keep `host.hello` tolerant of unknown keys so newer bindings can negotiate with older hosts. |
| Public direct-start contract ("code above `start()` runs twice") | Documented, taught in the agent skill and examples. | Keep; it is a reasonable trade-off. Harden its edges (F-26) instead of replacing it. |
| winit `0.31.0-beta.4` pin with two patched platform crates | Every winit upgrade means re-applying four patches. | Keep the pin until a non-beta 0.31 exists; then upgrade once, deliberately, with the native acceptance suite. |
| Taffy fork with ~1,400 lines of subgrid | Every Taffy update is a manual rebase; subgrid bugs are now this project's. | Freeze new Taffy feature work; try to upstream; schedule `verify.sh` in CI (F-27). |
| CPU-only raster | A GPU path would be a large new subsystem. | Keep. Add damage-limited conversion/present (RENDER-08) before considering anything else. |
| Single-crate renderer of ~123k lines with crate-wide `#[allow(dead_code)]` | Dead code accumulates invisibly. | Remove the blanket allow; delete or feature-gate the unreachable paged-media pipeline (RENDER-11). |

## 3.5 Simplifications worth making

- **Delete the committed source-hash manifest from CI** and generate it at publish time (F-09). In a Git repository the tree hash already is the manifest.
- **Single-source the version** (F-21) and delete the duplicate release workflows (`release-linux.yml`, `release-windows.yml`) in favour of one reusable build workflow (RELEASE-18).
- **Delete or feature-gate unreachable paged-media code** (~1,500 lines, RENDER-11) and the unused `protocol::MAX_OUTGOING_BYTES` constant (HOST-17).
- **Replace string-matching workflow tests** with structural YAML assertions plus actionlint/zizmor (RELEASE-15).
- **Make `qualification` records machine-generated** from the acceptance tools instead of hand-written (F-24).

## 3.6 Is the architecture proportionate?

Mostly yes. The renderer is large for "forms on the desktop", but most of its size is a real CSS engine (Stylo) and layout (Taffy) that the project integrates rather than writes; the project's own code concentrates on adapters, incremental machinery and platform integration. The host is compact and carefully built. The places where complexity exceeds value are in **release machinery** (manifests, hand-written ledgers, version literals, duplicated recipes), not in the runtime. The places where the runtime is *under*-engineered are the IPC event contract, recursion safety, shared geometry and headless fidelity. These are the four architectural investments this audit recommends, each sized to what the evidence requires; everything else is incremental repair.
