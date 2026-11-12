# UI4ME Core 0.1

A native desktop application host with a Rust-owned event loop and a separate
application backend. This repository contains **one Cargo package,
ui4me-host**, with a library and executable. A Python example demonstrates the
language-neutral local protocol.

This is the foundation for UI4ME's future HTML/CSS application system. The
included editable CPU surface is a native integration probe. An HTML/CSS
renderer is deliberately outside this version.

**Targets:** Debian/KDE on native Wayland, and Windows Server 2022 Desktop
Experience in an interactive RDP session. X11 and macOS are outside v0.1.
Implementation and verification are separate: read [STATUS](docs/STATUS.md)
for exactly what has run and what still needs a real desktop.

## Start here

Requirements: Python 3.11 or newer, the Rust toolchain pinned by
rust-toolchain.toml, and native C/C++ build tools. Python is only required for
the example and developer tooling; the host itself does not embed Python.

### Debian / KDE Wayland

From a terminal in your logged-in desktop:

    bash tools/bootstrap_linux.sh --install-system
    python3 tools/run_example.py

The bootstrap installs build prerequisites and builds the host. Its
--install-system option uses administrative package installation; omit it when
the prerequisites are already installed. Normal application execution uses a
systemd **user** transient service for process containment. No root process runs
the application.

Your KDE desktop needs its normal session D-Bus, xdg-desktop-portal and the KDE
portal implementation. The host uses native Wayland windows and native portal
file dialogs. There is no X11 fallback.

To exercise the host without Python:

    cargo run --locked -p ui4me-host -- probe

### Windows Server 2022

Use **Desktop Experience** and a logged-in interactive desktop. Install/build
from PowerShell:

    powershell -ExecutionPolicy Bypass -File tools/bootstrap_windows.ps1 -InstallBuildTools
    python tools/run_example.py

The bootstrap can install the toolchain and MSVC Build Tools when invoked with
the required local administrative rights. An SSH shell is useful for building,
but it is not a substitute for the interactive RDP desktop when testing GUI
behavior. The supplied Windows helpers launch work in the correct interactive
user session.

## Install the executable

From either platform:

    python tools/install.py
    ui4me-host doctor

The equivalent direct command is:

    cargo install --locked --path crates/ui4me-host

The executable is installed under Cargo's normal installation root. Ensure its
bin directory is on PATH. The source package is not a prebuilt Windows installer
and does not bundle a Python interpreter.

To install a standalone copy of the reference application outside this checkout:

    python tools/install.py --example-dir /absolute/path/to/ui4me-reference

The generated manifest records the current Python interpreter's absolute path.
Use tools/create_launcher.py to register a per-user desktop entry or Windows
Start Menu shortcut; see [the tooling guide](tools/README.md) for exact commands.

## Run your own backend

Write an application manifest:

    [app]
    id = "org.example.MyApplication"
    name = "My application"
    version = "0.1.0"
    # Optional PNG, resolved relative to this manifest:
    # icon = "icon.png"

    [backend]
    program = "/absolute/path/to/python3"
    args = ["{app_dir}/application.py"]
    cwd = "{app_dir}"

    [host]
    instance = "single"
    max_windows = 16
    startup_timeout_ms = 15000
    shutdown_timeout_ms = 5000

Use an actual Python executable path on Windows, for example
C:/Users/your-user/AppData/Local/Programs/Python/Python312/python.exe.
The example launcher discovers sys.executable automatically and generates
an appropriate manifest, so its checked-in example is portable.

Launch it with:

    ui4me-host run --app /absolute/path/to/app.toml

In the backend, import RpcClient from the supplied
examples/python/client.py, connect with RpcClient.from_environment(),
create a window, register close/quit callbacks and call app.ready.
The host passes a private endpoint and one-time credential in the child
environment. Do not put those values in source, command-line arguments or logs.
See the complete [Python application](examples/python/demo.py) and the
[protocol reference](docs/PROTOCOL.md).

The normal flow is:

1. The native host starts and owns the application lifetime.
2. It starts one managed backend process.
3. The backend authenticates and creates its windows through local RPC.
4. Native input, IME, rendering of the probe and accessibility remain responsive
   independently of a blocked backend callback.
5. Close and application quit requests wait asynchronously for allow/cancel.
6. Shutdown closes the backend and its managed descendants.

## What is implemented

| Area | Implementation |
| --- | --- |
| Native windows | Shared controller, multiple windows, visibility, title, sizing, focus requests, minimize/maximize/fullscreen |
| Application lifetime | Startup deadline, authenticated ready handshake, asynchronous close/quit veto, bounded shutdown, backend failure diagnostics |
| Local protocol | Framed JSON-RPC 2.0 over a private Unix socket or Windows named pipe, bounded messages/queues, string IDs |
| Process management | systemd user-service containment on Linux; Windows Job Object assignment at child creation |
| Desktop services | Text clipboard, file/folder/save dialogs with real parents and cancellation, external open, application paths, application icons and per-user launcher tooling |
| Native input probe | Keyboard, selection, Unicode-safe editing, IME composition and surrounding text, file drop, DPI-aware CPU presentation |
| Accessibility | Host-owned AccessKit tree, AT-SPI on Linux and UI Automation on Windows, focus/value/selection/button actions |
| Development | Python reference client, regression tests, verification reports, Linux/Windows CI, RDP/VM tooling and autonomous finishing instructions |

The probe has a single-line editor. It preserves Unicode text; keyboard movement
and unselected deletion use grapheme boundaries. Its diagnostic font rasterizer does not implement complex
script shaping or a production font fallback system. This is a renderer boundary,
not a claim that a full text engine exists.

## Verify

    python tools/verify.py
    python tools/native_acceptance.py --automated

The first command runs format, build/type checks, Rust and Python tests, and a
headless protocol application. The second runs only an automated desktop smoke
subset. Neither silently converts untested native scenarios into passing ones.
The native acceptance ledger retains separate Linux and Windows evidence.

The actual CPU probe can also be rendered to a PNG without opening a window:

    cargo run --locked --example render_probe -- probe.png

This checks the diagnostic surface's pixels, not native desktop integration.

The real socket and process integration tests require permission to create local
IPC endpoints. A locked-down cloud container may compile the code while blocking
those tests. The test failures should be fixed locally when caused by code;
environment denials must be reported explicitly.

--headless is for protocol/lifecycle tests and reports native capabilities as
unavailable. --allow-unsupervised is an explicit Linux diagnostic escape hatch
when a systemd user manager is unavailable. It does **not** qualify containment
after abrupt host death.

## Repository map

| Path | Purpose |
| --- | --- |
| crates/ui4me-host/src/host/ | Shared controller, RPC dispatch and native event handling |
| src/config.rs, src/lifecycle.rs, src/protocol.rs inside the crate | Manifest, policy and wire contracts |
| src/platform/, src/transport.rs, src/backend.rs inside the crate | OS adapters, bounded local transport and backend supervision |
| src/presentation.rs, src/accessibility.rs inside the crate | Integration probe and accessibility adapter |
| crates/ui4me-host/tests/ | Real binary/process regression tests |
| examples/python/ | Standard-library-only reference language client and app |
| tools/ | Build/install/verification and authorized local-lab helpers |
| docs/ | Architecture, protocol, verification status and finishing task |

The repository uses existing libraries as dependencies instead of copying an
application framework wholesale. Key choices are documented in
[SPEC](docs/SPEC.md) and [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES.md).
