# UI4ME Core 0.2

Build native desktop applications from **HTML, CSS and a separate language
backend**. Rust owns the native event loop, windows, rendering, input and desktop
services. Python is the supplied reference backend; it owns application logic.
There is no embedded browser, JavaScript runtime, Qt, HTTP server or Python
interpreter in the host.

This version combines the locally qualified 0.1 host with the existing UI4ME
HTML/CSS engine. It adds a typed document protocol, a small Python facade and a
complete application example. **Source delivery is not desktop qualification.**
See [STATUS](docs/STATUS.md) for checks performed during preparation and the
remaining Linux/Windows release gate. The 0.1 desktop results are preserved as
historical evidence and are not claimed as 0.2 results.

**Targets:** Debian/KDE Plasma on native Wayland, and Windows Server 2022
Desktop Experience in a normal user's interactive RDP session. macOS, X11 and
physical Windows hardware qualification remain later work.

## Build and run

Prerequisites: Python 3.11+, the Rust toolchain pinned in
`rust-toolchain.toml`, and native build tools. The supplied scripts use the
selected Python interpreter, including an absolute path in installed manifests.

### Debian / KDE Wayland

```sh
bash tools/bootstrap_linux.sh --install-system
python3 tools/run_renderer.py
```

Omit `--install-system` when build prerequisites are already installed.
Applications run as the logged-in user, contained in a systemd user service.
Use the session's D-Bus and KDE desktop portals. There is no automatic X11 or
unsupervised fallback.

### Windows Server 2022

From PowerShell in the test user's interactive desktop:

```powershell
powershell -ExecutionPolicy Bypass -File tools/bootstrap_windows.ps1
python tools/run_renderer.py
```

If build tools are missing, run the bootstrap separately with `-InstallBuildTools`
from an administrative provisioning session.
The installed application runs as a normal user. SSH alone does not qualify
GUI behavior; use the supplied interactive-session helpers and observe RDP.

### Install a standalone example

Linux:

```sh
python3 tools/install.py --example-dir ~/.local/share/ui4me-renderer
python3 tools/create_launcher.py install --app ~/.local/share/ui4me-renderer/app.toml
ui4me-host run --app ~/.local/share/ui4me-renderer/app.toml
```

Windows PowerShell:

```powershell
python tools/install.py --example-dir "$env:LOCALAPPDATA\ui4me-renderer"
python tools/create_launcher.py install --app "$env:LOCALAPPDATA\ui4me-renderer\app.toml"
ui4me-host run --app "$env:LOCALAPPDATA\ui4me-renderer\app.toml"
```

`tools/install.py` installs the release host through Cargo and copies the
HTML/CSS/Python/assets into the chosen application directory. Cargo dependencies
and build tools require an initial download; this ZIP is complete project source,
not a prebuilt installer or an offline mirror of all upstream dependencies.

## What 0.2 includes

- HTML parsing, a retained DOM, selector queries and generation-checked nodes.
- The migrated screen CSS pipeline: cascade, inherited styles, block/flex/grid/
  table layout, fonts, overflow, transforms, supported transitions and animations.
- CPU painting, Unicode shaping and fallback, local raster images, local fonts
  and static SVG. The precise implemented CSS table is queryable at runtime.
- Inputs, textareas, buttons, checkboxes/radios, selects, range controls, file
  inputs, validation, focus/Tab traversal, selection, scrolling and ordinary
  document dialogs/popovers supported by the engine.
- Native keyboard/pointer input, IME, plain-text clipboard, file pickers,
  accessibility trees/actions and incoming OS file/text drops.
- DOM/style updates over private JSON-RPC, transactional mutation batches,
  asynchronous event notifications and pre-registered default-action rules.
- Multiple native windows/documents; independent Python app logic; the qualified
  host supervision, lifecycle, native dialog and launcher behavior.
- A complete two-window application and both automated and real-desktop
  acceptance tooling.

This is a **defined desktop HTML/CSS subset**, not a claim that every web page
renders identically to Chromium. Browser reference fixtures compare shared
HTML/CSS where semantics overlap. Printing/PDF/pagination, rich contenteditable,
HTML clipboard, advanced SVG shape interaction, browser JavaScript/networking,
GPU rendering and extra desktop services are not public 0.2 capabilities.

## Minimal application logic

Keep `client.py` and `application.py` from `examples/python` alongside your
backend. A host-launched backend can use:

```python
from pathlib import Path
from application import Application

with Application.from_environment() as app:
    window = app.open_file(Path(__file__).parent / "index.html", title="My app")
    document = window.document
    document.on("click", lambda event: document.set_text("status", "Clicked"),
                element="save")
    app.run()
```

An `index.html` with a `save` button and a `status` element provides the UI.
Use a manifest like `examples/python/renderer-app.toml`; it names the backend
executable, working directory and application identity. The host launches the
backend. Running the Python file directly does not create a native event loop.

Native editing and drawing remain responsive while Python handles callbacks.
Callbacks can issue RPCs because the transport reader is independent. To
suppress a supported default action, register `document.prevent_default(...)`
in advance. A callback arriving after input cannot retroactively prevent it.

## Verification and diagnostics

```sh
python3 tools/verify.py
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 tools/run_renderer.py --headless
ui4me-host render --html examples/python/ui/index.html --output verification/screen.png
ui4me-host doctor
```

The PNG command uses a CSS viewport of 900×600 by default; `--scale` controls
physical output pixels. It waits for local resources with a bounded deadline
and reports diagnostics. It requires no display or backend. It is not a print
or export service in the application protocol.

The existing `ui4me-host probe` and `python3 tools/run_example.py` remain host
isolation tools. New native qualification must rerun N01–N15 plus the renderer
scenarios in [RENDERER_ACCEPTANCE](docs/RENDERER_ACCEPTANCE.md), on both targets.

## Repository map

| Path | Responsibility |
| --- | --- |
| `crates/ui4me-host` | Process/native ownership, typed protocol, document integration |
| `crates/ui4me-document` | HTML tree, parsing, control and DOM semantics |
| `crates/ui4me-renderer` | Style, layout, text, CPU paint, interaction and semantics |
| `crates/ui4me-assets` | Bounded local image/font/SVG resources |
| `crates/ui4me-engine-types` | Internal identities and shared engine contracts |
| `examples/python` | Transport client, small application facade and runnable examples |
| `tools` | Build/install, verification, browser references and lab helpers |
| `patches`, `third_party` | Pinned upstream source patches with provenance |

All four engine crates are internal, unpublished packages. There is one native
application host, not an alternative renderer-owned runtime or a second OS
integration layer.

Read [architecture and scope](docs/SPEC.md),
[host protocol](docs/PROTOCOL.md),
[document protocol](docs/PROTOCOL_DOCUMENTS.md),
[tooling](tools/README.md),
[verification status](docs/STATUS.md), and
[third-party notices](THIRD_PARTY_NOTICES.md).
