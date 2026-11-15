# Changelog

All notable changes to UI4ME Core: the `ui4me-core` Python package (PyPI), the
`UI4ME.Core` PowerShell module (PowerShell Gallery) and the native host they
bundle. Versions follow [Semantic Versioning](https://semver.org/); what the
version number promises is defined in [docs/release/versioning.md](docs/release/versioning.md).
Detailed notes for each release are in `docs/release/RELEASE_NOTES_<version>.md`.

## [Unreleased] - 1.0.2 (stabilization)

> **Draft.** The entries below are placeholders for the stabilization work in
> progress; the release lead finalises them (wording, finding IDs, measured
> numbers) when the branches are integrated and the release is qualified.
> Remove every item that did not land.

### Status

- The PyPI classifier changes from `Development Status :: 5 - Production/Stable` to
  `Development Status :: 4 - Beta`, and the PowerShell Gallery description says
  "beta", until the stable-release gates in the engineering audit
  (`docs/audit/05-release-readiness.md`) pass. Nothing about the API changes.

### Fixed (placeholders)

- _[F-01, F-03, F-06] Deeply nested documents and image SVGs no longer abort the
  host (stack overflow): larger thread stacks (Windows main thread 32 MiB) and
  depth limits. To be finalised by the lead._
- _[F-02] A backend that reads its events is no longer disconnected by a burst
  of notifications (for example colour transitions on many controls). To be
  finalised by the lead._
- _[F-29] The bindings no longer drop control notifications (`closed`,
  `app.shutdown`) under load. To be finalised by the lead._
- _[F-04] Memory no longer grows when the same fonts are requested repeatedly
  (issue #2). To be finalised by the lead._
- _[F-19 / RELEASE-06] A PowerShell module installed for all users on Linux
  works for non-root users. To be finalised by the lead._
- _Further fixes from the interaction and bindings workstreams. To be finalised
  by the lead._

### Release engineering

- Publishing refuses a commit unless the required CI jobs succeeded for that
  exact commit (`tools/release/ci_gate.py`), and the GitHub release is created as a
  draft with every asset before the PyPI upload, then published.
- The published Windows wheel can be the exact wheel that passed native
  qualification (`tools/release/qualify_windows_rc.py`, then promote the dry run).
  The verifier requires the Windows host's 32 MiB main-thread stack reserve and
  refuses debug-build hosts in any wheel.
- CI runs on every supported Python (3.10 to 3.14), real-host Pester on
  PowerShell 7 (Linux, Windows) and Windows PowerShell 5.1, clean-machine
  installs, clippy even when tests fail, workflow linting and `cargo deny`; a
  nightly job runs the ignored and fuzz tests, verifies the vendored patch
  stacks and runs a windowed smoke test under a headless compositor.
- All GitHub Actions are pinned by commit SHA; release tools are hash-pinned;
  host SBOMs (CycloneDX) and build-provenance attestations accompany releases.
- The release version has a single source (`Cargo.toml`), read by
  `tools/release/version.py`; `tools/release/bump_version.py` updates the few files
  whose format requires a literal.
- New: this changelog, [SECURITY.md](SECURITY.md) and the
  [versioning and support policy](docs/release/versioning.md).

### Known open issues (placeholder)

- _The release notes must list the open High-severity items that did not land
  (audit register: F-07, F-08, F-17, F-16a unless fixed)._

## [1.0.1] - 2026-11-14

Apps are ordinary scripts: `python app.py`, `uv run app.py` or `pwsh -File app.ps1`
start the bundled native host themselves. Full notes:
[RELEASE_NOTES_1.0.1.md](docs/release/RELEASE_NOTES_1.0.1.md).

### Added

- `Application.start()` (Python) and `Start-UI4MEScript` (PowerShell 7.2+ and
  Windows PowerShell 5.1) for direct script startup, with lifetime binding,
  single-instance forwarding (`instance="multiple"` / `-Instance multiple` to
  opt out) and preserved arguments and exit status.
- `app.on_window_event()` and `app.on_shutdown()` lifecycle hooks (Python).
- `ui4me-new` and `New-UI4MEProject` generate directly runnable projects.
- 1:1 Python and PowerShell documentation, recipes and example apps
  (`hello`, `notes`, `counter`, `frameless`, `live-bitmap`).

### Fixed

- Windows: absolute .NET Framework backend paths (for example
  `powershell.exe`) failed with a `ServicePointManager` type-initializer error.
- Tracebacks of an exception escaping `with Application.start(...)` could be lost.
- `app.run()` could return before the last window's `closed` event was delivered.
- `Wait-UI4ME -UntilShutdown` emitted a stray `0`.
- `Application.from_environment()` raises `StartupError` instead of `KeyError`
  without a host connection.

### Notes

- The 1.0.1 notes say the release was qualified with release-candidate
  artifacts built from one sdist. For Windows this is not exact: the natively
  qualified Windows host was a build of identical sources, not the published
  binary (audit F-11). See the erratum in the release notes.

## [1.0.0] - 2026-11-13

First stable release of the native HTML/CSS desktop runtime for Python, and
(same day, built from a later commit than the `v1.0.0` tag, see audit
RELEASE-04) the first `UI4ME.Core` PowerShell module release.

- Linux x86-64 (manylinux 2.34+) and Windows x86-64 wheels; Python 3.10+;
  installing a wheel does not require Rust.
- Rust-owned windows and application lifecycle; Python logic in a separate,
  supervised process over an authenticated local protocol.
- Local HTML, CSS, system fonts, common forms, CSS variables, responsive
  layout, events, native accessibility (AT-SPI, UIA) and native dialogs.
- Custom title bars, drag and resize regions, bounded live RGBA bitmap surfaces.
- No Chromium, WebKit or JavaScript engine: a documented desktop HTML/CSS
  subset, not a web browser.
- `ui4me-new` project generator and the bundled `ui4me-python` coding-agent
  skill (`ui4me-skill`).
- Qualified on Debian 13 / KDE Plasma (Wayland) and Windows Server 2022 (RDP).

Release notes: `git show v1.0.0:docs/release/RELEASE_NOTES_1.0.0.md`.

## Before 1.0

Development milestones, not published to package registries: 0.1 (qualified on
KDE Wayland and Windows Server 2022 over RDP, 2026-11-12), 0.2 (2026-11-12) and
1.0.0-rc.1 (2026-11-13). See the Git history for details.

[Unreleased]: https://github.com/ui4me/ui4me-core/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/ui4me/ui4me-core/releases/tag/v1.0.1
[1.0.0]: https://github.com/ui4me/ui4me-core/releases/tag/v1.0.0
