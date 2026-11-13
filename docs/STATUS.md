# UI4ME Core 1.0.0 — Linux release status

**Release decision (2026-11-13): READY for Linux (Debian 13 / KDE Plasma 6.3 / native Wayland).**
All Linux release gates passed on the final, unchanged source. Nothing has been published:
PyPI upload and a GitHub Release remain a separate, manual decision. Windows sources are in
the tree; Windows qualification has not started and is the next stage.

## Final identities

| Item | Value |
| --- | --- |
| Product version | `1.0.0` (Cargo workspace, Python metadata, examples) |
| Base commit | `ca348d9` (RC1 Linux checkpoint) + supplied final source + the fixes below |
| Ledger source identity (`tools/native_acceptance.py`) | `02bf81e422eb060d53abd5e9235fb85f5a23d1ed806d696294e77056ab939aac` |
| Installed host | `~/.cargo/bin/ui4me-host` 1.0.0, 41,039,744 bytes, SHA-256 `c70346ba412e8348cb6cbbd9fd322128df9bec12accff10d4b26c26fd6dd3146` |
| Source distribution | `ui4me_core-1.0.0.tar.gz`, 6,919,087 bytes, SHA-256 `0654251dc76c3a0f15e911de851534ef26390ce3714bbdd9146725ed0e0d50cd` |
| Repaired wheel (public candidate) | `ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl`, 16,316,258 bytes, SHA-256 `94789932dbbff719b310b4aa78efd7c2a17b3be98b2ec4fcf1d474e570f6b402` |
| Local wheel (not for PyPI) | `ui4me_core-1.0.0-py3-none-linux_x86_64.whl`, 16,314,988 bytes, SHA-256 `80edb21c7d371655468ff5653620be4b36ded8d0887dce77d52f31f67b2625fe` |
| Toolchain | Rust 1.99.1 (pinned), Cargo.lock unchanged by this stage |
| Platform | Debian 13.7, kernel 6.12.116, KDE Plasma 6.3.6 / KWin native Wayland, normal user, physical HDMI monitor 2048x1080 (scale 1; 1.5 and 2 tested), uinput keyboard/pointer for scripted input, fcitx5 pinyin + us-intl (temporary), AT-SPI 2.56.2, Orca 48.1 |

Artifacts were built with `tools/release/build_linux.sh` (sdist, then the wheel built *from*
the sdist) and the checked-in manylinux workflow reproduced locally with `cibuildwheel` in
`quay.io/pypa/manylinux_2_34_x86_64`, again from the sdist, followed by `auditwheel repair`.

## Gates (final source)

* `tools/verify.py` — fmt, check, test, python, headless, renderer, faults: **all PASS**.
  Rust: **1,093 passed, 0 failed**, 11 pre-existing ignored harnesses, nothing filtered
  (real Unix sockets and processes). Python: 57 run, 2 Windows-only skips.
* `cargo clippy --locked --workspace --all-targets -- -D warnings`: clean.
* Native ledgers on the installed binary, each `qualification: PASS` including its automated
  subset: host **N01–N15**, renderer **R01–R12**, completion **C01–C12**.
* Final-binary RC2 scenarios: frameless title bar drag / double-click maximize-restore /
  buttons excluded from dragging, all 8 native resize directions, left/right/middle button
  semantics, live bitmap (≥1,000 frames at 64x64, 320x200 and 1024x768, newest-frame on
  screen, ordering, error and budget semantics, overload dropping, teardown), bundle image and
  same-URL font replacement, scale 1/1.5/2, close veto, resize/animation/bitmap cost.
* Distribution: `twine check` PASS, `tools/release/verify_linux_artifact.py` PASS
  (`py3-none` tag, root layout, license files, no local-only/VCS/cache/credential files),
  `auditwheel show`: consistent with `manylinux_2_34_x86_64` (only libc/libm/libgcc_s,
  max `GLIBC_2.34`). The sdist matches the tree file-for-file (1,473 files).
