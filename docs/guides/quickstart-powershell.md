# Your first PowerShell app

This guide builds a small native window with HTML, CSS and PowerShell. You need the
`UI4ME.Core` module ([install it](install-powershell.md)); Python, Rust, a browser and
JavaScript are not needed. The same guide for Python is [Your first Python app](quickstart-python.md).

## 1. A window in one file

Create a folder with a file named `app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$page = @'
<main style="font-family: sans-serif; padding: 24px">
  <h1>Hello from PowerShell</h1>
  <button id="hello">Say hello</button>
  <p id="message" role="status">Click the button.</p>
</main>
'@

$app = Start-UI4MEScript -Name 'Hello UI4ME'
$window = New-UI4MEWindow -Application $app -Title 'Hello UI4ME' -Width 520 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html $page

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element hello -Callback {
    Set-UI4MEDocumentText -Document $doc -Element message -Text 'Hello from native UI4ME!'
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run it like any other script:

```powershell
pwsh -File ./app.ps1                          # PowerShell 7.2+, Linux or Windows
powershell.exe -NoProfile -File .\app.ps1     # Windows PowerShell 5.1
```

Running `./app.ps1` from a PowerShell prompt that is already open works too. A window opens;
press **Say hello** and the message changes. Close the window to end the script.

What each line does:

| Line | Purpose |
| --- | --- |
| `Start-UI4MEScript` | Starts the native window host and connects your script to it. Returns the application object. |
| `New-UI4MEWindow`, `Open-UI4MEHtml` | Creates a native window (size in CSS pixels), loads HTML into it and returns the document object. |
| `Register-UI4MEDocumentEvent` | Runs a scriptblock when the element with `id="hello"` is clicked. |
| `Set-UI4MEDocumentText` | Changes the text of the element with `id="message"`. |
| `Set-UI4MEReady`, `Wait-UI4ME -UntilShutdown` | Tells the host that startup is finished, then runs your callbacks until the user closes the app. |
| `Close-UI4ME` | Releases the connection. |

## 2. Why the top of the script runs twice

`Start-UI4MEScript` starts the Rust host, which then runs **the same script a second time**
as its supervised backend, with a private connection. In the first run (the launcher),
`Start-UI4MEScript` waits until the app ends and then exits the script with the host's exit
code (0 after a normal close), so nothing below it runs there. In the second run it returns
the connected application, and your code below builds the window.

So:

- Put `param()`, `$ErrorActionPreference`, `Import-Module` and plain assignments (such as the
  HTML string) above `Start-UI4MEScript`.
- Put everything with side effects (reading or writing files, logging, network calls,
  starting jobs) below it.
- Script arguments are passed on: `pwsh -File ./app.ps1 -Path data.csv` gives the backend
  `-Path data.csv` too, so a `param([string]$Path)` block works.
- `Write-Output`, `Write-Host` and errors from your script appear in the terminal you started
  it from. Set `$env:UI4ME_VERBOSE = '1'` before starting to see the host's own log as well.
- Ctrl+C in that terminal stops the app.
- The program must be a `.ps1` file: `pwsh -File ./app.ps1`, `powershell.exe -File .\app.ps1`
  and `./app.ps1` from a prompt work; code pasted into a prompt does not, because the host
  needs a script it can run again.

Useful options:

```powershell
$app = Start-UI4MEScript `
    -Name 'Invoice Tool' `
    -AppId 'com.example.InvoiceTool' `
    -Instance single
# -Name: window/app name (default: the script name)
# -AppId: stable reverse-DNS identity (default: derived from the script path)
# -Instance: single = a second launch reaches the running app; multiple allows several
```

## 3. Use HTML and CSS files

Real apps keep the interface in files next to the script. Create `index.html`:

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Greeter</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<main>
  <h1>Greeter</h1>
  <label for="name">Your name</label>
  <input id="name" type="text">
  <button id="greet">Say hello</button>
  <p id="message" aria-live="polite">Type a name and press the button.</p>
