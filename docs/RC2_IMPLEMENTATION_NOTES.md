# UI4ME Core 1.0.0 — review resolution and remaining verification

This is the last planned cross-cutting Linux core feature batch. The attached original (reported `ca348d9`) is the baseline. Source code was changed to address the architectural findings, not merely relabeled for final release.

| Review topic | Implemented change / verification target |
| --- | --- |
| Viewport rebuild | Stylo `set_device` and `TextServices::set_device_scale` in-place for normal environment changes; actual font sources still trigger a cold font rebuild. Compare against cold-render oracle. |
| Bundle image invalidation | MIME/extension-aware registry diff; stylesheet/font/image changes separated. `AppliedBatch.paint_only` triggers new `RESOURCE_PIXELS` scene/paint stage; dimension changes use layout. |
| Bundle font stale bytes | Changed URL source invalidates `@font-face` state and current requests; add same-URL font replacement native regression. |
| Displayed image retention | Keep RC1 decoded-cache pruning; rerun 500–1,000 displayed revisions after the change and profile any remaining 17–19 KiB revision growth. Do not claim it is fixed without measurement. |
| Accessibility | Reuse current ID map during ARIA relationship computation; avoid immediately rebuilding the tree twice for the same committed semantic state. Run real Orca on installed build. |
| Resize frame context | Preserve existing Vello CPU context and use `reset_and_resize` on size change, reducing reallocation. Compare native resize metrics, including when the window is covered. |
| Native window chrome | `decorations` flag, `data-ui4me-drag`, `data-ui4me-resize`, native gesture-time drag/resize and maximize on double click. Native controls remain ordinary HTML; eight directions. |
| Right and middle buttons | Direct native-mapped auxiliary button regression plus existing `contextmenu` semantics. |
| Live bitmap | `ui4me-bitmap://` resource source, private bounded host file, RGBA8 slots and `bitmap.create/present/close` IPC. Python `LiveBitmap` producer and native demo. No PNG/compression or raw pixel JSON. |
| Wheel distribution | ABI-independent Python client with native executable in platform-specific wheel. Standalone source distribution, local build script, `cibuildwheel` manylinux CI and artifact validator. No publication credentials. |

## Reasons not to introduce more core features

The underlying Taffy/Stylo/bidi/layout/paint implementation is already complex for functional reasons. No broad rewrite, GPU, browser, JS or rich-document editor was added. The native host keeps process and OS ownership; Python remains a separate process.

## Known verification-sensitive aspects

- **This cloud package has not been Rust-compiled or KDE-tested** due unavailable toolchain/downloads. The complete source implementation is supplied, and any discovered Rust compiler/runtime defects belong to local debugging.
- Native frame presentation and RAF-like producer latency remain measurement-sensitive. The bitmap API acknowledges after host snapshots the selected slot, not after compositor presentation. This distinction is intentional.
- Live bitmap has no ABI-stable shared-memory protocol outside the documented local host. The mmap file is created by the host in its private runtime directory; there is a document-level memory cap and monotonic sequences; a frame in use must not be overwritten by Python. See `examples/python/live_bitmap.py`.
- The source still retains existing optional paged/render-edit internals from the donor rather than risking a last-minute layout regression. These are not exposed by the hosted public API. The future roadmap can address cleanup if actual maintenance costs justify it.
- Wayland compositor focus/visibility restrictions and old KDE folder-dialog behavior remain documented native limitations. Do not misreport unsupported actions as successful.
- Real `1.0.0` Linux publication readiness is an **outcome of the local final gates**, not an assertion in this ZIP. No Windows validation now; the Windows adapter code stays in the repository for later work.
