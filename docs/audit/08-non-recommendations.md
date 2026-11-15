# 8. Explicit non-recommendations

Changes that were considered and rejected or postponed, with reasons. A professional audit should be as clear about what to leave alone as about what to change.

## Rejected

| Considered | Decision | Why |
| --- | --- | --- |
| **Embed a browser engine or webview** to get "full" HTML/CSS and escape renderer bugs | Rejected | It abandons the project's reason to exist (small, no browser runtime, no JS, native a11y owned by the host) and replaces known, fixable bugs with a much larger dependency surface. |
| **Move the backend in-process** (Python/PowerShell embedding the renderer via a C ABI) | Rejected | Loses the fault boundary, the responsiveness guarantee and language neutrality; reintroduces GIL/runspace threading problems on the GUI thread. The IPC design is sound; only its event contract needs work. |
| **Synchronous `preventDefault` from the backend** | Rejected | Would make the GUI thread wait on IPC and user code; the declarative rule system is the correct trade-off (INTERACT §5). |
| **Merge PR #3 as is** | Rejected | Trades a loud failure for silent loss of lifecycle and input events while leaving responses on the disconnecting path (HOST-02). |
| **Make the repository public as an engineering fix** | Not recommended by this audit | It would enable free branch protection and fix the 404 links, but visibility is an ownership, IP and licensing decision; the options are laid out in roadmap 6.2. |
| **Fix F-02 by dropping events in the host** (PR #3's approach, or the full lanes/sequence/loss-marker design now) | Rejected for 1.0.x | Removing the 128-message count cap while keeping the existing 8 MiB byte cap and the 10 s write deadline stops the failures without dropping anything (roadmap 1.2). Type-level subscription follows in 1.1; lanes, sequence numbers and loss markers wait for a measured need. |
| **Auto-allow close/quit on decision timeout** | Rejected | Data-loss risk; Ctrl+Shift+Q is the right escape (HOST §5). |
| **Fix issue #2 by shrinking cache bounds** | Rejected | The caches are correctly bounded and are not the cause (RENDER-01). |
| **Convert pixel-probe renderer tests to golden images** | Rejected | Probes are specific and diagnosable; goldens would be brittle and opaque. |
| **Rewrite history to split the large import commits** | Rejected | Tags, PyPI provenance and Sigstore attestations reference these SHAs. Document the imports instead. |
| **A cp310–cp314 wheel matrix** | Rejected | The host is a standalone executable with no CPython ABI; test interpreter versions instead (RELEASE §5). |
| **An MSRV CI job** | Rejected | All crates are unpublished and the toolchain is pinned; an MSRV job would test a configuration nobody builds. |
| **Lower the manylinux baseline** | Rejected | A Wayland-only host gains nothing from older glibc; current linkage is already minimal. |
| **Remove `catch_unwind` because it cannot catch stack overflow** | Rejected | It correctly contains panics; stack overflow needs a structural fix, not removal of panic isolation. |
| **Replace the dual-role direct start** | Rejected | Surprising but documented and taught; alternatives (separate launcher file, in-process host) are worse for the target users. Harden its edges instead (F-26). |
| **Broad `miri` coverage** | Rejected | Stylo, FFI and platform code make it impractical; target only small pure modules if at all. |

## Postponed

| Considered | Decision | Revisit when |
| --- | --- | --- |
| **GPU rendering** | Postponed indefinitely | CPU raster costs 13–19 ms at 4K today; damage-limited present (8.3) comes first. Only if measured frame budgets are missed on supported hardware after that. |
| **Multi-threaded raster or runtime SIMD** | Postponed | Determinism is valuable for tests; if added, opt-in outside qualification and golden runs. |
| **Full O(dirty) incremental pipeline** | Postponed until measured need | The two cheap fixes (cache touch on replay, memoised family resolution) remove the cliffs; the rest is large and only matters for documents with thousands of rows. Add work counters first (T13). |
| **Explicit work-stack conversion of all recursive stages** | Postponed (8.5) | After stack sizing and the depth-survival test exist, convert only the stages the test shows are tight. |
| **Upgrading winit to a newer beta** | Postponed | Four patched platform crates must be re-validated natively each time; wait for a stable 0.31. |
| **Upstreaming the Taffy subgrid work** | Postponed to the next Taffy upgrade | Valuable but not urgent; freeze further fork growth meanwhile. |
| **Code signing (Authenticode) for the Windows host and module** | Postponed until a certificate is available | Desirable for enterprise policy (AppLocker/WDAC rules commonly block unsigned executables in user-writable paths, where venvs and CurrentUser modules live); SmartScreen matters little for pip/PSGallery installs, which set no Mark-of-the-Web. Provenance attestations and SHA pinning come first. |
| **Coverage-guided fuzzing with `cargo-fuzz`** | Postponed | Run at a 1 MiB worker stack, the existing seeded HTML harness would very likely have found F-01 (it already generates nesting bombs to depth 600). F-03 and F-06 need new generators (image-SVG `<g>`/`clip-path` chains, deep move chains), which are cheaper to add to the existing harnesses than a libFuzzer setup. Add libFuzzer targets for pure byte→result functions (WOFF2, data URLs, batch decode, SVG pre-scan) later. |
| **A Python `asyncio` API** | Postponed | The thread-based model works and is simpler for the target users; add only on demand. |
| **macOS / X11 / ARM64 support** | Postponed | Out of documented scope; each would add a qualification burden that the current process cannot carry yet. |
