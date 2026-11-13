# PowerShell applications with UI4ME Core

Build native Linux and Windows desktop windows from local HTML and CSS, with your
application logic in PowerShell. The `UI4ME.Core` module contains the same native host
executables as the `ui4me-core` 1.0.0 Python wheels. Running an app needs PowerShell and
the module, but not Python, Rust, a browser engine, JavaScript or a web server.

| Backend | Platforms |
| --- | --- |
| PowerShell 7.2+ (`pwsh`) | Linux x86-64 (glibc 2.34+), Windows x64 |
| Windows PowerShell 5.1 (`powershell.exe`) | Windows x64 |

Qualified desktops are the same as for the Python package: Debian 13 / KDE Plasma 6.3 on
native Wayland, and Windows Server 2022 Desktop Experience. See
[platforms](platforms.md) and [STATUS](../STATUS.md) for evidence.

## Install and create an app

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
New-UI4MEProject -Path ./hello -Name 'Hello' -AppId org.example.Hello
Start-UI4MEHost -Manifest ./hello/app.toml
```

`New-UI4MEProject` writes four files:

| File | Purpose |
| --- | --- |
| `app.toml` | Manifest: app id and name, backend program (`pwsh`, or `powershell.exe` with `-WindowsPowerShell`), host settings |
| `backend.ps1` | Your PowerShell logic |
| `index.html`, `style.css` | The user interface |

`Start-UI4MEHost` starts the native host, which starts `backend.ps1` and hands it a private
endpoint and token. **Do not run `backend.ps1` directly**; it cannot connect without the host.

## How an app is structured

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core
$app = Connect-UI4ME
try {
    $window = New-UI4MEWindow -Application $app -Title 'Hello' -Width 840 -Height 540
    $doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')

    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element greet -Callback {
        $name = (Get-UI4MEValue -Document $doc -Element name).Trim()
        Set-UI4MEDocumentText -Document $doc -Element result -Text "Hello, $name!"
    }

    Set-UI4MEReady -Application $app
    Wait-UI4ME -Application $app -UntilShutdown
} finally {
    Close-UI4ME -Application $app
}
```

```html
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><link rel="stylesheet" href="style.css"></head>
<body>
  <input id="name"><button id="greet">Greet</button>
  <p id="result" aria-live="polite"></p>
</body></html>
```

1. `Connect-UI4ME` authenticates with the host.
2. Create windows and load documents. `Open-UI4MEFile` loads a file and resolves relative
   stylesheets, images and fonts next to it. `Open-UI4MEHtml -Html $markup -AssetRoot $dir`
   loads generated HTML.
3. Register callbacks, then call `Set-UI4MEReady`.
4. `Wait-UI4ME -UntilShutdown` runs callbacks until the user closes the app or
   `Request-UI4MEQuit` succeeds. Always `Close-UI4ME` in `finally`.

Elements are addressed by HTML `id`. `Set-UI4MEDocumentText` never interprets HTML, so it
is safe for user input; `Set-UI4MEDocumentHtml` is for trusted markup only. Several changes
can be applied atomically:

```powershell
Invoke-UI4MEBatch -Document $doc -Operations @(
    @{ op = 'set_text'; element = 'count'; text = '42' },
    @{ op = 'add_class'; element = 'count'; name = 'updated' }
)
```

Every document command in the [protocol](../PROTOCOL_DOCUMENTS.md) is available through
`Invoke-UI4MEDocumentCommand`; typed commands cover the common ones. See the
[command reference](../reference/powershell-api.md).

## Callbacks

Callbacks run on your script's own PowerShell runspace, one at a time and in the order the
host sent them. A callback may call any UI4ME command. While it waits for a reply, events
that arrive meanwhile are held until it returns; close and quit decisions are still
answered. Keep callbacks short, because a slow callback delays later events.

**Variables in callbacks.** A callback written at the top level of `backend.ps1` sees the
script's variables (`$doc`, `$app`, `$script:count`). A callback created *inside a function*
needs `.GetNewClosure()`, called inside that function, to keep the function's local
variables:

```powershell
function Add-Counter($Document, [string]$Button, [string]$Output) {
    $state = @{ n = 0 }
    $null = Register-UI4MEDocumentEvent -Document $Document -Type click -Element $Button -Callback ({
        $state.n++
        Set-UI4MEDocumentText -Document $Document -Element $Output -Text $state.n
    }.GetNewClosure())
}
```

Inside a closure, `$script:` refers to the closure's own scope rather than your script, so
keep shared state in objects you capture (like `$state` above).

**Events** arrive after the native action happened (the text was typed, the button was
pressed). To stop a default action such as form submission, register it beforehand:
`Set-UI4MEPreventDefault -Document $doc -Element form -EventType submit`. Event objects carry
`type`, `element`, `path` and type-specific fields; `click`, `auxclick` (middle button,
`button = 1`), `contextmenu`, `input`, `change`, `keydown`, `focus`, `submit` and
composition events are all available.

**Close and quit policies** decide with their last output:

```powershell
Set-UI4MEBeforeClose -Window $window -Callback {
    -not $script:unsaved      # $true closes; $false keeps the window open
}
```

