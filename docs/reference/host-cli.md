# `ui4me-host` command line and `app.toml`

`ui4me-host` is the native Rust executable that owns windows, rendering, input, accessibility and the event loop. It starts your Python or PowerShell code as a separate, supervised backend process (see [Architecture](../concepts/architecture.md)).

**Most applications never call it directly.** `Application.start()` (Python) and `Start-UI4MEScript` (PowerShell) write a private temporary manifest, start the bundled host and wait for it. A hand-written `app.toml` is only needed for an explicit launch: installers, desktop launchers, process managers, deployments and acceptance tests.

## Where the executable comes from

| Package | Location of the host | Command |
| --- | --- | --- |
| Python wheel `ui4me-core` | `ui4me_core/_bin/ui4me-host` (`ui4me-host.exe` on Windows) | The `ui4me-host` and `ui4me` console scripts and `python -m ui4me_core` forward their arguments to it |
| PowerShell module `UI4ME.Core` | `runtimes/linux-x64/…/ui4me-host` or `runtimes/win-x64/…/ui4me-host.exe` | `Start-UI4MEHost -Manifest app.toml` runs a manifest; `Get-UI4MEHostPath` prints the path |

The Python forwarder (`ui4me_core.launcher`) uses `UI4ME_HOST_BINARY` if it is set, then the bundled binary, then a `ui4me-host` on `PATH`. On POSIX it **replaces itself** with the host (`execv`), so signals from a process manager reach the host directly and its exit status is returned unchanged. On Windows it starts the host and **waits** for it, returning its exit code. The PowerShell module also honours `UI4ME_HOST_BINARY`.

## Commands

```text
ui4me-host run --app <APP> [--headless] [--allow-unsupervised]
ui4me-host probe [--allow-unsupervised]
ui4me-host render --html <HTML> --output <OUTPUT> [--width 900] [--height 600] [--scale 1]
ui4me-host doctor
ui4me-host --version | -V
ui4me-host --help | -h        (also: ui4me-host help <command>)
```

### `run`

Validates the manifest, starts the backend it describes and runs the application.

| Option | Meaning |
| --- | --- |
| `--app <APP>` | Path to the TOML manifest (required). |
| `--headless` | Protocol and lifecycle without a desktop: no native windows, dialogs, clipboard or external open. For CI and binding tests. It does not claim desktop integration. |
| `--allow-unsupervised` | Linux only: run without the `systemd-run --user` scope. A diagnostic mode; hard host death no longer reliably cleans up the backend. Ignored on Windows. |

Exit status: `0` after a normal shutdown, `1` when the manifest is invalid, the backend fails, does not become ready before `startup_timeout_ms`, exits on its own, exits non-zero during shutdown, or overruns `shutdown_timeout_ms`. A secondary launch forwarded to a running single instance exits `0` once the primary accepted it.

### `probe`

Opens a native diagnostic window without any language backend, to separate host/desktop problems (input, IME, clipboard, dialogs, accessibility) from application problems. `--allow-unsupervised` has the same meaning as for `run`.

### `render`

Renders one local HTML file to a PNG without a desktop or backend and prints a JSON summary. It is a screen-snapshot diagnostic, not a printing or PDF facility.

| Option | Default | Limits |
| --- | --- | --- |
| `--html <HTML>` | — | Regular UTF-8 file, at most 1 MiB. Relative resources resolve against its directory. |
| `--output <OUTPUT>` | — | PNG file to write. |
| `--width <WIDTH>` | `900` | Viewport width in CSS pixels, > 0. |
| `--height <HEIGHT>` | `600` | Viewport height in CSS pixels, > 0. |
| `--scale <SCALE>` | `1` | Device scale, 0.5–4. Width × height × scale² must stay within the renderer's 16,777,216-pixel budget. |

Resources (images, fonts) must settle within 15 seconds or the command fails.

### `doctor`

Prints a JSON object with environment facts: `host_version`, `protocol_version`, `os`, `architecture`, selected session variables (`XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`, `WAYLAND_DISPLAY`, `XDG_SESSION_ID`, `SESSIONNAME`, `UI4ME_SYSTEMD_UNIT`), the containment that `run` would use, standard per-app paths, renderer and windowing dependency. It does not run native tests and is not a qualification.

### Diagnostics output

