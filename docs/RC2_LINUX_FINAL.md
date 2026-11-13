# UI4ME Core 1.0.0 — Linux final scope and source-delivery status

**Product version:** `1.0.0`, not an RC. **Release status (2026-11-13): Linux release gates PASSED** on the installed release host, sdist and repaired manylinux wheel — see [STATUS.md](STATUS.md) for identities, results, the defects fixed during qualification and the remaining limits. This document records the supplied scope; it is not itself evidence.

The source baseline is the user's Linux-validated `ca348d9` repository archive. Its prior 39/39 native scenarios and 1,085 Rust tests are **historical results for that source**. The RC2 changes invalidate the affected performance, resource, input, accessibility and lifecycle evidence. The source ZIP contains no Git metadata; use the local Git checkout to preserve commit history. Windows implementation remains present but **Windows native qualification is explicitly deferred**, with no Windows work in this stage.

## Delivered source changes

1. **Viewport/theme updates preserve font and style services**. Ordinary environment changes now call the pinned Stylo `set_device` and update the text device scale in place. Real font source changes retain a separate cold-rebuild flag.
2. **Bundle-diff classification** distinguishes stylesheet, font, image and conservative unknown-path changes. Image-only changes no longer trigger stylesheet reload. Same-URL font changes invalidate loaded faces and re-request them.
3. **Resource paint-only stage** connects `AppliedBatch.paint_only` and `intrinsic_size` to separate scene versus layout invalidation. Revision-aware decoded raster pruning from the qualified baseline stays in place.
4. **Accessibility** reuses the traversal-built ID index for ARIA relations; host refreshes avoid constructing the tree again immediately after a prior semantic update.
5. **Native custom titlebars** expose a `decorations` window property, immediate press-time dragging, 8 native resize edges/corners and double-click maximize/restore; interactive HTML controls exclude dragging. Right/middle click regressions are extended.
6. **Vello CPU resize** reuses renderer contexts with `reset_and_resize` where supported by the pinned dependency.
7. **Live raw bitmap resource**: `<img src="ui4me-bitmap://ID">`; `bitmap.create/present/close`; host-owned private file with two or three slots; RGBA8 sRGB, tightly packed stride `width*4`, 1..4096 per axis, 64 MiB per document, eight bitmap surfaces; monotonic sequence; Python mapping/producer API; document/window cleanup. Frame contents are **not** JSON or HTTP. Frame uploads without geometry change request scene/paint, not CSS/layout. The application can discard an update via `try_present` when another update is in progress.
8. **Python wheel**: `pyproject.toml`, ABI-independent Python client plus a packaged standalone Rust ELF host, platform-specific wheel metadata, native binary version check, complete source distribution, and an explicit manylinux build workflow. No Python C-extension ABI is introduced. Publication remains manually authorized.
9. **Examples/tests**: native custom-titlebar + bitmap demo; isolated Python slot/lifecycle tests; additional Rust image/bitmap regression tests; final-Linux acceptance instructions.

## Non-negotiable Linux gate

The source must compile under pinned Rust 1.99.1. The following must pass on Debian 13 / KDE Plasma 6.3 native Wayland, using **installed release binaries/wheels as the normal user**:

- `cargo fmt --all -- --check`, all-target Clippy `-D warnings`, unfiltered full Rust tests including real Unix sockets and processes, Python tests and clean source integrity.
- N01–N15, R01–R12, C01–C12 from the current qualification plus focused **custom title bar, pointer button, live bitmap, image replacement, font replacement, resize and leak** checks.
- Independent screen-reader/IME/clipboard exercises, window lifecycle, scaling, native dialogs and backend failure.
- Linux wheel created by the supported build workflow; install it into a fresh Python virtual environment; assert executable ELF and `ui4me-host --version`; test a separately installed consumer (not just the source checkout).
- Sustained image replacement and live-frame loops with bounded memory, CPU and presentation latency; all resize regressions on final executable; no covered-window freezes or event storms.
- Source/binary hash linked acceptance evidence. Record platform, build profile, test command, pass/fail and relevant measurements. No automatic status promotion by a successful compilation alone.

## Environment limitations of this preparation

This preparation environment has Python and file editing, but no Rust toolchain or working download path to acquire it. Python tests and static/source package integrity can be run; **Rust compilation, actual Linux ELF wheel production, native Wayland tests, manylinux repair, and auditwheel verification must be done by the local finishing agent**. Those checks are required, not waived.

## Deferred and excluded

- Windows verification/port-specific cleanup until Linux is locked and released.
- Printing/PDF, webview/JavaScript, rich contenteditable, GPU, advanced interactive SVG, broad components and application widgets.
- New UI frameworks, platform-native macOS/X11 testing, speculative performance rewrites.
- Publications to PyPI/GitHub Release without explicit user authorization. Building and validating artifacts is authorized; uploading/publishing requires the user's later decision.

## Wheel packaging model

`ui4me_core` contains only Python client code and a packaged `ui4me-host` ELF executable. It does **not** bundle Python itself, and the user still supplies their application environment. `ui4me-host` and `ui4me` Python console scripts invoke that native executable. `tools/release/build_linux.sh` builds a local platform wheel and source distribution. `.github/workflows/release-linux.yml` produces a repaired manylinux wheel as a manually downloadable CI artifact (not a publication). The local agent must confirm the manylinux build is installable and meets target glibc constraints before recommending public release.
