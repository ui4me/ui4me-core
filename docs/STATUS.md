# UI4ME Core 1.0.0 — release status (Linux and Windows)

**Release decision (2026-11-13): READY for Linux (Debian 13 / KDE Plasma 6.3 / native Wayland)
and READY for Windows (Windows Server 2022 Desktop Experience x64, interactive RDP session).**
All gates passed on the final, unchanged source. Nothing has been published: PyPI upload and a
GitHub Release remain a separate, manual decision. macOS is out of scope.

## Final identities (one source, both platforms)

| Item | Value |
| --- | --- |
| Product version | `1.0.0` (Cargo workspace, Python metadata, examples, generated manifests) |
| Base commit | `33e1317` (Linux release) + the Windows-stage changes below |
| Ledger source identity (`tools/native_acceptance.py`, identical on both OSes) | `a089b4f5338f49417f56e95a7bef7f5cb6c8f3db7c0afba938628fac7d271d5d` |
| Source distribution (shared) | `ui4me_core-1.0.0.tar.gz`, 10,292,712 bytes, SHA-256 `70d70bd0975d9f53d134e78a92f6a2cb00e4f08dbd9322d87e1511234a7de92f` |
| Windows wheel (public candidate) | `ui4me_core-1.0.0-py3-none-win_amd64.whl`, 13,292,457 bytes, SHA-256 `0557024f5ce85cfe145b84a1afe8e89602100cb9fc22b8da4b4fef91fd33b7a6` (embedded host `7b6e1d6f…`, built from the sdist) |
| Linux wheel (public candidate) | `ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl`, 16,316,299 bytes, SHA-256 `2688e72dafa8d2a6c3262f6c5ad87b829d29498f949e93dd6dfe71a09ddcb53f` |
| Linux local wheel (not for PyPI) | `ui4me_core-1.0.0-py3-none-linux_x86_64.whl`, 16,314,662 bytes, SHA-256 `de9b2c18378c5cfc0e1ee2800644379b5766277dfac097d59bb5a8c84ea75a58` |
| Windows installed host (native evidence) | `%USERPROFILE%\.cargo\bin\ui4me-host.exe` 1.0.0, 30,219,776 bytes, SHA-256 `bc704f662345611188b650cc9db1fa696c4934943ccc153100a74c0ba2836f6d` (re-`cargo install` of the final tree reproduced it) |
| Linux installed host (final-source regression) | `~/.cargo/bin/ui4me-host` 1.0.0, 41,038,888 bytes, SHA-256 `14094b7d95489cf9e28d37aef3608b095f303f7da0739399e6ee980dc42f524b` |
| Toolchain / lockfile | Rust 1.99.1 (pinned); `Cargo.lock` unchanged since `33e1317` |
| Windows platform | Windows Server 2022 Standard Evaluation 10.0.20348, VM `omegaui`, normal user `ui4me_test` (not an administrator) in RDP session 2 (xfreerdp3, 1600x1000, 96 DPI; 144 DPI and 1280x800 exercised); real RDP keyboard/mouse; Microsoft Pinyin (zh-CN, temporary); NVDA 2026.3.1; CPython 3.13.17 system, 3.10.22/3.12.16 for wheels |

Linux and Windows artifacts come from the same tree: `tools/release/build_linux.sh` built the sdist and
the local wheel, `cibuildwheel` (manylinux_2_34) and the Windows build front end each built their
wheel **from that sdist** (`python -m build --wheel ui4me_core-1.0.0.tar.gz`, fresh target directory).

## Windows gates (final source)

* `tools/verify.py` on Windows (fmt, check, test, python, headless, renderer, faults incl. forced
  host kill): **all PASS**. Rust: **1,007 passed, 0 failed, 9 ignored** (`--no-fail-fast`; the
  Linux-only tests are cfg-gated). Python: 60 run, OK (2 platform skips).