The host writes structured JSON log lines to **stderr**. Backend stdout/stderr lines are drained into bounded buffers (512 records of up to 4 KiB each) and re-emitted as log records with `"source":"backend"`. The default filter is `ui4me_host=info`; set `RUST_LOG` (for example `RUST_LOG=ui4me_host=debug`) to change it. The direct-start launchers turn these records back into plain output; `UI4ME_VERBOSE=1` also shows informational host records.

## The `app.toml` manifest

A manifest is **data, not a shell command**. Arguments are passed as separate argv entries and are never shell-expanded (`$HOME` and `$(…)` stay literal). Unknown tables or keys are rejected. The file must be a regular UTF-8 file of at most 256 KiB.

### `[app]`

| Key | Required | Rules |
| --- | --- | --- |
| `id` | yes | ASCII, at most 128 bytes, at least two dot-separated components; each component begins with a letter and contains only letters, digits, `_` or `-` (for example `org.example.MyApp`). Used for the Wayland app ID / Windows AppUserModelID, single-instance arbitration and the per-app standard paths. |
| `name` | yes | 1–128 characters, not blank, no control characters. Default window title and accessibility application name. |
| `version` | no | Same rules as `name`. |
| `icon` | no | Path to a PNG (≤ 4 MiB, 1–1024 px per side). The file must exist. Without it, the built-in UI4ME icon is used. |

### `[backend]`

| Key | Required | Default | Rules |
| --- | --- | --- | --- |
| `program` | yes | — | Executable. A bare name (`python3`, `pwsh`) is looked up on `PATH`. A value containing `/` or `\`, or an absolute path, is resolved against the manifest directory and must exist and be a file. |
| `args` | no | `[]` | At most 256 arguments, each at most 32,768 UTF-8 bytes, no NUL. |
| `cwd` | no | manifest directory | Must exist and be a directory; relative values resolve against the manifest directory. |
| `env` | no | `{}` | Table of extra environment variables: at most 128 entries; names non-empty, without `=` or NUL, at most 256 bytes; values at most 32,768 bytes. |

The combined size of all arguments plus environment names and values is limited to 128 KiB.

**Reserved environment names.** `backend.env` cannot set `UI4ME_ENDPOINT`, `UI4ME_TOKEN`, `UI4ME_APP_ID`, `UI4ME_PROTOCOL_VERSION`, or any name starting with `UI4ME_HOST_` (compared case-insensitively). The host sets the first four for the backend; see the [protocol reference](protocol.md#backend-environment).

The backend inherits the host's environment, then `backend.env`, then the four transport variables. Its stdin is closed (null); stdout and stderr are captured by the host.

### `[host]`

The whole table is optional.

| Key | Default | Range |
| --- | --- | --- |
| `startup_timeout_ms` | `15000` | 1000–300000. Time from host start until the backend has authenticated **and** sent `app.ready`. |
| `shutdown_timeout_ms` | `5000` | 100–120000. Time the backend has to exit after `app.shutdown` before it is killed (exit status 1). |
| `max_windows` | `16` | 1–64. Further `window.create` requests fail with "busy". |
| `instance` | `"single"` | `"single"` or `"multiple"`. |

`Application.start()` and `Start-UI4MEScript` generate a manifest with `startup_timeout_ms = 30000` unless you pass another value.

### `{app_dir}` substitution and path resolution

`{app_dir}` is the only substitution. It is replaced by the canonical manifest directory in `backend.program`, every `backend.args` entry, `backend.cwd`, every `backend.env` value and `app.icon`. Forward slashes after it (`{app_dir}/app.py`) work on Windows too. Relative paths in `program` (with a separator), `cwd` and `icon` resolve against the manifest directory, not the shell's current directory.

**Virtual environments (Linux).** The host checks that `program` exists by resolving symlinks, but on Unix it starts the path *as written*. A venv's `bin/python` is a symlink whose own location selects the environment, so naming `{app_dir}/.venv/bin/python` runs with that venv's packages (including `ui4me-core`). On Windows the resolved path is started; `.venv\Scripts\python.exe` is a real file there.

**Windows specifics.** A bare `program` name is searched on `PATH` (a `PATH` in `backend.env` takes precedence) and `.exe` is appended when no extension is given. `.cmd` and `.bat` files are refused: batch scripts are never run implicitly.

## Supervision

| Platform | How the backend is contained |
| --- | --- |
| Linux | `run` and `probe` re-launch the host under a transient user service: `systemd-run --user --wait --pipe --collect --service-type=exec --expand-environment=no --property=KillMode=control-group --property=TimeoutStopSec=5s --same-dir`. Environment *names* are forwarded with `--setenv=NAME`, never values on the command line. The backend runs in its own session with a parent-death signal (`SIGKILL`). The supervised host also watches the launching process and shuts down if it disappears. Without a working systemd user manager, `run` fails; `--allow-unsupervised` is the explicit diagnostic escape hatch. |
| Windows | The backend is created inside a Job Object with *kill on job close*, so it ends with the host. The host releases a console window it owns when launched from Explorer or the Start menu. |

The direct-start launchers additionally bind the host to themselves: Linux uses a parent-death signal (`setpriv --pdeathsig` for PowerShell when available), Windows a kill-on-close job.

## Single instance

With `instance = "single"` (default), the first launch for a given `app.id` in the same desktop session becomes the primary instance. On Linux the session is the user plus the Wayland compositor socket; on Windows it is the logon session (a named mutex). A later launch does not start a backend. It sends `app.activate` to the primary with its `args`, `cwd` and (on Linux) `XDG_ACTIVATION_TOKEN`, retries while the primary is still starting (within `startup_timeout_ms` + 5 s), and exits `0`. The primary focuses one of its windows where the platform allows and delivers an `app.activated` notification to the backend. With `instance = "multiple"`, every launch is independent.

## Example: Python application

```toml
[app]
id = "org.example.InvoiceTool"
name = "Invoice Tool"
version = "1.2.0"
icon = "{app_dir}/icon.png"

