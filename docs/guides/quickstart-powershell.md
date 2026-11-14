# Your first PowerShell desktop app

This guide builds a small native window with HTML, CSS and PowerShell. You need the
`UI4ME.Core` module ([install it](install-powershell.md)); Python, Rust, a browser and
JavaScript are not needed.

## 1. A window in one file

Create a folder with a file named `app.ps1`:

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

Run it like any other script:

```powershell
pwsh -File ./app.ps1                          # PowerShell 7.2+, Linux or Windows
powershell.exe -NoProfile -File .\app.ps1     # Windows PowerShell 5.1
```

Running `./app.ps1` from a PowerShell prompt that is already open works too. A window opens;
press **Go** and the heading changes. Close the window to end the script.

What each line does:

| Line | Purpose |
| --- | --- |
| `Start-UI4MEScript` | Starts the native window host and connects your script to it. Returns the application object. |
| `New-UI4MEWindow` | Creates a native window (size in CSS pixels). |
| `Open-UI4MEHtml` | Loads HTML into the window and returns a document object. |
| `Register-UI4MEDocumentEvent` | Runs a scriptblock when the element with `id="go"` is clicked. |
| `Set-UI4MEDocumentText` | Changes the text of the element with `id="title"`. |
| `Set-UI4MEReady` | Tells the host that startup is finished. |
| `Wait-UI4ME -UntilShutdown` | Runs your callbacks until the user closes the app. |
| `Close-UI4ME` | Releases the connection. |

## 2. Why the top of the script runs twice

`Start-UI4MEScript` starts the Rust host, which then runs **the same script a second time**
as its supervised backend, with a private connection. In the first run (the launcher),
`Start-UI4MEScript` waits until the app ends and then exits the script with the host's exit
code, so nothing below it runs there. In the second run it returns the connected
application, and your code below builds the window.

So:

- Put `param()`, `$ErrorActionPreference` and `Import-Module` above `Start-UI4MEScript`.
- Put everything with side effects (reading or writing files, logging, network calls) below it.
- Script arguments are passed on: `pwsh -File ./app.ps1 -Path data.csv` gives the backend
  `-Path data.csv` too, so a `param([string]$Path)` block works.
- `Write-Output`, `Write-Host` and errors from your script appear in the terminal you started
  it from. Set `$env:UI4ME_VERBOSE = '1'` before starting to see the host's own log as well.
- Ctrl+C in that terminal stops the app.

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
`$PSScriptRoot` so the script works from any current directory. Elements are found by their
HTML `id`, so give an `id` to everything your script reads or changes.

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

This writes `app.ps1`, `index.html`, `style.css` and an optional `app.toml`. The manifest is
only needed when something else should start the app explicitly with
`Start-UI4MEHost -Manifest ./my-app/app.toml`; add `-WindowsPowerShell` to make its backend
`powershell.exe` instead of `pwsh`. Daily development just runs `app.ps1`.

## Common mistakes

- **Work above `Start-UI4MEScript` happens twice.** Move it below the call.
- **Nothing happens on click.** Check that the `-Element` id exists in the HTML, and that
  `Set-UI4MEReady` and `Wait-UI4ME -UntilShutdown` are reached.
- **Callbacks inside functions lose variables.** Use `.GetNewClosure()`; see the
  [command reference](../reference/powershell-api.md#callbacks-ordering-and-closures).
- **A non-ASCII script shows garbled text on Windows PowerShell 5.1.** Save the `.ps1` as
  UTF-8 *with BOM*.

## Next steps

- [Common patterns](patterns.md): reading fields, batch updates, file dialogs, live images.
- [PowerShell command reference](../reference/powershell-api.md): every command.
- [Application lifecycle](../concepts/lifecycle.md) and [HTML/CSS scope](../concepts/html-css-scope.md).
- [Troubleshooting](troubleshooting.md).
- Examples in the repository: [`powershell/examples`](https://github.com/ui4me/ui4me-core/tree/main/powershell/examples).