* `cargo clippy --locked --workspace --all-targets -- -D warnings` on Windows: clean.
* Native ledgers on the installed release host, each `qualification: PASS` including its automated
  subset: host **N01–N15**, renderer **R01–R12**, completion **C01–C12** — 39/39, recorded in the
  interactive session ([evidence](verification/core-1.0-windows-final-2026-11-13/README.md)).
* RC2 features on the final binary: frameless title-bar drag, double-click maximize/restore, title
  buttons and an input in the title bar excluded from dragging, all 8 native resize directions,
  left/right/middle semantics (right/middle drags never move the window); live bitmaps ≥1,000
  frames at 64x64, 320x200 and 1024x768 with newest-frame pixels on screen, error/budget semantics
  identical to Linux, overload dropping via `try_present`, flat memory/handles, **no transport files
  left in `%TEMP%`**, document replacement and backend kill; 1,000 displayed PNG replacements
  (0 bytes/replacement), intrinsic sizes, same-URL fonts, close veto for ×, `WM_CLOSE` and Alt+F4.
* **UI Automation TextPattern (former 0.2 blocker): resolved.** Acquiring TextPattern,
  DocumentRange, GetSelection, Select and reading the selection all complete in milliseconds
  through the native IUIAutomation client in MTA and STA. The "stall" reproduces only with the
  managed `System.Windows.Automation` client, which raises `AccessViolationException` in
  `UiaCoreApi.RawTextRange_GetText` on any `GetSelection` range — identically for UI4ME/AccessKit,
  WordPad's RichEdit and Chrome — in both apartments; the original 0.2 repro now ends with
  0xC0000005 at that call instead of hanging. NVDA's real keyboard journey and UIA actions
  (SetValue, Select, Toggle, RangeValue, Invoke, ExpandCollapse, SelectionItem, ScrollItem) pass.
  An AccessKit change (SAFEARRAY element pointers) was prototyped, shown not to be causal, and
  not shipped.
* Distribution: `twine check` PASS; `tools/release/verify_windows_artifact.py` PASS (PE32+ x86-64
  console-subsystem host that releases its console on GUI launch, single tag
  `py3-none-win_amd64`, platlib root, license texts generated for `x86_64-pc-windows-msvc`, no
  local/VCS/cache/credential files). Clean installs as the normal user with Rust removed from
  PATH (`pip install --no-index --only-binary :all:`) into venvs with CPython **3.10.22, 3.12.16
  and 3.13.17**: `pip check` clean, import, `ui4me-host --version`, `doctor`, smoke test. An
  independent consumer app outside the repository (own directory, manifest naming the venv's
  `python.exe`) ran on the wheel's host (120 live frames, exit 0) and from its own Start Menu
  shortcut: Explorer → wheel host → venv Python, no console window, taskbar button with the app
  name and icon.

## Linux regression on the final source

The Windows stage changed shared code (renderer accessibility scrolling, Python live-bitmap
client, tools, packaging metadata). On the final source:

* `tools/verify.py`: all PASS; Rust **1,094 passed, 0 failed, 11 ignored** (1,093 + the new
  scroll regression test); Python 60 OK; Clippy `-D warnings` clean; `cargo deny`: no
  vulnerabilities (the same four *unmaintained* notices as before).
* sdist and both Linux wheels rebuilt from this source; `twine check`, `verify_linux_artifact.py`
  and `auditwheel show` (manylinux_2_34_x86_64) PASS; clean installs with CPython 3.10.22, 3.12.16
  and 3.13.5 without Rust.
* Native KDE Wayland checks of the changed behavior with the final installed host: AT-SPI
  `scroll_to` on a static-text leaf now scrolls (formerly a documented limitation) with the
  selection untouched; the independent consumer on the rebuilt manylinux wheel presented 120 live
  frames and exited 0 with no transport file left.
* The Linux N/R/C ledgers recorded at `33e1317` (source `02bf81e4…`) are not re-recorded: the
  changes since are covered by the gates above, and the remaining edits are Windows-only code
  paths or tooling.

## Windows measurements (installed release host)

CPU from `QueryProcessCycleTime` (exact cycles; `GetProcessTimes`, used by
`tools/measure_resources.py`, samples 15.6 ms ticks and under-counts bursty paint work).

