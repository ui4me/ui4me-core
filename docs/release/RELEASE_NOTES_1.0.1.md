# UI4ME Core 1.0.1

UI4ME Core 1.0.1 makes UI4ME apps ordinary scripts: run them with `python app.py`, `uv run app.py` or `pwsh -File app.ps1`, without writing a manifest or starting `ui4me-host` yourself.

## New

- **`Application.start()` (Python).** Starts the bundled native host when you run a script directly, and connects when the host runs it as the app process. Works with `python app.py`, `uv run app.py`, `uv run python app.py` and `python -m package.app`. Script arguments, interpreter options, virtual environments and non-ASCII paths are preserved. The app's `print()` output appears in your terminal as plain text (`UI4ME_VERBOSE=1` adds the host's diagnostics). Exit status: 0 after a normal close, nonzero after a failure, and 128+signal after Ctrl+C.
- **`Start-UI4MEScript` (PowerShell).** The same for `pwsh -File ./app.ps1` (PowerShell 7.2+, Linux and Windows) and `powershell.exe -File .\app.ps1` (Windows PowerShell 5.1). It returns the connected application, so `$app = Start-UI4MEScript -Name 'My App'` replaces `Connect-UI4ME` in a directly run script. No Python is needed.
- **Lifetime binding.** If the process you started is killed, the host and app go with it: a parent-death signal on Linux, a kill-on-close job object on Windows. Ctrl+C shuts the app down gracefully, and temporary files are removed.
- **Single instance by default.** Launching an app that is already running forwards the launch to it (`app.activated`) and exits 0. Pass `instance="multiple"` / `-Instance multiple` to allow several copies.
- **Project generators run directly.** `ui4me-new` now creates `app.py`, `index.html`, `style.css`, a `pyproject.toml` for `uv run app.py`, and an optional `app.toml`. `New-UI4MEProject` creates a directly runnable `app.ps1` plus an optional `app.toml`.
- **Lifecycle hooks (Python):** `app.on_window_event(callback)` subscribes to native window events (closed, resized, focused, ...) after the API's own bookkeeping, and `app.on_shutdown(callback)` runs once the app has shut down. Frameworks no longer need to replace the private notification handlers.
- **New documentation** for both languages: installation, quickstarts, recipes, lifecycle, HTML/CSS scope, platforms, troubleshooting, a complete Python API and PowerShell command reference, the `ui4me-host` CLI and manifest, and the IPC protocol. The Python and PowerShell pages are 1:1: the same sections and equivalent examples in the install guides, quickstarts, recipes, API references and coding-agent skills. The PyPI description and the PowerShell Gallery readme are written for their own audiences. Both coding-agent skills teach the new startup model.
- **The same example apps in both languages.** `examples/python/` and `powershell/examples/` each contain `hello`, `notes`, `counter`, `frameless` and `live-bitmap`, every one a directly runnable `app.py` / `app.ps1` with its own HTML and CSS. The PowerShell `counter`, `frameless` and `live-bitmap` manifests now run `app.ps1` (`backend.ps1` remains as a forwarder), and the Python ones have matching manifests. `examples/python/hello.py` moved to `examples/python/hello/app.py`.

## Fixed

- **Windows:** a manifest whose `backend.program` is an absolute path to a .NET Framework program, such as `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`, failed with "The type initializer for 'System.Net.ServicePointManager' threw an exception". The host now starts backends without the verbatim `\\?\` path prefix.
- **Lost tracebacks:** when an exception escaped `with Application.start(...)` / `with Application.from_environment()`, the host could stop the failing process before Python printed the traceback. It is now printed first, exactly once. A `Start-UI4MEScript` app keeps its connection until the process ends, so PowerShell errors are shown too.
- `app.run()` could return, with the shutdown flag set, before the last window's `closed` event had been delivered, because `app.shutdown` is handled outside the ordered event queue. `run()` now waits (up to 2 seconds) for events that arrived before the shutdown.
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

Qualified on 2026-11-14 with release-candidate artifacts built from one sdist (full record: `tools/release/records/qualification-1.0.1.json`).

| | Linux (Debian 13, KDE Plasma 6, Wayland) | Windows Server 2022 x64 (interactive session) |
| --- | --- | --- |
| Source gates | fmt, Clippy with `-D warnings`, 1094 Rust tests, 120 Python tool tests, Pester 54/54, headless, renderer and failure-injection suites | host configuration tests including the new Windows test, Python tool tests, Pester 54/54 on PowerShell 7 and 54/54 on 5.1, headless, renderer and failure-injection suites |
| Packages | `twine check`, release-set verifier, manylinux smoke test, clean installs on Python 3.10, 3.12 and 3.13, Gallery stage verifier, Pester against the staged module | installed-wheel smoke test; staged module installed for PowerShell 7 and 5.1 |
| Direct start with real windows and real input: normal close, Ctrl+C or killed launcher, second launch, backend crash | `python app.py` 4/4, `uv run app.py` 4/4, `pwsh -File` 4/4 | `python app.py` 4/4, `pwsh -File` 4/4, `powershell.exe -File` 4/4 |
| PowerShell qualification harness (real input) | 28/28 | 28/28 on PowerShell 7, 28/28 on 5.1 |
| Other | renderer and host-probe automated subsets, generated projects, examples | |

### Known limitations

- Windows: a PowerShell app's own console output (`Write-Output`, `Write-Host`) loses non-ASCII characters on the way to the terminal; use `[Console]::Out.WriteLine` for exact text. The UI is unaffected.
- After Ctrl+C, a PowerShell launcher exits with the host's status (the app still shuts down gracefully); Python reports 128 + the signal number.
- If a window is closed at the very moment its backend is exiting after a failure, the status line can keep saying "Waiting for application close decision"; close it again.
- Rendering: dashed or dotted borders combined with `border-radius` are not drawn, and `option:checked` colours in `<select multiple>` are ignored. Date, time and colour inputs are still rejected (see [HTML and CSS support](../concepts/html-css-scope.md)).
- Only Windows Server 2022 and KDE Plasma 6 on Wayland were qualified for this release.
