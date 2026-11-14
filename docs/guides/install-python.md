# Install UI4ME Core for Python

The Python package **ui4me-core** contains the native window host for its platform. Nothing
else is needed at run time: no Rust, compiler, browser, JavaScript runtime or web server.
The same guide for PowerShell is [Install for PowerShell](install-powershell.md).

## Supported systems

| Python | Operating system |
| --- | --- |
| Python 3.10 to 3.14 | Linux x86-64 (glibc 2.34+, Wayland desktop session; qualified on KDE Plasma 6), Windows x64 (Windows 10 or 11, or Windows Server with Desktop Experience; qualified on Windows Server 2022) |

ARM64 and macOS are not supported in 1.0.1, and X11-only Linux sessions are not qualified. See
[platforms](platforms.md) for desktop behavior and the qualified environments.

## Install

### With uv (recommended)

```bash
uv init --bare my-app
cd my-app
uv add ui4me-core
```

Put your `app.py` in that folder and run it with `uv run app.py`. In an existing uv project,
just `uv add ui4me-core`.

### With pip on Linux

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install --upgrade ui4me-core
```

### With pip on Windows

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install --upgrade ui4me-core
```

## Check the installation

```bash
python -c "import ui4me_core; print(ui4me_core.__version__)"
python -c "from ui4me_core import host_binary; print(host_binary())"
ui4me-host --version
ui4me-host doctor
```

`host_binary()` prints the bundled native host. `doctor` prints facts about the installation
and desktop session as JSON; it does not open a window. Over SSH, or in a terminal without a
desktop session, windows cannot open; run your apps from a terminal inside the desktop.

Then run the [quickstart](quickstart-python.md):

```bash
ui4me-new hello --name "Hello"
python hello/app.py
```

## Update and uninstall

```bash
python -m pip install --upgrade ui4me-core
python -m pip uninstall ui4me-core

# In a uv project
uv lock --upgrade-package ui4me-core
uv remove ui4me-core
```

Restart running apps after an update; each app process keeps the version it started with.

`Application.start()` (direct `python app.py` start) needs ui4me-core 1.0.1 or later.
Projects that need it can say so with `ui4me-core>=1.0.1` in their dependencies (`ui4me-new`
writes that into `pyproject.toml`).

## Execution policy (Windows)

Python scripts are not subject to the PowerShell execution policy, but activating a virtual
environment with `Activate.ps1` is. If PowerShell refuses to run it, allow local scripts for
your account once, or use `.venv\Scripts\activate.bat` from Command Prompt:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

## What the package installs

| Command | Purpose |
| --- | --- |
| `ui4me-new` | Create a starter project (`app.py`, `index.html`, `style.css`, `pyproject.toml`, optional `app.toml`) |
| `ui4me-skill` | Install the coding-agent skill ([Agent skills](agent-skills.md)) |
| `ui4me-host` | The bundled native host: `--version`, `doctor`, `render`, `run --app` ([reference](../reference/host-cli.md)) |
| `ui4me` | Same as `ui4me-host` |

The `ui4me_core` package itself is described in the [Python API reference](../reference/python-api.md).

## Coding agents

The package bundles a skill that teaches coding agents (Claude Code, Codex, OpenCode, GitHub
Copilot, Cursor, Gemini CLI) to write UI4ME apps:

```bash
ui4me-skill install --agents all
```

See [agent skills](agent-skills.md).

## Working from a source checkout

Without the PyPI package, put the package source on `PYTHONPATH` and point
`UI4ME_HOST_BINARY` at a native host you built (see [development](development.md)):

```bash
export PYTHONPATH="$PWD/python"
export UI4ME_HOST_BINARY="$PWD/target/release/ui4me-host"   # ui4me-host.exe on Windows
```

Problems? See [troubleshooting](troubleshooting.md).
