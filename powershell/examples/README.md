# PowerShell examples

Small, complete HTML/CSS/PowerShell apps for UI4ME Core. Each example is a folder with its own `app.ps1`, `index.html` and `style.css`, and imports the installed `UI4ME.Core` module, or falls back to the module in this repository checkout. The same apps for Python are in [`examples/python`](https://github.com/ui4me/ui4me-core/tree/main/examples/python).

| Example | Shows | Run it with |
| --- | --- | --- |
| [`hello`](hello/app.ps1) | **Start here.** One window, a text field and a button; `Start-UI4MEScript`, `Open-UI4MEFile`, a click callback, `Get-UI4MEValue`, `Set-UI4MEDocumentText`. | `pwsh -File ./powershell/examples/hello/app.ps1` |
| [`notes`](notes/app.ps1) | HTML and CSS files, a form, a list built from PowerShell (`Set-UI4MEDocumentHtml` with encoded text), a native save dialog. | `pwsh -File ./powershell/examples/notes/app.ps1` |
| [`counter`](counter/app.ps1) | Click callbacks, atomic DOM batches (`Invoke-UI4MEBatch`), the system clipboard, a stylesheet from disk. | `pwsh -File ./powershell/examples/counter/app.ps1` |
| [`frameless`](frameless/app.ps1) | A window without system decorations: HTML title bar with native drag, eight resize edges, minimize/maximize/close buttons. | `pwsh -File ./powershell/examples/frameless/app.ps1` |
| [`live-bitmap`](live-bitmap/app.ps1) | Raw RGBA8 frames shared with the host through `ui4me-bitmap://`, assembled from precomputed rows with `[Buffer]::BlockCopy` in the script's own loop and paced to 60 frames/s. | `pwsh -File ./powershell/examples/live-bitmap/app.ps1` |
| [`showcase`](showcase/app.ps1) | A multi-page tour of the supported HTML and CSS: cards, forms with validation, flex and grid layouts, typography, a dialog, a popover, toasts and a light/dark theme. See the [showcase guide](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/showcase.md). | `pwsh -File ./powershell/examples/showcase/app.ps1` |

Install the module first:

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
```

## The direct-start style (all examples)

Every `app.ps1` is a normal script. Run it with `pwsh -File` (PowerShell 7.2+, Linux or Windows), or with `powershell.exe -NoProfile -File .\powershell\examples\hello\app.ps1` on Windows PowerShell 5.1. `Start-UI4MEScript` starts the native host, which runs the script again as its backend; code above that call runs twice, so the examples keep all real work below it. This is the recommended style for new apps (it is what `New-UI4MEProject` generates). See the [quickstart](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart-powershell.md).

## The manifest style (`counter`, `frameless`, `live-bitmap`)

These examples also have an `app.toml` manifest that runs the same `app.ps1`:

```powershell
Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml
```

The host starts `app.ps1` with its private connection, and `Start-UI4MEScript` then only connects. This style suits launchers and deployments that start the host explicitly. The manifests run the backend with `pwsh`; for Windows PowerShell 5.1 change `program` to `"powershell.exe"`. (`backend.ps1` in these folders only forwards to `app.ps1`, for launchers that still name it.) See [`ui4me-host` and app.toml](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/host-cli.md).

## From a source checkout

Without the Gallery package, set `UI4ME_HOST_BINARY` to a native host you built (or the `ui4me-host` in an installed `ui4me-core` wheel's `ui4me_core/_bin` folder). The examples fall back to the module in this checkout by themselves:

```bash
export UI4ME_HOST_BINARY="$PWD/target/release/ui4me-host"   # ui4me-host.exe on Windows
pwsh -File ./powershell/examples/hello/app.ps1
```

```powershell
$env:UI4ME_HOST_BINARY = (Resolve-Path ./target/release/ui4me-host).Path   # ui4me-host.exe on Windows
Import-Module ./powershell/UI4ME.Core/UI4ME.Core.psd1
pwsh -File ./powershell/examples/hello/app.ps1
Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml
```

Scripts that contain non-ASCII text must be saved as UTF-8 with a BOM, because Windows PowerShell 5.1 reads BOM-less scripts in the ANSI code page (`live-bitmap/app.ps1` has one).

## More Python apps

The larger desktop-workspace reference app (`renderer_demo.py`) and the frameless live-bitmap screenshot app (`live_bitmap_demo.py`) exist for Python only; see [`examples/python`](https://github.com/ui4me/ui4me-core/tree/main/examples/python). The PowerShell native tests in `powershell/tests/native` drive the `counter`, `frameless` and `live-bitmap` manifests above.

More: [quickstart](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart-powershell.md), [recipes](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns-powershell.md) and [command reference](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/powershell-api.md).
