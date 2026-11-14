---
name: ui4me-powershell
description: Build native Linux and Windows desktop GUI applications in PowerShell with UI4ME Core (module UI4ME.Core) using local HTML and CSS. Use for creating windows, loading or generating HTML, handling events, forms, native dialogs, clipboard, custom title bars and live pixel previews from PowerShell 7 or Windows PowerShell 5.1. No browser, JavaScript, WPF or Python is involved.
---

# Build a PowerShell desktop app with UI4ME Core

**Use this skill when a user wants a desktop GUI written in PowerShell with HTML and CSS.**
UI4ME is a native Rust host that creates the windows, lays out and paints HTML/CSS, handles
input, IME and accessibility, and sends events to a separate PowerShell backend process.
It does **not** run JavaScript, WebView/WebKit/Chromium, WPF, WinForms or a web server.
Never write `<script>`, `onclick=`, `fetch`, HTTP listeners or WPF/XAML for these apps.

Read `references/powershell-api.md` (the full command reference, 83 commands) before writing
non-trivial code. Check a parameter with `Get-Help <command>` or
`(Get-Command <command>).Parameters.Keys` rather than guessing.

## Fast path

```powershell
Install-Module UI4ME.Core -Scope CurrentUser          # needs UI4ME.Core 1.0.1+
New-UI4MEProject -Path ./my-app -Name 'My App' -AppId org.example.MyApp
pwsh -File ./my-app/app.ps1                           # Windows PowerShell 5.1: powershell.exe -NoProfile -File .\my-app\app.ps1
```

The project has `app.ps1` (the logic), `index.html`, `style.css`, and an optional `app.toml`
for launchers that start the host explicitly (`Start-UI4MEHost -Manifest`). Edit the files
and run `app.ps1` again.

## Rules

1. **Start with `Start-UI4MEScript`** near the top of a `.ps1` file and run the file
   directly (`pwsh -File ./app.ps1`). It starts the native host, which runs the same script
   again as its backend; in that second run it returns the connected application.
2. **Everything above `Start-UI4MEScript` runs twice** (launcher and backend). Put only
   `param()`, `$ErrorActionPreference`, `Import-Module` and plain assignments there; put
   side effects below it. In the launcher run the call never returns: the script exits with
   the host's exit code.
3. App shape: `Start-UI4MEScript` -> `New-UI4MEWindow` -> `Open-UI4MEFile`/`Open-UI4MEHtml`
   -> register callbacks -> `Set-UI4MEReady` -> `Wait-UI4ME -UntilShutdown` -> `Close-UI4ME`.
4. Address elements by HTML **id**. Put an `id` on everything the script reads or changes,
   and make sure every `-Element` value exists in the HTML.
5. `Set-UI4MEDocumentText` for any text that comes from users, files or the network; it is
   never parsed as HTML. Use `Set-UI4MEDocumentHtml` only with trusted or
   `[Net.WebUtility]::HtmlEncode`d markup.
6. Loading another document makes the old document object, its nodes, bitmaps and
   subscriptions stale (`UI4ME.StaleObjectException`). Use the object returned by the latest
   `Open-UI4MEFile`/`Open-UI4MEHtml`.
7. Callbacks run on the script's runspace, one at a time, in host order. Keep them short.
   Never call UI4ME commands from jobs, runspaces or threads.
8. Callbacks defined **inside a function** need `.GetNewClosure()` called in that function
   to keep its local variables. Inside such a closure `$script:` is not the script scope:
   keep shared state in a captured hashtable or list instead.
9. Events arrive after the native action. Prevent defaults ahead of time with
   `Set-UI4MEPreventDefault` (for example form `submit`).
10. Close/quit policies (`Set-UI4MEBeforeClose`, `Set-UI4MEBeforeQuit`) decide by their
    **last output**: `$true` allows, `$false`/nothing vetoes.
11. Discard unwanted command output (`$null = Register-UI4MEDocumentEvent ...`); in the
    backend, stray output is printed to the user's terminal.
12. Use only local files and the `ui4me-res://` / `ui4me-bitmap://` schemes for assets.
    Remote `http(s)` stylesheets and images are not fetched.
13. Load files relative to `$PSScriptRoot`, not the current directory.
14. A `.ps1` with non-ASCII characters must be saved as UTF-8 **with BOM** for Windows
    PowerShell 5.1.

## Minimal app

`app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Notes' -AppId 'org.example.Notes'
$window = New-UI4MEWindow -Application $app -Title 'Notes' -Width 900 -Height 600
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')

$script:saved = 0
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element save -Callback {
    $text = Get-UI4MEValue -Document $doc -Element note
    $script:saved++
    Set-UI4MEDocumentText -Document $doc -Element status -Text "Saved $($text.Length) characters ($script:saved)"
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

`index.html` (next to `app.ps1`):

```html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Notes</title><link rel="stylesheet" href="style.css"></head>
<body>
  <main>
    <label for="note">Note</label>
    <textarea id="note" rows="8"></textarea>
    <button id="save">Save</button>
    <p id="status" aria-live="polite">Ready.</p>
  </main>
