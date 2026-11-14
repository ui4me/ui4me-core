---
name: ui4me-powershell
description: Build native Linux and Windows desktop GUI applications in PowerShell with UI4ME Core (module UI4ME.Core), using local HTML and CSS for the interface and PowerShell scriptblocks for behavior (no browser, no JavaScript, no web server). Use when creating or modifying a UI4ME.Core app - Start-UI4MEScript, windows, documents, events, forms, batch updates, styling, native file dialogs, clipboard, close/quit policy, custom title bars, live RGBA bitmaps - from PowerShell 7 or Windows PowerShell 5.1, or when debugging one.
---

# Build a desktop app with UI4ME Core (PowerShell)

**Use this skill when a user wants a desktop GUI written in PowerShell with HTML and CSS.**
UI4ME Core is a native Rust host that creates the windows, lays out and paints HTML/CSS,
handles input, IME, editing and accessibility, runs the event loop, and sends events to a
**separate supervised PowerShell process** over authenticated local IPC. It does **not**
run JavaScript, WebView/WebKit/Chromium, WPF, WinForms or a web server. Never write
`<script>`, `onclick=`, `fetch`, HTTP listeners or WPF/XAML for these apps. HTML
containing `<script>`, `<iframe>`, any `on...=` attribute, `contenteditable`,
`<input type="color">`/date/time inputs, or duplicate `id`s **fails to load**
(`UI4ME.RpcException` -32602).

Read [references/powershell-api.md](references/powershell-api.md) (the full command
reference, 83 commands) before writing non-trivial code. Check a parameter with
`Get-Help <command>` or `(Get-Command <command>).Parameters.Keys` rather than guessing.

## Fast path

```powershell
Install-Module UI4ME.Core -Scope CurrentUser          # needs UI4ME.Core 1.0.1+
New-UI4MEProject -Path ./my-app -Name 'My App' -AppId org.example.MyApp
pwsh -File ./my-app/app.ps1                           # Windows PowerShell 5.1: powershell.exe -NoProfile -File .\my-app\app.ps1
```

The project has `app.ps1` (the logic), `index.html`, `style.css`, and an optional `app.toml`
for launchers that start the host explicitly (`Start-UI4MEHost -Manifest`). Edit the files
and run `app.ps1` again. The module runs on PowerShell 7.2+ (Linux x64, glibc 2.34+, Wayland
desktop; Windows x64) and Windows PowerShell 5.1 (Windows x64).

## Minimal program

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'My App' -AppId 'org.example.MyApp'
$window = New-UI4MEWindow -Application $app -Title 'My App' -Width 520 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html '<h1>Hello UI4ME</h1>'
Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run with `pwsh -File ./app.ps1`, `powershell.exe -NoProfile -File .\app.ps1` (5.1), or
`./app.ps1` from an open prompt.

## How `Start-UI4MEScript` works (read this first)