[backend]
# The virtual environment's interpreter, named by its venv path (Linux).
# On Windows use "{app_dir}/.venv/Scripts/python.exe".
program = "{app_dir}/.venv/bin/python"
args = ["-u", "{app_dir}/app.py"]
cwd = "{app_dir}"

[backend.env]
PYTHONIOENCODING = "utf-8"
INVOICE_DATA_DIR = "{app_dir}/data"

[host]
startup_timeout_ms = 30000
shutdown_timeout_ms = 5000
max_windows = 8
instance = "single"
```

`app.py` is unchanged from a directly started app: inside a host-started backend, `Application.start()` sees `UI4ME_ENDPOINT`/`UI4ME_TOKEN` and connects instead of launching another host (`Application.from_environment()` connects explicitly). Run it with:

```bash
ui4me-host run --app /opt/invoice-tool/app.toml     # the ui4me-host command from the ui4me-core package
```

## Example: PowerShell application

```toml
[app]
id = "org.example.InvoiceTool"
name = "Invoice Tool"
version = "1.2.0"
icon = "{app_dir}/icon.png"

[backend]
# "pwsh" is found on PATH. For Windows PowerShell 5.1 use "powershell.exe".
program = "pwsh"
args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "{app_dir}/app.ps1"]
cwd = "{app_dir}"

[backend.env]
INVOICE_DATA_DIR = "{app_dir}/data"

[host]
startup_timeout_ms = 30000
shutdown_timeout_ms = 5000
max_windows = 8
instance = "single"
```

`app.ps1` is unchanged from a directly started app: inside a host-started backend, `Start-UI4MEScript` sees `UI4ME_ENDPOINT`/`UI4ME_TOKEN` and connects instead of launching another host (`Connect-UI4ME` connects explicitly). Run it with:

```powershell
Start-UI4MEHost -Manifest /opt/invoice-tool/app.toml     # or: & (Get-UI4MEHostPath) run --app /opt/invoice-tool/app.toml
```

The Python example above and this one differ only in `[backend]`. `ui4me-new` writes such a manifest next to a generated `app.py` (`--python` pins the interpreter); `New-UI4MEProject` writes one next to a generated `app.ps1` (`-WindowsPowerShell` selects `powershell.exe`).

## Direct start versus an explicit manifest

| | Direct start | Explicit manifest |
| --- | --- | --- |
| Command | `python app.py`, `uv run app.py`, `pwsh -File app.ps1`, `powershell.exe -File app.ps1` | `ui4me-host run --app app.toml`, `Start-UI4MEHost -Manifest app.toml` |
| Manifest | Private temporary `ui4me-start-*` folder, removed afterwards | Your file |
| Backend command | The same interpreter and script, started again | Whatever `[backend]` names |
| Default app ID | Derived from the script path (`local.ui4me.<name>.h<hash>`) | `app.id` |
| Host, protocol, supervision | Identical | Identical |

See also: [protocol reference](protocol.md), [lifecycle](../concepts/lifecycle.md), [troubleshooting](../guides/troubleshooting.md).