| Measure | Windows | Linux (context) |
| --- | --- | --- |
| Startup to app ready (5 runs, median) | renderer 130 ms (reduce 132 ms); probe 105 ms | 534 ms / 127 ms (incl. systemd re-entry) |
| First frame after host start (median) | 93–94 ms | ~41 ms |
| Idle CPU | 0.08 % host (cycles); 0.00 % by GetProcessTimes | 0.25 % |
| Host + backend memory after idle | 39.7–40.1 MiB private (renderer), 29.4 MiB (probe) | 59.9 MiB PSS |
| Window open/close | probe flat; renderer 29–38 MiB oscillation, ≤1.8 KiB/cycle trend over 1,200 cycles | 24–26 KiB/cycle (AT-SPI on) |
| DOM churn (400 cycles) | flat (≤0.01 KiB/cycle) | flat |
| Displayed bundle PNG replacement (1,000) | 0 bytes/replacement, one decoded raster | ~0.39 KiB/replacement |
| Resize-to-present (≈240 sizes) | p50 2.2 ms, p95 3.5 ms, max 7.5 ms static; max ≤ 13.7 ms with animation / 60 fps bitmap | p50 2.6, p95 6.3, max 18.7 ms |
| Infinite CSS animation | 16.4 % of a core, 0.11 % after removal | ~33 % |
| Live bitmap 60 fps 320x200 | 14.3 % | ~16 % |
| Live bitmap unthrottled (ack p50) | 64x64 2.1 ms (456 fps), 320x200 2.4 ms, 1024x768 6.7 ms (146 fps); memory/handles flat | 0.064 / 0.92 ms |
| Click → Python callback (via RDP + agent) | 38–39 ms idle, 38–40 ms under unthrottled 1024x768 load | 27–31 ms |

Windows repaints each presented frame immediately (there is no compositor frame-callback
throttling as on Wayland), so an unthrottled producer is bounded by full-window raster time;
acknowledgement, newest-frame and memory semantics are unchanged.

## Defects found and fixed in the Windows stage

| # | Defect | Fix / test |
| --- | --- | --- |
| W1 | Renderer test asserted that a ligature changes pixels; with Windows' unrounded advances Noto Sans's `ffi` ligature is pixel-identical (`face_feature_default…` failed only on Windows) | Assert the shaped glyph IDs (the contract) |
| W2 | Generated reference/probe manifests said `version = "1.0.0-rc.1"` (Linux installs too) | Version read from the workspace; `tools/test_run_example.py` |
| W3 | Every adapter ScrollIntoView reached the hinted arm, which scrolled the raw DOM text node: static-text leaves never scrolled (UIA and AT-SPI; the earlier F18 fix was unreachable) | Hinted and hint-less arms scroll the text node's element; `adapter_scroll_into_view_on_static_text_scrolls_its_element` through `decode_request` (fails before, passes after) |
| W4 | winit-win32 read all drag payloads in `DragEnter`: delayed-rendering sources blocked the GUI thread during hover, large payloads were copied without a drop, and the late `Drop` hit-tested after the page changed (async target replacement retargeted) | Vendored patch: probe formats with `QueryGetData`, retain the `IDataObject`, read the requested format during `Drop` (`third_party/README.md`); native source log shows `GetData` only at release |
| W5 | `desktop.open_external` with an absolute path or `file:` URL to an `.exe`/script/shortcut **launched** it through `ShellExecuteEx` | Refuse shell-executable types (`AssocIsDangerous`) with `-32003`; Windows unit test; C11 native |
| W6 | Live-bitmap transport files leaked in `%TEMP%` (one per bitmap): the Python client's handle lacked delete sharing, so the host's unlink-after-present failed and its `TempPath` was consumed | Client opens the transport `O_TEMPORARY` (delete-on-close + delete sharing, also covers killed processes); host keeps the path for removal at close if an unlink fails; two Windows host tests + Python test |
| W7 | The wheel build refused Windows; license collection was Linux-only | `setup.py` builds `ui4me-host.exe` into a `py3-none-win_amd64` wheel (honours `CARGO_TARGET_DIR`); `collect_licenses.py --target windows`; `tools/release/verify_windows_artifact.py`, `build_windows.ps1`, non-publishing `release-windows.yml`, Windows CI leg |
| W8 | `tools/create_launcher.py` rejected UTF-8-with-BOM manifests (PowerShell 5.1 / Notepad) that the host accepts | Read `utf-8-sig`; `tools/test_create_launcher.py` |
| W9 | Ledger source identity included the git-ignored local agent files, so identical trees hashed differently per checkout | Excluded; identity test extended |

