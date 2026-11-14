# UI4ME Core 1.0.1

UI4ME Core 1.0.1 makes UI4ME apps ordinary scripts: run them with `python app.py`, `uv run app.py` or `pwsh -File app.ps1`, without writing a manifest or starting `ui4me-host` yourself.

## New

- **`Application.start()` (Python).** Starts the bundled native host when you run a script directly, and connects when the host runs it as the app process. Works with `python app.py`, `uv run app.py`, `uv run python app.py` and `python -m package.app`. Script arguments, interpreter options, virtual environments and non-ASCII paths are preserved. The app's `print()` output appears in your terminal as plain text (`UI4ME_VERBOSE=1` adds the host's diagnostics). Exit status: 0 after a normal close, nonzero after a failure, and 128+signal after Ctrl+C.
- **`Start-UI4MEScript` (PowerShell).** The same for `pwsh -File ./app.ps1` (PowerShell 7.2+, Linux and Windows) and `powershell.exe -File .\app.ps1` (Windows PowerShell 5.1). It returns the connected application, so `$app = Start-UI4MEScript -Name 'My App'` replaces `Connect-UI4ME` in a directly run script. No Python is needed.
- **Lifetime binding.** If the process you started is killed, the host and app go with it: a parent-death signal on Linux, a kill-on-close job object on Windows. Ctrl+C shuts the app down gracefully, and temporary files are removed.
- **Single instance by default.** Launching an app that is already running forwards the launch to it (`app.activated`) and exits 0. Pass `instance="multiple"` / `-Instance multiple` to allow several copies.
- **Project generators run directly.** `ui4me-new` now creates `app.py`, `index.html`, `style.css`, a `pyproject.toml` for `uv run app.py`, and an optional `app.toml`. `New-UI4MEProject` creates a directly runnable `app.ps1` plus an optional `app.toml`.
- **New documentation** for both languages: installation, quickstarts, recipes, lifecycle, HTML/CSS scope, platforms, troubleshooting, a complete Python API and PowerShell command reference, the `ui4me-host` CLI and manifest, and the IPC protocol. The PyPI description and the PowerShell Gallery readme are written for their own audiences. Both coding-agent skills teach the new startup model.

## Fixed

- **Windows:** a manifest whose `backend.program` is an absolute path to a .NET Framework program, such as `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`, failed with "The type initializer for 'System.Net.ServicePointManager' threw an exception". The host now starts backends without the verbatim `\\?\` path prefix.
- **Lost tracebacks:** when an exception escaped `with Application.start(...)` / `with Application.from_environment()`, the host could stop the failing process before Python printed the traceback. It is now printed first, exactly once. A `Start-UI4MEScript` app keeps its connection until the process ends, so PowerShell errors are shown too.
- `Wait-UI4ME -UntilShutdown` no longer emits a stray `0` at the end of every script.
- `Application.from_environment()` and `Connect-UI4ME` explain how to start a script directly when no host connection exists. `from_environment()` now raises `ui4me_core.StartupError` instead of a bare `KeyError`.

## Compatibility

- Existing apps are unaffected. `ui4me-host run --app app.toml`, `Application.from_environment()`, `Connect-UI4ME` and `Start-UI4MEHost -Manifest` work as before, and a manifest-launched backend may also call the new start helpers.
- With direct start, code **above** the start call runs twice (once in the process you started, once in the app process). Keep side effects below it, and keep `if __name__ == "__main__":`.
- The app process has no standard input; the connection secret is removed from its environment after connecting.
- The Python client identifies itself as `ui4me-python 1.0.1`, and `ui4me_core` now exports `Node` and `StartupError`.
- Repository: historical milestone reports were removed from `docs/` (they remain in Git history and in the `v1.0.0` tag). `SOURCE_FILES.sha256` and the machine-readable release records moved to `tools/release/`. The Python examples import the `ui4me_core` package instead of carrying copies of it.

## Platforms

Linux x86-64 (glibc 2.34+, Wayland desktop session) and Windows x64; Python 3.10–3.14; PowerShell 7.2+ on both, and Windows PowerShell 5.1 on Windows.

## Qualification

QUALIFICATION_PENDING
