# PowerShell examples

Small, complete HTML/CSS/PowerShell apps for UI4ME Core. Each example imports the installed
`UI4ME.Core` module, or falls back to the module in this repository checkout.

| Example | Shows | Start it with |
| --- | --- | --- |
| `hello` | **Start here.** One window, a text field and a button; `Start-UI4MEScript`, `Open-UI4MEFile`, a click callback, `Get-UI4MEValue`, `Set-UI4MEDocumentText`. | `pwsh -File ./powershell/examples/hello/app.ps1` |
| `counter` | Click callbacks, atomic DOM batches (`Invoke-UI4MEBatch`), the system clipboard, a stylesheet from disk. | `Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml` |
| `frameless` | A window without system decorations: HTML title bar with native drag, eight resize edges, minimize/maximize/close buttons. | `Start-UI4MEHost -Manifest ./powershell/examples/frameless/app.toml` |
| `live-bitmap` | Raw RGBA8 frames shared with the host through `ui4me-bitmap://`, assembled with `[Buffer]::BlockCopy` and paced to 60 frames/s. | `Start-UI4MEHost -Manifest ./powershell/examples/live-bitmap/app.toml` |

Install the module first:

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
```

## The direct-start style (`hello`)

`hello/app.ps1` is a normal script. Run it with `pwsh -File`, or with
`powershell.exe -NoProfile -File .\powershell\examples\hello\app.ps1` on Windows PowerShell
5.1. `Start-UI4MEScript` starts the native host, which runs the script again as its backend;
code above that call runs twice, so the example keeps all real work below it. This is the
recommended style for new apps (it is what `New-UI4MEProject` generates).

## The manifest style (`counter`, `frameless`, `live-bitmap`)

These examples have an `app.toml` manifest and a `backend.ps1` that calls `Connect-UI4ME`.
`Start-UI4MEHost -Manifest` starts the host, which starts `backend.ps1` with its private
connection, so `backend.ps1` cannot be run on its own. This style suits launchers and
deployments that start the host explicitly. The manifests run the backend with `pwsh`; for
Windows PowerShell 5.1 change `program` to `"powershell.exe"`.

## From a source checkout

Without the Gallery package, set `UI4ME_HOST_BINARY` to a native host you built (or the
`ui4me-host` in an installed `ui4me-core` wheel's `ui4me_core/_bin` folder):

```powershell
$env:UI4ME_HOST_BINARY = (Resolve-Path ./target/release/ui4me-host).Path   # ui4me-host.exe on Windows
Import-Module ./powershell/UI4ME.Core/UI4ME.Core.psd1
pwsh -File ./powershell/examples/hello/app.ps1
Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml
```

Scripts that contain non-ASCII text must be saved as UTF-8 with a BOM, because Windows
PowerShell 5.1 reads BOM-less scripts in the ANSI code page.

More: [quickstart](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart-powershell.md)
and [command reference](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/powershell-api.md).
