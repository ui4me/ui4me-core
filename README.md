# UI4ME Core 1.0.0 — Native HTML/CSS desktop applications

Python programs control normal local HTML/CSS desktop windows through a separate
Rust-owned native host. **Release 1.0.0 is qualified for Linux: Debian 13, KDE
Plasma 6.3, native Wayland**, as an installed release binary and as a Python
wheel. The qualification record, measurements and remaining limits are in
[docs/STATUS.md](docs/STATUS.md). Windows sources remain in the tree; Windows
qualification is a later, separate stage. Nothing is published automatically.

## Install the Python package (Linux x86_64)

The wheel contains the pure-Python client (`ui4me_core`) and the compiled
`ui4me-host` executable; installing it needs no Rust toolchain. It is tagged
`py3-none-manylinux_2_34_x86_64` (glibc 2.34+) and works with any Python 3.10+.

```sh
python3 -m venv ~/.venvs/ui4me && ~/.venvs/ui4me/bin/pip install ui4me_core-1.0.0-*.whl
~/.venvs/ui4me/bin/ui4me-host --version
~/.venvs/ui4me/bin/ui4me-host doctor
```

```python
from ui4me_core import Application
```

Build the sdist, a local wheel and a repaired manylinux wheel as described in
[docs/release/BUILDING_LINUX.md](docs/release/BUILDING_LINUX.md).

## Live pixels and custom title bars

* `<img src="ui4me-bitmap://NAME">` displays a live RGBA8 surface created with
  `document.create_bitmap(NAME, width, height)`; `present()`/`try_present()`
  hand over whole frames through a private, bounded, host-owned mapping (no
  JSON pixels, no image encoding). Same-size frames repaint without style or
  layout. See [examples/python/live_bitmap_demo.py](examples/python/live_bitmap_demo.py).
* `decorations=False` windows mark HTML regions with `data-ui4me-drag` (native
  move, double-click maximize/restore) and `data-ui4me-resize="n|s|e|w|ne|nw|se|sw"`
  (native resize). Interactive controls inside a drag region stay ordinary controls.

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

### Windows (deferred)

The Windows sources and adapter contracts remain in the tree. No Windows development, build or native qualification is part of the final Linux stage. Windows begins only after the Linux release is locked.

### Install a standalone app

Choose a **new or empty** application destination:

```sh
python3 tools/install.py --example-dir ~/.local/share/ui4me-core1-reference
python3 tools/create_launcher.py install --app ~/.local/share/ui4me-core1-reference/app.toml
ui4me-host run --app ~/.local/share/ui4me-core1-reference/app.toml
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

For development you may copy `client.py` and `application.py` alongside an app. The publishable Python package instead provides `from ui4me_core import Application`. Launch through an application manifest. The reference app also uses `completion_checks.py`
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
