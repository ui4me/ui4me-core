# Install UI4ME Core for Python

You need **Python 3.10 or newer** and one of these platforms:

| Platform | Notes |
| --- | --- |
| Linux x86-64, glibc 2.34+ | Run apps inside a Wayland desktop session (qualified on KDE Plasma 6) |
| Windows x64 | Windows 10 or 11, or Windows Server with Desktop Experience (qualified on Windows Server 2022) |

The platform wheel already contains the native host (`ui4me-host`), so you do not need Rust or a compiler.

## With uv (recommended)

```bash
uv init --bare my-app
cd my-app
uv add ui4me-core
```

Put your `app.py` in that folder and run it with `uv run app.py`. In an existing uv project, just `uv add ui4me-core`.

## With pip on Linux

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install --upgrade ui4me-core
```

## With pip on Windows

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install --upgrade ui4me-core
```

If PowerShell refuses to run `Activate.ps1`, allow local scripts for your account once with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, or use `.venv\Scripts\activate.bat` from Command Prompt.

## Check the installation

```bash
python -c "import ui4me_core; print(ui4me_core.__version__)"
ui4me-host --version
ui4me-host doctor
```

`doctor` prints facts about the installation and desktop session as JSON. It does not open a window. Over SSH, or in a terminal without a desktop session, windows cannot open; run your apps from a terminal inside the desktop.

The package installs four commands:

| Command | Purpose |
| --- | --- |
| `ui4me-new` | Create a starter project (`app.py`, `index.html`, `style.css`, `pyproject.toml`, optional `app.toml`) |
| `ui4me-skill` | Install the coding-agent skill ([Agent skills](agent-skills.md)) |
| `ui4me-host` | The bundled native host: `--version`, `doctor`, `render`, `run --app` ([reference](../reference/host-cli.md)) |
| `ui4me` | Same as `ui4me-host` |

Next: [Your first Python app](quickstart-python.md).