</main>
</body>
</html>
```

`style.css`:

```css
body { margin: 0; padding: 28px; font-family: system-ui, sans-serif; }
main { max-width: 480px; display: flex; flex-direction: column; gap: 8px; }
button { padding: 8px 16px; border: 0; border-radius: 8px; background: #3268d8; color: #fff; }
```

And `app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Greeter' -AppId 'org.example.Greeter'
$window = New-UI4MEWindow -Application $app -Title 'Greeter' -Width 560 -Height 360
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element greet -Callback {
    $name = (Get-UI4MEValue -Document $doc -Element name).Trim()
    if (-not $name) { $name = 'world' }
    Set-UI4MEDocumentText -Document $doc -Element message -Text "Hello, $name!"
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

`Open-UI4MEFile` resolves `style.css`, images and fonts relative to the HTML file. Use
`$PSScriptRoot` (the script's folder) so the script works from any current directory.
Elements are found by their HTML `id`, so give an `id` to everything your script reads or
changes. For HTML strings, pass `-AssetRoot $PSScriptRoot` to `Open-UI4MEHtml` if they
reference local files.

`Set-UI4MEDocumentText` never interprets HTML, which makes it safe for anything a user typed.
There is no JavaScript: `<script>` and `onclick="..."` do nothing. Handle events in
PowerShell with `Register-UI4MEDocumentEvent` instead.

`-AppId` gives the app a stable identity (reverse-DNS style). By default only one copy of an
app runs: starting it again while it is open hands the launch to the running copy. Pass
`-Instance multiple` to `Start-UI4MEScript` to allow several.

## 4. Or generate a project

```powershell
New-UI4MEProject -Path ./my-app -Name 'My App' -AppId org.example.MyApp
pwsh -File ./my-app/app.ps1
```

This writes `app.ps1` (saved as UTF-8 with BOM), `index.html`, `style.css` and an optional
`app.toml`. Daily development just runs `app.ps1`.

## 5. Launching the host explicitly (advanced)

Installers, desktop shortcuts and process managers can start the host from a checked-in
manifest such as the generated `app.toml`:

```powershell
Start-UI4MEHost -Manifest ./my-app/app.toml     # or: ui4me-host run --app ./my-app/app.toml
```

A backend started this way can still call `Start-UI4MEScript`, which then just connects.
Scripts that use `Connect-UI4ME` keep working with a manifest launch. `New-UI4MEProject
-WindowsPowerShell` makes the manifest's backend `powershell.exe` instead of `pwsh`. See
[`ui4me-host` and app.toml](../reference/host-cli.md).

## Common mistakes

- **Work above `Start-UI4MEScript` happens twice.** Move it below the call.
- **Nothing happens on click.** Check that the `-Element` id exists in the HTML, and that
  `Set-UI4MEReady` and `Wait-UI4ME -UntilShutdown` are reached.
- **A callback cannot change a script variable.** Assign it as `$script:name = ...` (see the
  `hello` example). Callbacks created inside functions lose the function's variables; use
  `.GetNewClosure()`, see the
  [command reference](../reference/powershell-api.md#callbacks-ordering-and-closures).
- **`Read-Host` hangs or fails.** The backend has no standard input; ask with a form control.
- **A non-ASCII script shows garbled text on Windows PowerShell 5.1.** Save the `.ps1` as
  UTF-8 *with BOM*.

## Next steps

- [PowerShell recipes](patterns-powershell.md): forms, lists, batch updates, screens, styles, file dialogs, close guards, background jobs and live images.
- [PowerShell command reference](../reference/powershell-api.md): every command.
- [Application lifecycle](../concepts/lifecycle.md) and [HTML/CSS scope](../concepts/html-css-scope.md).
- [Troubleshooting](troubleshooting.md).
- Examples in the repository: [`powershell/examples`](https://github.com/ui4me/ui4me-core/tree/main/powershell/examples).