## Native services

```powershell
# File dialogs: returns @{ cancelled; paths }
$pick = Show-UI4MEFileDialog -Window $window -Kind OpenFiles -Multiple $true -Filters @(@{ name = 'Text'; extensions = @('txt', 'md') })
$save = Show-UI4MEFileDialog -Window $window -Kind SaveFile -FileName 'report.txt'

# Cancellable: start, then wait or cancel (Stop-UI4MERpc closes the dialog)
$request = Show-UI4MEFileDialog -Window $window -Kind ChooseFolder -NoWait
$folder = Wait-UI4MERpc -Request $request -TimeoutMilliseconds -1

Set-UI4MEClipboard -Application $app -Text 'copied'
Get-UI4MEClipboard -Application $app
Open-UI4MEExternal -Application $app -Target 'https://example.com'
```

On Wayland the compositor only lets an application set the clipboard after it received
keyboard or pointer input. Wayland also leaves window focus and placement to the
compositor: `Set-UI4MEWindow -Focus` is refused there, and a minimized window can only be
restored by the user.

## Frameless windows

Create the window with `-Decorations $false` and mark HTML regions. The host performs the
native drag and resize itself:

```html
<div class="titlebar" data-ui4me-drag>
  <span>My app</span>
  <nav data-ui4me-no-drag><button id="min">_</button><button id="max">□</button><button id="close">×</button></nav>
</div>
<div data-ui4me-resize="se" class="grip"></div>   <!-- n s e w ne nw se sw -->
```

```powershell
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element max -Callback {
    $state = Get-UI4MEWindow -Window $window
    $null = Set-UI4MEWindow -Window $window -Maximized (-not $state.observed.maximized)
}
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element min -Callback { $null = Set-UI4MEWindow -Window $window -Minimized $true }
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element close -Callback { Close-UI4MEWindow -Window $window }
```

## Live pixels

Show generated RGBA images (previews, plots, video-like output) without encoding them:

```html
<img id="preview" src="ui4me-bitmap://preview" width="320" height="200" alt="Preview">
```

```powershell
$bitmap = New-UI4MELiveBitmap -Document $doc -Id preview -Width 320 -Height 200
$frame = New-Object byte[] (320 * 200 * 4)     # straight-alpha RGBA, rows packed tightly
# ... fill $frame ...
$null = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame
```

`Send-UI4MELiveBitmap` returns once the host has taken the frame, which paces a producer
that is faster than the display. The pixels travel through a shared memory file, not JSON.
Per-pixel loops in PowerShell are slow: assemble frames from precomputed rows with
`[Buffer]::BlockCopy`, as in `powershell/examples/live-bitmap`, or compute them with .NET.

## Generated resources

Images and fonts made at run time can be served to the document:

```powershell
$null = Register-UI4MEResources -Document $doc -Name generated -Entries @{
    'chart.png' = @{ Bytes = $pngBytes; Mime = 'image/png' }
}
# <img src="ui4me-res://generated/chart.png">
```

## Windows PowerShell 5.1

`New-UI4MEProject -WindowsPowerShell` writes `program = "powershell.exe"` and saves
`backend.ps1` as UTF-8 with a byte-order mark, which Windows PowerShell 5.1 needs to read
non-ASCII text correctly. The module compiles a small C# helper with `Add-Type` on first
use in each process (about 0.1–0.5 s).

The generated manifests pass `-ExecutionPolicy Bypass` for the backend process only, so a
machine policy of `Restricted` does not stop your own app. An organization's Group Policy
still takes precedence.

## Coding agents

The module bundles a `ui4me-powershell` skill for coding agents:

```powershell
Install-UI4MESkill -Agents all -Scope user            # Claude Code, Codex, OpenCode, Copilot, Cursor, Gemini CLI
Install-UI4MESkill -Agents claude,codex -Scope project -ProjectPath .
Install-UI4MESkill -Action status
Install-UI4MESkill -Action uninstall -Agents all
```

The installer records what it wrote and refuses to overwrite or remove skill folders that
it did not create, or that were edited since, unless you pass `-Force`.

## Examples

`powershell/examples/` in the repository contains three runnable apps: a counter (events,
batches, clipboard), a live bitmap animation and a frameless window. From a checkout:

```powershell
Start-UI4MEHost -Manifest ./powershell/examples/counter/app.toml
```

## Troubleshooting

- **"UI4ME_ENDPOINT and UI4ME_TOKEN come from the native host"**: the backend was started
  directly. Use `Start-UI4MEHost`.
- **The window opens, then shows a failure message**: the backend stopped. The host log
  (its standard error) includes the PowerShell error and line.
- **Errors inside callbacks** are written to the host log and the app keeps running.
  Catch `UI4ME.RpcException` for host refusals; `$_.Exception.Code` holds the code.
- **`UI4ME.StaleObjectException`**: a document, node or bitmap was used after the window
  loaded another document or closed. Use the new object returned by `Open-UI4MEFile`.
- **Linux: "The native host is not executable"**: the module was extracted without the
  execute bit and could not be fixed automatically (for example in a read-only
  all-users location). Run `chmod +x` on the path from `Get-UI4MEHostPath`.
