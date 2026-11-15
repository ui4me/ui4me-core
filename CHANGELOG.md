# Changelog

All notable changes to UI4ME Core: the `ui4me-core` Python package (PyPI), the
`UI4ME.Core` PowerShell module (PowerShell Gallery) and the native host they
bundle. Versions follow [Semantic Versioning](https://semver.org/); what the
version number promises is defined in [docs/release/versioning.md](docs/release/versioning.md).
Detailed notes for each release are in `docs/release/RELEASE_NOTES_<version>.md`.

## [Unreleased]

Nothing yet.

## [1.0.2] - unreleased (release candidate)

A stabilization release from the engineering audit of 1.0.1: no new features and
no protocol change. Full notes, including what did not land:
[RELEASE_NOTES_1.0.2.md](docs/release/RELEASE_NOTES_1.0.2.md).

### Status

- The PyPI classifier changes from `Development Status :: 5 - Production/Stable` to
  `Development Status :: 4 - Beta`, and the PowerShell Gallery description says
  "beta", until the stable-release gates in the engineering audit pass. The API
  does not change.

### Fixed

- **Windows: deeply nested documents and SVG images no longer crash the host**
  (F-01, F-03, F-06). 1.0.1 overflowed the 1 MiB main-thread stack at about 40
  nested `<div>`s, and an image SVG with 700 nested groups overflowed the asset
  worker. The engine thread now has 32 MiB, layout uses about 11 KiB per level
  instead of 23–35 KiB, and SVG nesting, reference chains and expansion are bounded.
  `move_element` and `insert_html` can no longer bypass the depth limit.
- **A backend that reads its events is no longer disconnected by bursts** (F-02).
  The host's outbound queue is bounded by 8 MiB instead of 128 messages, and each
  frame is one write. A colour-scheme switch over 100 animated controls used to
  disconnect the app.
- **The bindings never drop control notifications** (F-29). Window `closed`,
  `document.error` and `app.shutdown` are always delivered. When callbacks fall
  behind, pointer, scroll and animation events are dropped first, and every drop is
  counted (`dropped_by_kind` in Python, `DroppedCoalescible`/`DroppedDiscrete`/
  `DroppedControl` in PowerShell). Events nobody subscribed to are filtered on arrival.
- **Memory no longer grows with repeated font use** (F-04, issue #2). A windowed
  replay grew from 58 to 296 MiB in 15 minutes; it now stays at 44–55 MiB.
- **Long lists stay fast** (F-16a). Appending to a long list after unrelated updates
  took 691–750 ms per append; it now takes 54–55 ms. A cold first render of the
  audit document went from 828–930 ms to 273–334 ms.
- **Content can no longer stall the GUI thread or fail a frame** (F-14, F-15):
  - nested inline-blocks and buttons are laid out in polynomial time (26 levels:
    7.7 s to 0.2 s);
  - deep unclosed markup is refused quickly (40,000 `<div>`s: 5 s to 2 ms);
  - paragraphs of many inline inputs are broken in linear time (4,000 inputs:
    17 s to 0.95 s);
  - SVG and CSS filters have a work budget (an `feTurbulence` image: 47 s to 0.2 s);
  - over-budget gradients, filter chains, masks and nesting are drawn degraded
    with a `BUDGET_EXCEEDED` diagnostic instead of failing the document.
- **Keyboard and accessibility** (F-07, F-08, F-17, F-17b):
  - focus moved by Tab, by the backend or by assistive technology scrolls the
    element into view;
  - Enter on a focused link activates it (`click`, then `link_activated`);
  - editing and clipboard shortcuts work on non-Latin layouts (by physical key);
  - a click whose press and release land on different parts of one control (an
    icon and the button's padding) clicks the control;
  - caret, IME, selection and accessibility geometry follow CSS transforms and
    `position: fixed` ancestors;
  - the accessibility tree is no longer rebuilt on every repaint.
- **Headless mode behaves like a window** (F-12): transitions and animations run
  on a virtual clock (`test.advance_time`), and frames are produced without a read.
- **Host lifecycle** (F-25): SIGTERM, SIGINT, SIGHUP, Ctrl+Break and session end
  start an orderly shutdown with `app.shutdown`. An accept error no longer ends the
  listener. A relaunch during shutdown waits and takes over. Message processing is
  time-budgeted (8 ms per turn), and slow commands are logged.
- **Direct start** (F-13, F-26, F-28; BIND-*, BIND-PS-*):
  - Python callbacks are isolated and print full tracebacks; `sys.exit()` in a
    callback quits the app;
  - an unencodable result is answered with an error instead of no answer;
  - a second `Application.start()` raises;
  - pip launcher shims are not mistaken for the host;
  - the launcher survives Ctrl+C/Ctrl+Break and exits 130 after the app's
    `on_shutdown`;
  - PowerShell: reserved member names (`psbase`, …) no longer kill the
    connection, `break`/`continue` in callbacks are contained, dialogs keep no
    15-second deadline, `Start-UI4MEScript` identifies the right script, and
    Ctrl+Break no longer opens the debugger.
- **Linux** (F-18, F-19): exported shell functions and non-UTF-8 environment values
  no longer stop the launch. An AllUsers PowerShell install works for non-root
  users through a hash-verified per-user copy of the host.
- `ui4me_core` ships `py.typed`.

### Changed

- PowerShell policy callbacks must output an explicit `[bool]`; anything else
  vetoes, with a warning (1.0.1 converted with `[bool]`, so `'false'` allowed).
- Python: a callback exception or a missing `return` in a close/quit policy keeps
  the window open, with a warning.
- Event order changes caused by the interaction fixes are listed in the
  [protocol reference](docs/reference/protocol.md).

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
  installs, clippy even when tests fail, workflow linting and `cargo deny`. A
  nightly job runs the ignored and fuzz tests, verifies the vendored patch
  stacks and runs a windowed smoke test under a headless compositor. These
  workflows have not yet run on GitHub.
- All GitHub Actions are pinned by commit SHA; release tools are hash-pinned;
  host SBOMs (CycloneDX) and build-provenance attestations accompany releases.
- The release version has a single source (`Cargo.toml`), read by
  `tools/release/version.py`; `tools/release/bump_version.py` updates the few files
  whose format requires a literal.
- New: this changelog, [SECURITY.md](SECURITY.md) and the
  [versioning and support policy](docs/release/versioning.md).

### Known open issues

See [RELEASE_NOTES_1.0.2.md](docs/release/RELEASE_NOTES_1.0.2.md#known-open-issues).

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

[Unreleased]: https://github.com/ui4me/ui4me-core/compare/v1.0.2...HEAD
[1.0.2]: https://github.com/ui4me/ui4me-core/releases/tag/v1.0.2
[1.0.1]: https://github.com/ui4me/ui4me-core/releases/tag/v1.0.1
[1.0.0]: https://github.com/ui4me/ui4me-core/releases/tag/v1.0.0