Diagnostics added: `tools/windows/uia_com_probe.py` (native COM client) and `--client com` /
`select-read` in `uia_probe_matrix.py`.

## Windows limitations (not claimed fixed)

* The managed `System.Windows.Automation` client cannot read text from a `GetSelection` range on
  this system for any provider (client defect, see above); use IUIAutomation (Narrator/NVDA do).
* AccessKit's UIA adapter has no Grid/Table/GridItem patterns (upstream TODO) and exposes
  `aria-describedby` as FullDescription, not a relation; tables are DataGrid/DataItem trees.
* The Open-With chooser for unassociated types is a separate shell process: its cancellation is
  not reported (`accepted:true, presented:null`) and parent close or quit do not close it.
* With OLE, delayed-render drop data is read synchronously inside `Drop`; the GUI waits for the
  source (no retargeting occurs).
* Windows vertical Aero Snap (dragging an edge onto the screen edge) can size a window 7 px above
  its maximum height; edge/corner drags clamp exactly.
* Microsoft Pinyin consumes Tab while composing (IME behavior). The v0.1 probe's host-local
  editor drops an uncommitted composition on a mouse focus change; the HTML renderer keeps it.
* Right/middle presses do not move focus (shared renderer rule).
* High-contrast theme switching and physical monitors/multi-monitor DPI were not exercised;
  scaling was tested through RDP desktop scale. Windows client editions and ARM64 are untested.
* Wheel-from-sdist builds must use a fresh Cargo target directory (see
  [BUILDING_WINDOWS](release/BUILDING_WINDOWS.md)).

## Final documentation and publishing preparation (2026-11-13)

Added the public README with real screenshots, the documentation guides and API reference, the `ui4me-new` project generator, the bundled `ui4me-python` Agent Skill with `ui4me-skill`, and the **manual-only** `publish.yaml` workflow. **Native Core source is unchanged** (`crates/`, `patches/`, `third_party/`, `Cargo.*`, `examples/` are identical to `4f9f4bd`), so the native N/R/C ledgers remain attached to their original identity and were not re-recorded. Python tooling did change; the affected paths were tested on both platforms.

Defects found and fixed during this preparation:

* The generated app's click handler failed at runtime because it batched `set_value`, which is not a batch operation. The documented batch whitelist is now pinned by a test against the host's `DomOp` enum.
* Generated code could break on app names containing `"""`; HTML titles were not escaped; non-Latin-only names were rejected; the printed run command was not shell-quoted.
* `ui4me-new`/`ui4me-skill` reported failure after succeeding when a redirected Windows console (cp1252) could not print a path.
* The `ui4me-host` console wrapper orphaned the native host on SIGTERM; on POSIX it now `exec`s the host. On Windows the wrapper still waits for the host; see [platforms](guides/platforms.md).
* The source manifest listed maintainer-local files; it now honours Git excludes. The README links are absolute so the PyPI page renders images and navigation.

Qualification of source commit `0cbdff0`:

| Gate | Result |
| --- | --- |
| `tools/verify.py` (fmt, check, test, python, headless, renderer, faults) | PASS (all stages at `d34d3f7`; python stage again at `0cbdff0`, the only later changes being Python) |
| Clippy `-D warnings`, `python -m unittest discover -s tools` (87 tests) | PASS |
| `refresh_source_manifest.py --check`, `verify_package.py`, `verify_source.py` | PASS (1760 files) |
| Canonical sdist from a clean clone; manylinux (cibuildwheel 4.3.5, `manylinux_2_34`) and win_amd64 wheels built **from that sdist** | PASS, `twine check`, artifact validators, `verify_publish_dist.py` |
| Clean venv install, Linux (KDE Plasma Wayland), Python 3.10 / 3.12 / 3.13 | PASS: smoke, `--version`, `doctor`, all four entry points, `ui4me-new` with active venv (manifest names the venv interpreter; Unicode/quotes/space paths; non-empty directory refused), generated window driven through AT-SPI with the Python handler updating the DOM, host stops on SIGTERM to the wrapper |
| Clean venv install, Windows Server 2022, non-admin `ui4me_test` in interactive RDP, Python 3.10 / 3.12 / 3.13 | PASS: same checks; generated window driven through native UI Automation Invoke |
| `ui4me-skill` on both platforms | PASS: dry-run writes nothing, install into all six per-user folders, status, reinstall, modified and unmanaged directories refused without `--force`, unrelated skills preserved, uninstall |
| Agent discovery (Linux) | `ui4me-python` listed by Claude Code 2.1.323, Codex CLI 0.169.0, GitHub Copilot CLI 1.0.109 and OpenCode 2.2.5. Cursor and Gemini CLI were not installed, so for those only the folder placement was verified |

Locally qualified artifacts (from `0cbdff0`):

```
ec44d0d9d02404aef243af6b70e9a62cb76a694da8d6ab2778d9d736f0690c32  ui4me_core-1.0.0.tar.gz
6592fe5038ed0b31d55f51177e5c0f979b0181885496147c52762ab701b77029  ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl
65d660cbd402a12fd15035cadbd188d4627f7656a5f4b8217a71e1859d3a143d  ui4me_core-1.0.0-py3-none-win_amd64.whl
```

The published files are built by `publish.yaml` from the commit that records this section (documentation-only difference from `0cbdff0`), so their hashes differ. sdists embed timestamps and the CI toolchains differ. The published hashes are recorded below after a successful run.

## Publication

Publishing happens only through the manually dispatched `.github/workflows/publish.yaml`
(PyPI Trusted Publishing, GitHub environment `pypi`, restricted to `main`). It rebuilds the sdist and both
wheels from one commit and uploads exactly those three files. Never upload local
artifacts by hand, and never upload the local `linux_x86_64` wheel. See
[PUBLISHING.md](release/PUBLISHING.md).

**Status: not yet published.** This line is replaced with the run URL and published hashes
only after the workflow succeeds.

Evidence: [Windows](verification/core-1.0-windows-final-2026-11-13/README.md),
[Linux](verification/core-1.0-linux-final-2026-11-13/README.md).
Historical baselines: [BASELINE_RC1_LINUX_STATUS.md](BASELINE_RC1_LINUX_STATUS.md),
[BASELINE_0.2_STATUS.md](BASELINE_0.2_STATUS.md).

## Linux qualification (stage 1, 2026-11-13, commit 33e1317)

The Linux release was qualified first; its native ledgers and evidence remain valid for the
Linux contract (see "Linux regression on the final source" above for what changed since).

### Linux stage identities (commit 33e1317, superseded artifacts)

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

### Gates (Linux stage source `02bf81e4…`)

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

### Linux measurements (installed release host, stage 1)

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

### Defects found and fixed in the Linux stage

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

### Linux limitations (not claimed fixed)

* ~~AT-SPI `Component.scroll_to` on a static-text leaf does not scroll~~ — fixed in the Windows
  stage (W3); verified natively on KDE with the final source.
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

### Linux stage artifacts (superseded)

The `33e1317` sdist/wheels listed above are superseded by the final artifacts in "Final
identities"; publish only those.

Evidence index: [verification/core-1.0-linux-final-2026-11-13/](verification/core-1.0-linux-final-2026-11-13/README.md).
Historical baseline: [BASELINE_RC1_LINUX_STATUS.md](BASELINE_RC1_LINUX_STATUS.md).