* Clean installs without any Rust build (`pip install --no-index --only-binary :all:`) into
  fresh venvs outside the repository with CPython 3.10.22, 3.12.16 and 3.13.5: `pip check`
  clean, `import ui4me_core`, `ui4me-host --version`, `ui4me-host doctor`, installed smoke
  test; the live-bitmap example ran on the wheel's packaged host; an independent consumer
  app (own directory, manifest naming the venv's `bin/python`) presented 120 live frames and
  exited 0.
* `cargo deny check advisories`: no vulnerabilities, unsound or yanked crates; four
  *unmaintained* notices (`ttf-parser`, `zbus_names`, `zvariant`, `zvariant_derive`). Their
  only remedy is the zbus 6 migration of the pinned AccessKit/portal stack — a maintenance
  item, not a security fix, so it was not upgraded speculatively.
* Licenses: the 380 Rust packages linked into the host are MIT/Apache-2.0/BSD/Zlib/Unicode/ISC
  family and MPL-2.0 (Servo/Stylo, source and patches shipped in the sdist); no GPL. The wheel
  carries `ui4me_core/_bin/THIRD_PARTY_LICENSES.txt` (generated from `cargo metadata`) and the
  project, font (OFL) and MPL texts in its dist-info.

## Measurements (installed release host)

| Measure | Result |
| --- | --- |
| Renderer reference ready (5 runs, median) | 534 ms (system) / 541 ms (explicit reduce); RC1 569 ms, 0.2 527 ms |
| Static idle CPU (60 s) | host 0.25 %, backend 0.29–0.48 %; caret blink adds ~0.9 % |
| Host + backend PSS after idle | 59.9 MiB (RC1 59.6 MiB); probe 33.7 MiB, ready 127 ms |
| DOM churn (400 cycles) | flat, ≤ 0.07 KiB/cycle |
| Window open/close (400 cycles) | 24–26 KiB/cycle with AT-SPI enabled, **0.1 KiB/cycle with AT-SPI disabled** (known zbus/AT-SPI residual) |
| Displayed bundle PNG replacement (1,000) | +484 KiB host PSS (~0.39 KiB/replacement; RC1 open finding was 17–19 KiB), one decoded raster throughout |
| Resize-to-present (≈240 distinct sizes) | p50 2.6 ms, p95 6.3 ms, max 18.7 ms (static); max ≤ 18.3 ms with an animation or a 60 fps bitmap |
| Infinite CSS animation | ~33 % host (full-window CPU raster), back to 0.25–0.30 % after removal |
| Live bitmap 60 fps 320x200 | ~16 % host, identical with AT-SPI on/off |
| Live bitmap throughput (ack p50) | 64x64: 0.064 ms (~11,000 fps); 1024x768: 0.92 ms (~660 fps); memory flat, threads/fds constant |
| Click→Python callback | 27–28 ms idle, 26–31 ms while an unthrottled 1024x768 producer runs |

## Defects found and fixed in this stage

Each has a regression test where it can be tested without a desktop.

| # | Defect | Fix / test |
| --- | --- | --- |
| F1 | Supplied source did not compile (missing `Stage` import; test used a private method) | Import; `pub(crate)` accessor |
| F2 | Clippy `chunks_exact_to_as_chunks` in new code | `as_chunks_mut` |
| F3 | In-place Stylo `set_device` ignored the returned origin set: media queries (reduced motion, colour scheme, width) kept stale results | `force_stylesheet_origins_dirty`; 4 existing tests that had failed |
| F4 | Web-font loads no longer rebuilt the text context (`font_context_pending` never set): @font-face features and PNG colour glyphs lost | Set it on font-epoch changes; 2 existing tests that had failed |
| F5 | Byte-identical bundle re-registration dropped decoded caches yet notified nobody | Consistent no-op; test updated to the RC2 contract |
| F6 | `<img src="ui4me-bitmap://…">` without explicit size laid out at 0x0 (layout read only the request table) | Live surfaces size layout; `recreated_live_bitmap_is_laid_out_and_painted_again` |
| F7 | Bitmap backing file leaked in `$XDG_RUNTIME_DIR` (up to 64 MiB/document) when the host was killed | Unlink after the first present; `transport_file_is_unlinked_after_the_first_present` |
| F8 | Every pixel-only present republished the whole accessibility tree | Present only requests a redraw (ack p50 0.2 → 0.064 ms for 64x64) |
| F9 | Manifest `backend.program` was canonicalized, so a venv's `bin/python` ran the base interpreter without the app's packages (e.g. `ui4me_core` from the wheel) | Spawn the named path on Unix; `symlinked_backend_program_keeps_the_named_path` |
| F10 | Packaged `ui4me_core.application` lacked the `decorations` window option | Synced; `tools/test_package_sync.py` |
| F11 | `LiveBitmap.close()` raised during host shutdown (demo exited 1) | Tolerate closed/shutting-down; Python test |
| F12 | Demo maximize button read a non-existent state key (never restored) | Use `observed.maximized` |
| F13 | Wheel tagged `cp313-cp313`, modules under `.data/purelib` | `py3-none-<platform>`, platlib root |
| F14 | sdist omitted extensionless build inputs (e.g. `SHA256SUMS`) and could not build; local build hid it | `graft` trees; wheel built from the sdist |
| F15 | Wheel shipped no third-party license texts for the statically linked host | `tools/release/collect_licenses.py`, license-files |
| F16 | sdist would include local agent files; `verify_source.py` required one | Removed; verifier rejects local-only/VCS/cache/credential files |
| F17 | Ledger source identity changed while wheels were being built (`build/`, `*.egg-info`) | Excluded build outputs; test extended |
| F18 | ScrollIntoView on a static-text accessibility object scrolled nothing | Hint-less request fixed; see limitations |

Debug-level frame timing (`RUST_LOG=ui4me_host=debug`: frame ms, resize→present ms) was added
for measurement; it is off by default.

## Remaining limitations (not claimed fixed)

* AT-SPI `Component.scroll_to` on a static-text leaf object still does not scroll: AT-SPI always
  sends a scroll hint (`ScrollIntoViewWithHint`), whose arm in `Document::accessibility_request`
  still targets the text node. Element and field text-run scrolling work. Pre-existing since
  RC1; follow-up.
* At output scale ≠ 1 AT-SPI extents are physical pixels (AccessKit adapter behaviour).
* `aria-labelledby` to a non-exposed `<span>` yields the name but no labelled-by relation;
  accesskit_unix 0.24.1 has no AT-SPI Table/TableCell interfaces (indices are attributes).
* ~24–26 KiB retained per closed window while the AT-SPI bus is enabled (none when disabled).
* A composition interrupted by a focus change stays in the field that owned it (browser
  behaviour, as the finish instructions require).
* KDE folder chooser is unparented; KDE's portal exposes no reduced-motion value; RTL resize
  grip stays physical lower-right; in native failure mode a frameless window has no title bar
  (its Quit button works).
* An empty `text/uri-list` transfer is delivered as a drop with zero paths.
* Evidence used a physical monitor but scripted uinput input devices. Alt+F4 did not reach
  windows in this session's input-method configuration (compositor close and the HTML close
  button were verified instead).

## Can the owner publish?

Yes, for Linux, if they choose to: upload exactly `ui4me_core-1.0.0.tar.gz` and
`ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl` with the hashes above (or rebuild them
through `.github/workflows/release-linux.yml` and compare). Do not upload the local
`linux_x86_64` wheel. No credentials are stored in the repository and no step publishes
automatically.

Evidence index: [verification/core-1.0-linux-final-2026-11-13/](verification/core-1.0-linux-final-2026-11-13/README.md).
Historical baseline: [BASELINE_RC1_LINUX_STATUS.md](BASELINE_RC1_LINUX_STATUS.md).