`pwsh -File ./app.ps1` is the **launcher**: `Start-UI4MEScript` runs the packaged native
host, which starts the **same script again** (same PowerShell edition) as the backend with
a private connection. In the launcher the call never returns (the script exits with the
host's exit code). In the backend it connects and returns the `UI4ME.Application`.

- **Code above `Start-UI4MEScript` runs twice.** Put only `param()`,
  `$ErrorActionPreference`, `Import-Module` and plain assignments there; put side effects
  (file writes, network, logging, heavy init) below it.
- It must be called from a `.ps1` file; pasted into a prompt it throws
  `Start-UI4MEScript must be called from a .ps1 script file`.
- Arguments pass through: `pwsh -File ./app.ps1 -Path x.txt` gives the backend
  `-Path x.txt`, so a `param([string]$Path)` block works in both runs.
- `Write-Output`, `Write-Host` and errors in the backend appear in the terminal.
  `Read-Host` does not work (no stdin).
- Options: `-Name`, `-AppId` (reverse-DNS; default derived from the script path, so set one
  for real apps), `-Version`, `-Icon`, `-Instance single|multiple`,
  `-StartupTimeoutMilliseconds`, `-ShutdownTimeoutMilliseconds`, `-MaxWindows`. Invalid
  values throw.
- Exit code: 0 normal; nonzero if the backend failed (the host shows an error window); 130
  on Ctrl+C.

## Rules

1. **Start with `Start-UI4MEScript`** near the top of a `.ps1` file and run the file
   directly (`pwsh -File ./app.ps1`).
2. **Everything above `Start-UI4MEScript` runs twice** (launcher and backend). Keep only
   `param()`, `$ErrorActionPreference`, `Import-Module` and plain assignments there.
3. App shape: `Start-UI4MEScript` -> `New-UI4MEWindow` -> `Open-UI4MEFile`/`Open-UI4MEHtml`
   -> register callbacks with `Register-UI4MEDocumentEvent` -> `Set-UI4MEReady` ->
   `Wait-UI4ME -UntilShutdown` -> `Close-UI4ME`.
4. Address elements by HTML **id** (`-Element save` is an id, not a selector). Put an `id`
   on everything the script reads or changes. `-Element` also matches events bubbling from
   descendants; `$e.element` is the nearest identified element.
5. `Set-UI4MEDocumentText`/`Set-UI4MEDocumentValue` for any text that comes from users,
   files or the network; they are never parsed as HTML. Use `Set-UI4MEDocumentHtml` only
   with trusted or `[Net.WebUtility]::HtmlEncode()`d markup.
6. Loading another document makes the old document object, its nodes, bitmaps and
   subscriptions stale (`UI4ME.StaleObjectException`). Use the object returned by the
   latest `Open-UI4MEFile`/`Open-UI4MEHtml`, or `$window.Document`, and register callbacks
   again.
7. Callbacks run one at a time, in host order, on the script's own runspace while it pumps
   (`Wait-UI4ME`, or any command waiting for a reply). Keep them short; never call UI4ME
   commands from jobs, runspaces or threads (do slow work in `Start-Job` and poll it from a
   `Wait-UI4ME -TimeoutMilliseconds` loop). Close/quit policies run on the same runspace,
   so no lock is needed.
8. Callback state: a callback runs in a child scope. Assign script variables with
   `$script:name = ...`, or mutate a shared hashtable (`$state.dirty = $true`). Callbacks
   defined **inside a function** need `.GetNewClosure()` called in that function to keep
   its locals; inside such a closure `$script:` is not the script scope, so capture a
   hashtable or list instead. (Python counterpart: `nonlocal` or a mutable dict.)
9. Events arrive after the native action. A callback cannot cancel it: call
   `Set-UI4MEPreventDefault -Document $doc -Element form-id -EventType submit` *before* the
   input happens.
10. Close/quit policies (`Set-UI4MEBeforeClose`, `Set-UI4MEBeforeQuit`) decide by their
    **last output**: `$true` allows, `$false` or no output vetoes. A throw counts as a veto.
11. Discard unwanted command output (`$null = Register-UI4MEDocumentEvent ...`); in the
    backend, stray output is printed to the user's terminal and pollutes policy results.
    (Python counterpart: use named `def` callbacks.)
12. Use only local files and the `ui4me-res://` / `ui4me-bitmap://` schemes for assets.
    Remote `http(s)` stylesheets and images are not fetched.
13. Load files relative to the script: `Join-Path $PSScriptRoot 'index.html'`, and
    `Open-UI4MEHtml -AssetRoot $PSScriptRoot` for generated HTML with relative URLs.
14. Text offsets in `Get-UI4MESelection`/`Set-UI4MESelection` are **UTF-16 units** (an
    emoji counts as 2), which is also how .NET strings count.
15. Source encoding: a `.ps1` with non-ASCII characters must be saved as UTF-8 **with
    BOM**, or Windows PowerShell 5.1 reads it in the ANSI code page. (Python counterpart:
    `.py` files are UTF-8 by default.)

## Typical app

`index.html` (next to `app.ps1`):

```html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Tasks</title><link rel="stylesheet" href="style.css"></head>
<body>
  <form id="add-form">
    <label for="task">Task</label>
    <input id="task" name="task" type="text" required>
    <button id="add" type="submit">Add</button>
  </form>
  <ul id="tasks"></ul>
  <p id="status" role="status">Ready</p>
</body>
</html>
```

`app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Tasks' -AppId 'org.example.Tasks'
$window = New-UI4MEWindow -Application $app -Title 'Tasks' -Width 520 -Height 420
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
Set-UI4MEPreventDefault -Document $doc -Element add-form -EventType submit

$null = Register-UI4MEDocumentEvent -Document $doc -Type submit -Element add-form -Callback {
    $title = (Get-UI4MEValue -Document $doc -Element task).Trim()
    Invoke-UI4MEBatch -Document $doc -Operations @(
        @{ op = 'insert_html'; element = 'tasks'; position = 'beforeend'
           html = '<li>' + [Net.WebUtility]::HtmlEncode($title) + '</li>' },
        @{ op = 'set_text'; element = 'status'; text = 'Added ' + $title }
    )
    Set-UI4MEDocumentValue -Document $doc -Element task -Value ''   # form values are not batch operations
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type error -Callback {
    param($e)
    Write-Host "Document error: $($e.message)"
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run: `pwsh -File ./app.ps1`. The `required` attribute is validated natively, so an empty
task is never submitted.

## Common tasks

These lines run below `Start-UI4MEScript`, where `$app`, `$window` and `$doc` exist.

```powershell
# Several DOM changes atomically
Invoke-UI4MEBatch -Document $doc -Operations @(
    @{ op = 'set_text'; element = 'count'; text = '3' },
    @{ op = 'add_class'; element = 'count'; name = 'changed' },
    @{ op = 'set_attribute'; element = 'save'; name = 'disabled'; value = '' }
)

# Lists from data: build escaped HTML, then replace the container's children
$items = $rows | ForEach-Object { '<li>' + [Net.WebUtility]::HtmlEncode($_.Name) + '</li>' }
Set-UI4MEDocumentHtml -Document $doc -Element list -Html ($items -join '')

# Generated page with relative assets
$doc = Open-UI4MEHtml -Window $window -Html $html -AssetRoot $PSScriptRoot

# Forms
$null = Register-UI4MEDocumentEvent -Document $doc -Type input -Element name -Callback { param($e) $state.name = $e.value }
Set-UI4MEPreventDefault -Document $doc -Element form -EventType submit
$null = Register-UI4MEDocumentEvent -Document $doc -Type submit -Element form -Callback {
    $entries = (Get-UI4MEFormData -Document $doc -Form form).entries
}

# Checkboxes and selects
$on = Get-UI4MEChecked -Document $doc -Element agree
Set-UI4MESelectedValues -Document $doc -Element size -Values @('large')

# Right and middle click
$null = Register-UI4MEDocumentEvent -Document $doc -Type contextmenu -Element item -Callback {
    param($e)
    Write-Host "context menu at $($e.x), $($e.y)"
}
$null = Register-UI4MEDocumentEvent -Document $doc -Type auxclick -Element item -Callback {
    param($e)
    if ($e.button -eq 1) { Write-Host 'middle button' }
}

# Native dialogs (wait for the user; return cancelled and paths)
$r = Show-UI4MEFileDialog -Window $window -Kind OpenFiles -Filters @(@{ name = 'CSV'; extensions = @('csv') })
if (-not $r.cancelled -and @($r.paths).Count -gt 0) { $path = @($r.paths)[0] }
$s = Show-UI4MEFileDialog -Window $window -Kind SaveFile -FileName 'report.txt'

# Clipboard, external links
Set-UI4MEClipboard -Application $app -Text 'hello'
$null = Open-UI4MEExternal -Application $app -Target 'https://example.com'

# Window state
$null = Set-UI4MEWindow -Window $window -Title 'Edited' -Maximized $true
$windowState = Get-UI4MEWindow -Window $window   # $windowState.observed.maximized

# Unsaved-changes guard
Set-UI4MEBeforeClose -Window $window -Callback { -not $state.dirty }

# Second launch of a single-instance app
Register-UI4MENotification -Application $app -Method app.activated -Callback {
    param($params)
    Write-Host 'launched again'
}

# Anything else in the document protocol
$null = Invoke-UI4MEDocumentCommand -Document $doc -Operation scroll_into_view -Parameters @{ element = 'row-40' }
```

## Custom title bar (frameless window)

```powershell
$window = New-UI4MEWindow -Application $app -Title 'Tool' -Decorations $false -MinWidth 480 -MinHeight 320
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
```

```html
<header class="titlebar" data-ui4me-drag>
  <span>Tool</span>
  <nav data-ui4me-no-drag><button id="min">_</button><button id="max">[ ]</button><button id="close">X</button></nav>
</header>
<div class="edge e" data-ui4me-resize="e"></div>  <!-- also n s w ne nw se sw -->
```

```powershell
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element min -Callback {
    $null = Set-UI4MEWindow -Window $window -Minimized $true
}
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element max -Callback {
    $observed = (Get-UI4MEWindow -Window $window).observed
    $null = Set-UI4MEWindow -Window $window -Maximized (-not [bool]$observed.maximized)
}
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element close -Callback {
    Close-UI4MEWindow -Window $window
}
```

The host performs drag and resize natively; do not try to move the window from a callback.

## Live pixel previews

```html
<img id="view" src="ui4me-bitmap://view" width="320" height="200" alt="Preview">
```

```powershell
$bitmap = New-UI4MELiveBitmap -Document $doc -Id view -Width 320 -Height 200
$frame = New-Object byte[] (320 * 200 * 4)      # RGBA, straight alpha, tight rows
$null = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame   # returns when the host took it
```

For animation, replace `Wait-UI4ME -UntilShutdown` with a loop: `Wait-UI4ME -Application $app
-TimeoutMilliseconds <time until next frame>`, build the frame, `Send-UI4MELiveBitmap`, until
`$app.ShutdownRequested` or `$window.Closed`; then `Remove-UI4MELiveBitmap` and a final
`Wait-UI4ME -UntilShutdown`. There is no producer thread, so `-Try` (returns `$null` when a
present is in progress) only matters inside callbacks. Avoid per-pixel PowerShell loops;
copy precomputed rows with `[Buffer]::BlockCopy`.

## Errors and debugging

```powershell
try { Set-UI4MEDocumentText -Document $doc -Element missing -Text x }
catch [UI4ME.RpcException] { Write-Host $_.Exception.Code $_.Exception.RpcMessage }
```

- `UI4ME.StaleObjectException`: an old document/node/bitmap was used.
  `UI4ME.DisconnectedException`: the host went away. `UI4ME.RpcTimeoutException`: a wait
  expired and the outcome is unknown.
- Errors thrown in event callbacks are printed (`UI4ME: event handler for 'click' failed`)
  and do not stop the app. An uncaught error in the script body ends the app with a nonzero
  exit code and the host shows an error window.
- Output and errors of the backend appear in the terminal that ran `app.ps1`. Set
  `$env:UI4ME_VERBOSE = '1'` to also see the host log.
- `Get-UI4MEDiagnostics -Document $doc` and
  `Get-UI4MEComputedStyle -Document $doc -Properties prop -Element id` explain CSS that
  seems ignored. `Register-UI4MEDocumentEvent -Type error` reports asynchronous document
  errors.

## Verify your work

- Import and version: `(Get-Module UI4ME.Core -ListAvailable).Version`.
- Runtime check: `& (Get-UI4MEHostPath) --version` and `& (Get-UI4MEHostPath) doctor`.
- Headless smoke test (no desktop needed): `$env:UI4ME_TEST_MODE = '1'; pwsh -File ./app.ps1`.
  No window appears and the app must end itself, so temporarily add
  `$null = Request-UI4MEQuit -Application $app` just before `Wait-UI4ME -UntilShutdown`;
  exit code 0 (`$LASTEXITCODE`) means startup, document loading and shutdown worked.
  Assert on `Get-UI4MEDocumentText`/`Get-UI4MEValue` inside the script (`throw` on a
  mismatch).
- Real input, IME, clipboard, dialogs and appearance need a real desktop session (Wayland
  on Linux, an interactive session on Windows; not an SSH shell).

## Platform facts

- Linux needs PowerShell 7.2+ on an x86-64, glibc 2.34+ Wayland desktop; Windows x64
  supports `pwsh` 7.2+ and `powershell.exe` 5.1. macOS, X11-only sessions and ARM64 are not
  supported.
- Wayland: the app must receive input before it can set the clipboard; `-Focus`,
  positioning (`-X -Y`) and un-minimizing are compositor-controlled and throw
  `UI4ME.RpcException` -32001. Check `(Get-UI4MEWindow -Window $window).observed` and
  `Get-UI4MECapabilities`.
- Executables, scripts and shortcuts are refused as `Open-UI4MEExternal` targets.
- Links never navigate: `<a href>` sends a `link_activated` event with `href` and `url`.

## Advanced: explicit host launch

For packaging, launchers or custom process policy, the host can start the backend from a
manifest: `Start-UI4MEHost -Manifest ./app.toml` (or `ui4me-host run --app app.toml`; the
`app.toml` from `New-UI4MEProject` runs `app.ps1` with `pwsh`, or `powershell.exe` with
`-WindowsPowerShell`). In that backend `Start-UI4MEScript` simply connects; the lower-level
`Connect-UI4ME` does the same without launcher behavior and throws when no host started the
process. `$env:UI4ME_HOST_BINARY` selects a development host build.

## Do not

- Do not use JavaScript, WebView2, WPF, WinForms, Electron, Node, Python or a web server.
- Do not set or hardcode `UI4ME_ENDPOINT`/`UI4ME_TOKEN`, and do not run a `Connect-UI4ME`
  backend script by hand; use `Start-UI4MEScript`.
- Do not put side effects above `Start-UI4MEScript`, and do not expect code after it to run
  in the launcher.
- Do not interpolate untrusted text into HTML.
- Do not reuse documents, nodes or bitmaps after loading another document.
- Do not report a dialog, clipboard or external-open request as successful without checking
  its result.

## References

- [references/powershell-api.md](references/powershell-api.md): every command, objects, callbacks and closures, errors, limits.
- [references/projects.md](references/projects.md): project layout, `New-UI4MEProject`, running in both shells, manifests, packaging, common mistakes.
- [references/css-and-security.md](references/css-and-security.md): supported HTML/CSS, assets, security boundaries, platform differences.
- Online docs: [PowerShell commands](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/powershell-api.md), [recipes](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns-powershell.md), [HTML/CSS scope](https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md), [platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md).
- Complete example apps (hello, notes, counter, frameless, live bitmap): https://github.com/ui4me/ui4me-core/tree/main/powershell/examples

If a command is not listed, run `Get-Command -Module UI4ME.Core` and `Get-Help <command>`; never invent commands or browser APIs.
