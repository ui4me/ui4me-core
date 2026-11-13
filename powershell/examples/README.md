# PowerShell examples

Self-contained HTML/CSS/PowerShell apps for the UI4ME native host. Each `backend.ps1` imports an
installed `UI4ME.Core` module, or falls back to the module in this repository checkout.

```powershell
Install-Module UI4ME.Core -Scope CurrentUser        # or: Import-Module ./powershell/UI4ME.Core/UI4ME.Core.psd1
Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml
Start-UI4MEHost -Manifest ./powershell/examples/live-bitmap/app.toml
Start-UI4MEHost -Manifest ./powershell/examples/frameless/app.toml
```

From a source checkout without the Gallery package, point `UI4ME_HOST_BINARY` at a native host
(for example the `ui4me-host` from an installed `ui4me-core` wheel's `ui4me_core/_bin` folder).

- `counter`: click callbacks, atomic DOM batches, system clipboard, stylesheet from disk.
- `live-bitmap`: RGBA8 frames assembled with `[Buffer]::BlockCopy`, shared with the host, paced to 60 frames/s.
- `frameless`: HTML title bar with native drag, eight resize edges, minimize/maximize/close buttons.

The manifests run the backend with `pwsh`. For Windows PowerShell 5.1 change `program` to
`"powershell.exe"`; scripts containing non-ASCII text must be saved as UTF-8 with a BOM for 5.1.