</body>
</html>
```

Run: `pwsh -File ./app.ps1`. Script arguments pass through to the backend
(`pwsh -File ./app.ps1 -Path x.txt` works with a `param([string]$Path)` block above
`Start-UI4MEScript`). Useful `Start-UI4MEScript` options: `-AppId` (reverse-DNS identity;
default is derived from the script path), `-Instance multiple` (default `single` forwards a
second launch to the running app as an `app.activated` notification), `-Version`, `-Icon`,
`-MaxWindows`.

## Common tasks

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
$null = Register-UI4MEDocumentEvent -Document $doc -Type input -Element name -Callback { param($e) $script:name = $e.value }
Set-UI4MEPreventDefault -Document $doc -Element form -EventType submit
$null = Register-UI4MEDocumentEvent -Document $doc -Type submit -Element form -Callback {
    $entries = (Get-UI4MEFormData -Document $doc -Form form).entries
}

# Checkboxes and selects
$on = Get-UI4MEChecked -Document $doc -Element agree
Set-UI4MESelectedValues -Document $doc -Element size -Values @('large')

# Right and middle click
$null = Register-UI4MEDocumentEvent -Document $doc -Type contextmenu -Element item -Callback { param($e) }
$null = Register-UI4MEDocumentEvent -Document $doc -Type auxclick -Element item -Callback { param($e) if ($e.button -eq 1) { } }

# Native dialogs (wait for the user; returns cancelled and paths)
$r = Show-UI4MEFileDialog -Window $window -Kind OpenFiles -Filters @(@{ name = 'CSV'; extensions = @('csv') })
if (-not $r.cancelled) { $path = $r.paths[0] }
$s = Show-UI4MEFileDialog -Window $window -Kind SaveFile -FileName 'report.txt'

# Clipboard, external links
Set-UI4MEClipboard -Application $app -Text 'hello'
$null = Open-UI4MEExternal -Application $app -Target 'https://example.com'

# Window state
$null = Set-UI4MEWindow -Window $window -Title 'Edited' -Maximized $true
$state = Get-UI4MEWindow -Window $window          # $state.observed.maximized

# Unsaved-changes guard
Set-UI4MEBeforeClose -Window $window -Callback { -not $script:dirty }

# Second launch of a single-instance app
Register-UI4MENotification -Application $app -Method app.activated -Callback { param($p) }

# Anything else in the document protocol
Invoke-UI4MEDocumentCommand -Document $doc -Operation scroll_into_view -Parameters @{ element = 'row-40' }
```

## Custom title bar (frameless window)

```powershell
$window = New-UI4MEWindow -Application $app -Title 'Tool' -Decorations $false -MinWidth 480 -MinHeight 320
```

```html
<header class="titlebar" data-ui4me-drag>
  <span>Tool</span>
  <nav data-ui4me-no-drag><button id="min">_</button><button id="max">[ ]</button><button id="close">X</button></nav>
</header>
<div class="edge e" data-ui4me-resize="e"></div>  <!-- also n s w ne nw se sw -->
```

Wire the buttons with `Set-UI4MEWindow -Minimized $true`, `-Maximized (-not $state.observed.maximized)`
and `Close-UI4MEWindow`. The host performs drag and resize natively; do not try to move the
window from a callback.

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
-TimeoutMilliseconds 0`, build the frame, `Send-UI4MELiveBitmap`, until
`$app.ShutdownRequested`. Avoid per-pixel PowerShell loops on large frames; copy precomputed
rows with `[Buffer]::BlockCopy`.

## Errors and debugging

```powershell
try { Set-UI4MEDocumentText -Document $doc -Element missing -Text x }
catch [UI4ME.RpcException] { $_.Exception.Code; $_.Exception.RpcMessage }
```

- `UI4ME.StaleObjectException`: an old document/node/bitmap was used.
  `UI4ME.RpcTimeoutException`: the outcome is unknown.
- Errors thrown in event callbacks are printed (`UI4ME: event handler for 'click' failed`)
  and do not stop the app. An uncaught error in the script body ends the app with a nonzero
  exit code and the host shows an error window.
- Output and errors of the backend appear in the terminal that ran `app.ps1`. Set
  `$env:UI4ME_VERBOSE = '1'` to also see the host log.
- Headless protocol tests (no desktop): `$env:UI4ME_TEST_MODE = '1'`; the test must end the
  app itself with `Request-UI4MEQuit`. `UI4ME_HOST_BINARY` selects another host build.

## Platform facts

- Linux needs PowerShell 7.2+ on an x86-64, glibc 2.34+ Wayland desktop; Windows x64
  supports `pwsh` 7.2+ and `powershell.exe` 5.1.
- Wayland: the app must receive input before it can set the clipboard; `-Focus`,
  positioning and un-minimizing are compositor-controlled.
- Executables, scripts and shortcuts are refused as `Open-UI4MEExternal` targets.

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

Documentation: https://github.com/ui4me/ui4me-core/blob/main/docs/reference/powershell-api.md
