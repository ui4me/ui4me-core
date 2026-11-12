# UI4ME Core 1.0 candidate

Build native desktop applications from **local HTML, CSS and Python**. Rust owns
the native window, application lifecycle, rendering, editing and OS services.
Python owns application logic in a supervised process. No browser, JavaScript,
HTTP server, Qt runtime or embedded Python interpreter is included in the host.

This repository is **1.0.0-rc.1**, the implementation candidate for the finite
completion scope in [CORE_1.0_SPEC](docs/CORE_1.0_SPEC.md). It builds on the
supplied 0.2 code and preserves the existing native fixes. Both Linux and Windows
code, the Python API, examples, tests, audit dispositions and autonomous local
completion prompt are included.

**Native qualification is not claimed by this source ZIP.** The previous
Windows UI Automation selection stall requires controlled diagnosis on Windows.
See [STATUS](docs/STATUS.md) for exact preparation evidence and remaining gates.

## Start here

1. Extract into the existing repository, preserving its Git directory, local
   configuration and history.
2. Give the local agent [LOCAL_FINISH_PROMPT.md](LOCAL_FINISH_PROMPT.md).
3. It qualifies Python/Linux KDE first and commits and pushes, then Windows
   Server 2022 and commits and pushes, then performs final common-source
   stabilization and the final commit and push.

The prompt authorizes debugging and fixes without handing feature development
back to the user. It cannot turn unexecuted scenarios into passes. The public
1.0 version is set after release gates, not merely after unzipping.

## Build and run

Prerequisites are Python 3.11+, the Rust toolchain pinned in
`rust-toolchain.toml`, and native build tools. The scripts pin the selected
Python interpreter in generated application manifests.

### Debian / KDE Plasma, native Wayland

```sh
bash tools/bootstrap_linux.sh --install-system
python3 tools/run_renderer.py
```

Omit `--install-system` if prerequisites are installed. Run as the logged-in
user with that desktop's D-Bus and portal services. Linux supervision uses a
systemd user service. There is no automatic X11 fallback.

### Windows Server 2022, Desktop Experience

From PowerShell in the normal test user's interactive RDP session:

```powershell
powershell -ExecutionPolicy Bypass -File tools/bootstrap_windows.ps1
python tools/run_renderer.py
```

Administrative provisioning is separate from normal-user application execution.
SSH can launch tasks into an existing interactive session using the supplied
helpers. An SSH logon alone does not qualify desktop behavior.

### Install a standalone app

Choose a **new or empty** application destination:

```sh
python3 tools/install.py --example-dir ~/.local/share/ui4me-core1-reference
python3 tools/create_launcher.py install --app ~/.local/share/ui4me-core1-reference/app.toml
ui4me-host run --app ~/.local/share/ui4me-core1-reference/app.toml
```

PowerShell equivalent:

```powershell
python tools/install.py --example-dir "$env:LOCALAPPDATA\ui4me-core1-reference"
python tools/create_launcher.py install --app "$env:LOCALAPPDATA\ui4me-core1-reference\app.toml"
ui4me-host run --app "$env:LOCALAPPDATA\ui4me-core1-reference\app.toml"
```

The installer builds the release host and copies the complete HTML/CSS/Python
example. This package is complete source, not prebuilt platform binaries or an
offline mirror of every dependency. Setup downloads pinned dependencies.

## Application code

```python
from pathlib import Path
from application import Application

with Application.from_environment() as app:
    window = app.open_file(Path(__file__).with_name("index.html"),
                           title="My app", width=900, height=600,
                           min_width=320, min_height=240)
    doc = window.document
    doc.on("click", lambda _: doc.set_text("status", "Saved"), element="save")
    app.run()
```

Copy `client.py` and `application.py` alongside application logic and launch it
through an app manifest. The reference app also uses `completion_checks.py`
for automated verification; the installer copies it.

The facade covers editing/selection/undo, forms/validity, stable node handles,
stylesheets, scrolling, dialogs/popovers, resources and OS services.
`Document.command()` exposes every typed operation. See
[Python examples](examples/python/README.md), the
[document protocol](docs/PROTOCOL_DOCUMENTS.md), and
[Core 1.0 additions](docs/CORE_1.0_PROTOCOL.md).

## What this completion adds

- Anonymous node queries and atomic mutations with stale-generation protection.
- Focus/blur/hit-testing, UTF-16 selection/range editing and edit-menu commands.
- Ordered form data, reset/request-submit, custom validity and selected-file labels.
- More useful events, opt-in incoming drop policy, Unicode segmentation and
  improved accessibility semantics and assistive scrolling.
- Window constraints, observed state, attention, desktop preferences, IME hints,
  parent-aware handoff and manifest accessibility identity.
- Selected CSS/text fixes, bitmap glyphs, optional hinting/precision diagnostics,
  bounded resource bundles and cache statistics/purge.
- A static idle reference app, Python helpers, new unit/IPC fixtures, C01–C12
  completion scenarios, and independent Windows UIA diagnostics.

Existing controls, complex shaping, flex/grid/table layout, local images, static
SVG, CPU effects, multiple windows, native clipboard/file pickers, supervision,
launchers and failure recovery remain.

## Defined limits

This is a desktop HTML/CSS subset. It does not claim complete browser parity.
All 125 audit rows have dispositions in [CAPABILITY_SCOPE](docs/CAPABILITY_SCOPE.md),
including excluded extensions and application-layer responsibilities. They are
not an endless future-version plan.

Linux and Windows share a contract with honest availability. The current
Wayland backend cannot implement window visibility/focus requests as requested,
so unsupported operations return errors and unknown observations stay null.
Attention depends on compositor support. Windows field-purpose IME hints are
unavailable through the pinned backend. High contrast is observable without a
complete forced-colors CSS palette.

Printing/PDF, browser networking/JavaScript, full SVG text/cascade, animated
images, rich editing, native menus/tray/global shortcuts, media and a component
framework are outside this core. macOS, X11 and physical-hardware qualification
are outside the current tested environments.

## Verification

```sh
python3 tools/verify.py --jobs 2
cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings
```

Perform the installed-release desktop scenarios in
[CORE_1.0_ACCEPTANCE](docs/CORE_1.0_ACCEPTANCE.md) using host N01–N15, renderer
R01–R12 and completion C01–C12 ledgers. Build/headless results cannot replace
desktop observations. Historical reports remain in `docs/verification/` and
`BASELINE_0.1_STATUS.md` / `BASELINE_0.2_STATUS.md`.

Optional glyph hinting: `UI4ME_GLYPH_HINTING=1`. Higher precision diagnostics
require the `diagnostic-raster-quality` Cargo feature and
`UI4ME_RASTER_QUALITY=high`; default builds keep the normal CPU path.

## Dependencies and provenance

The lockfile pins dependencies. Source patches remain under `third_party/` and
`patches/` with license and change records. See
[THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES.md),
[third_party/README](third_party/README.md), and
[source provenance](docs/SOURCE_PROVENANCE.json).
Preserve patched dependencies during qualification.
`tools/verify_package.py` checks the source manifest before local changes.
