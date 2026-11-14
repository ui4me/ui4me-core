# Troubleshooting

Start by making sure you are on the current release:

```bash
python -c "import ui4me_core; print(ui4me_core.__version__)"
ui4me-host --version
```

```powershell
(Get-Module UI4ME.Core -ListAvailable).Version
& (Get-UI4MEHostPath) --version
```

Set `UI4ME_VERBOSE=1` (`$env:UI4ME_VERBOSE = '1'` in PowerShell) before starting an app to see the native host's diagnostic records in the terminal.

## The start of my script runs twice

This is expected. `Application.start()` and `Start-UI4MEScript` launch the native host, which runs your script a second time as the connected app. Code **above** the start call runs in both processes; code **below** it runs only in the app. Keep side effects (writing files, network calls, starting servers) below the start call. In Python, keep the `if __name__ == "__main__":` guard. Only the second run opens windows.

## `StartupError: Application.start() must run from a script file` / `Start-UI4MEScript must be called from a .ps1 script file`

The host needs something it can run again. In Python, start your app as `python app.py`, `uv run app.py` or `python -m package.module`; `python -c`, standard input, notebooks and the interactive interpreter are not supported. In PowerShell, call `Start-UI4MEScript` from a `.ps1` file and start it with `pwsh -File ./app.ps1`, `powershell.exe -File .\app.ps1` or `./app.ps1`; code pasted into a prompt is not supported.

## `No UI4ME host connection` or `UI4ME_ENDPOINT and UI4ME_TOKEN come from the native host`

The script uses `Application.from_environment()` (Python) or `Connect-UI4ME` (PowerShell). Those only connect to a host that already launched the script from a manifest. To run the script directly, use `Application.start()` or `Start-UI4MEScript` instead. Both also connect correctly when a manifest launch starts the script.

## `Incomplete UI4ME host environment`

Only one of `UI4ME_ENDPOINT` and `UI4ME_TOKEN` is set, usually left over from a shell or IDE configuration. Unset both and start the app again. The host sets them for the app process itself.

## The native host is missing

```bash
python -c "from ui4me_core import host_binary; print(host_binary())"
```

```powershell
Get-UI4MEHostPath
```

The platform wheels and the PowerShell module contain the host. Common causes:

- **Wrong environment:** the Python that runs `app.py` is not the one where you installed `ui4me-core`. With uv, use `uv run app.py` inside the project. In PowerShell, `Get-Module UI4ME.Core -ListAvailable` shows which module copies the session can find.
- **Unsupported platform:** there is no wheel for this OS or CPU, so pip fell back to the source distribution, which needs Rust; the PowerShell module contains only x64 hosts. Only Linux x86-64 (glibc 2.34+) and Windows x64 are supported.
- **Linux execute permission (PowerShell):** package extraction can drop the host's execute bit. `Get-UI4MEHostPath` restores it; if the module folder is read-only, run `chmod +x` on the printed path.
- **`UI4ME_HOST_BINARY` points to a missing file.** This variable is for development builds only; unset it.

## The window never appears (Linux)

UI4ME needs a Wayland desktop session. Run the app from a terminal inside the desktop, not over SSH or from a service without `WAYLAND_DISPLAY`. `ui4me-host doctor` shows what the host can see. On Linux the host runs under a transient `systemd --user` service. If `systemd-run` is unavailable or the user manager isn't running, starting fails with a message about systemd user supervision.

## The app starts and the error window says the app failed

Your app process raised an error or exited before an orderly quit. The traceback or PowerShell error is printed in the terminal you started from. Fix it, close the error window and start again. Common causes:

- the script ended without `app.run()` (Python) or `Wait-UI4ME -UntilShutdown` (PowerShell);
- `Set-UI4MEReady` or `app.ready()` was called before any window existed;
- startup took longer than the startup timeout. Pass a larger `startup_timeout_ms=` / `-StartupTimeoutMilliseconds`;
- (PowerShell) a stale document handle was used after loading another page (`UI4ME.StaleObjectException`); in Python the same mistake raises `StaleDocumentError`.

## A second launch does nothing

That is single-instance behaviour. The launch was forwarded to the copy that is already running, possibly minimized or on another virtual desktop. Pass `instance="multiple"` / `-Instance multiple` if you want independent copies.

## `input()` or `Read-Host` does not work

The app process has no console input. Ask for input with HTML form controls.

## My page looks different from a browser, or a feature is ignored

UI4ME implements a focused desktop profile of HTML and CSS, not a web browser. Scripts, iframes, network URLs, audio and video are not supported. Check [HTML and CSS support](../concepts/html-css-scope.md). The engine explains what it did:

```python
print(document.diagnostics())                                  # CSS warnings, resource errors
print(document.computed_style(["outline-color"], "card"))      # supported / unsupported / invalid
```

```powershell
Get-UI4MEDiagnostics -Document $doc                            # CSS warnings, resource errors
(Get-UI4MEComputedStyle -Document $doc -Properties outline-color -Element card).properties
```

Document errors that happen later (for example a failed clipboard transfer) arrive as `error` events: `document.on("error", callback)` in Python, `Register-UI4MEDocumentEvent -Document $doc -Type error -Callback { param($e) ... }` in PowerShell.

## Windows: PowerShell refuses to run the script

Allow local scripts for your account once: `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, or start the script with `powershell.exe -ExecutionPolicy Bypass -File .\app.ps1`. UI4ME starts its app process with `-ExecutionPolicy Bypass` for that process only. Python apps are not affected, but activating a virtual environment with `.venv\Scripts\Activate.ps1` is; use the same `Set-ExecutionPolicy` command, or `activate.bat` from Command Prompt.

## Windows PowerShell 5.1 shows garbled non-ASCII text

Save `.ps1` files that contain non-ASCII characters as **UTF-8 with BOM**. Windows PowerShell 5.1 reads files without a BOM using the legacy code page.

## Windows: non-ASCII text printed by a PowerShell app shows as `?`

The app process has no console. Windows PowerShell (and PowerShell 7 on Windows) writes its own output (`Write-Output`, `Write-Host`, error messages) through a legacy code page, so characters such as `ř` reach the terminal as `r` or `?`. Only this relayed text is affected; your HTML and the values you set in the page keep full Unicode. To print exact text, use `[Console]::Out.WriteLine("Příliš")`, which follows the UTF-8 output encoding that `Start-UI4MEScript` selects. Python apps are not affected: `print()` output is relayed as UTF-8.

## Behaviour differs between Linux and Windows

Desktop policies differ. On Wayland, for example, an app cannot move its own windows or steal focus. See [Platforms](platforms.md). Feature checks belong in your code, and requests the platform refuses raise an error with code `-32001` instead of pretending to work:

```python
try:
    window.set(x=100, y=100)
except RpcError as error:   # from ui4me_core import RpcError
    print("Not on this desktop:", error.code, error.message)
print(app.capabilities())
```

```powershell
try { $null = Set-UI4MEWindow -Window $window -X 100 -Y 100 }
catch [UI4ME.RpcException] { "Not on this desktop: $($_.Exception.Code) $($_.Exception.RpcMessage)" }
Get-UI4MECapabilities -Application $app
```

## Reporting a problem

Open an issue at <https://github.com/ui4me/ui4me-core/issues> with the UI4ME, Python or PowerShell, and OS versions, the desktop session type, a minimal script and HTML that reproduce it, and the terminal output with `UI4ME_VERBOSE=1`. Remove anything private first.
