# UI4ME.Core - native desktop windows from HTML, CSS and PowerShell

UI4ME.Core lets a PowerShell script open real desktop windows whose interface is local HTML
and CSS. A native Rust host, included in this module, owns the windows, rendering, input,
IME, accessibility and dialogs; your script runs as its supervised backend and handles the
events. There is no browser engine, JavaScript, WPF, Python or web server.

## Install

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
```

| Shell | Operating system |
| --- | --- |
| PowerShell 7.2 or later (`pwsh`) | Linux x86-64 (glibc 2.34+, Wayland desktop), Windows x64 |
| Windows PowerShell 5.1 (`powershell.exe`) | Windows x64 |

## Your first app

Save this as `app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Hello UI4ME'
$window = New-UI4MEWindow -Application $app -Title 'Hello UI4ME' -Width 520 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html '<h1 id="title">Hello</h1><button id="go">Go</button>'
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element go -Callback {
    Set-UI4MEDocumentText -Document $doc -Element title -Text 'Clicked'
}
Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run it directly:

```powershell
pwsh -File ./app.ps1                          # PowerShell 7.2+, Linux or Windows
powershell.exe -NoProfile -File .\app.ps1     # Windows PowerShell 5.1
```

`Start-UI4MEScript` starts the native host, which runs the same script a second time as its
backend. Code above the call therefore runs twice: keep side effects below it. Your
script's output and errors appear in the terminal; Ctrl+C stops the app.

To start from a template with separate `index.html` and `style.css` files:

```powershell
New-UI4MEProject -Path ./my-app -Name 'My App' -AppId org.example.MyApp
pwsh -File ./my-app/app.ps1
```

## Key commands

| Command | Purpose |
| --- | --- |
| `Start-UI4MEScript` | Start the host and connect this script (returns the application) |
| `New-UI4MEWindow`, `Set-UI4MEWindow`, `Close-UI4MEWindow` | Create and control native windows |
| `Open-UI4MEFile`, `Open-UI4MEHtml` | Load HTML from a file or a string |
| `Register-UI4MEDocumentEvent` | Run a scriptblock on `click`, `input`, `change`, `submit`, ... |
| `Set-UI4MEDocumentText`, `Get-UI4MEValue`, `Set-UI4MEClass`, `Invoke-UI4MEBatch` | Read and change elements by HTML `id` |
| `Show-UI4MEFileDialog`, `Set-UI4MEClipboard`, `Open-UI4MEExternal` | Native dialogs, clipboard, default apps |
| `Set-UI4MEBeforeClose`, `Request-UI4MEQuit` | Close guards and quitting |
| `New-UI4MELiveBitmap`, `Send-UI4MELiveBitmap` | Live RGBA pixel previews |
| `Set-UI4MEReady`, `Wait-UI4ME -UntilShutdown`, `Close-UI4ME` | Finish startup, run callbacks, clean up |
| `New-UI4MEProject` | Generate a runnable project |
| `Install-UI4MESkill` | Install the coding-agent skill |

`Get-Command -Module UI4ME.Core` lists all 83 commands; `Get-Help <command>` describes each.

## Coding agents

The module bundles a `ui4me-powershell` skill for Claude Code, Codex, OpenCode, GitHub
Copilot, Cursor and Gemini CLI:

```powershell
Install-UI4MESkill -Agents all -Scope user
Install-UI4MESkill -Agents claude,codex -Scope project -ProjectPath .
```

## Troubleshooting

- **Code at the top of the script runs twice**: expected; move side effects below
  `Start-UI4MEScript`.
- **"UI4ME_ENDPOINT and UI4ME_TOKEN come from the native host"**: a script that calls
  `Connect-UI4ME` was run directly. Use `Start-UI4MEScript` instead, or start its manifest
  with `Start-UI4MEHost -Manifest ./app.toml`.
- **Garbled non-ASCII text on Windows PowerShell 5.1**: save the `.ps1` as UTF-8 with BOM.
- **More detail**: set `$env:UI4ME_VERBOSE = '1'` before starting the app to see the host log.
- **Linux: "The native host is not executable"**: run `chmod +x` on the path printed by
  `Get-UI4MEHostPath`.

More in the [troubleshooting guide](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/troubleshooting.md).

## Documentation

- [Install](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/install-powershell.md)
- [Quickstart](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart-powershell.md)
- [Command reference](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/powershell-api.md)
- [Supported platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md)
- [Coding-agent skills](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/agent-skills.md)
- [Examples](https://github.com/ui4me/ui4me-core/tree/main/powershell/examples)

The same guides are in this module's `docs` folder. The native hosts in `runtimes/` are the
binaries of the `ui4me-core` 1.0.1 Python wheels; `HOST-SOURCE-MANIFEST.json` records their
SHA-256. License texts are in `licenses/` and `runtimes/*/ui4me_core/_bin/THIRD_PARTY_LICENSES.txt`.

Source and issues: https://github.com/ui4me/ui4me-core (MIT license)
