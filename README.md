# UI4ME Core

**Native desktop apps from local HTML and CSS, with your logic in plain Python or PowerShell.**

[![CI](https://github.com/ui4me/ui4me-core/actions/workflows/ci.yml/badge.svg)](https://github.com/ui4me/ui4me-core/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/ui4me-core?label=PyPI)](https://pypi.org/project/ui4me-core/)
[![PowerShell Gallery](https://img.shields.io/powershellgallery/v/UI4ME.Core?label=PowerShell%20Gallery)](https://www.powershellgallery.com/packages/UI4ME.Core)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

[Documentation](docs/index.md) · [Python quickstart](docs/guides/quickstart-python.md) · [PowerShell quickstart](docs/guides/quickstart-powershell.md) · [Python API](docs/reference/python-api.md) · [PowerShell commands](docs/reference/powershell-api.md) · [Releases](https://github.com/ui4me/ui4me-core/releases)

<details>
<summary><b>Install for Python on Linux</b></summary>

Python 3.10+ on Linux x86-64 (glibc 2.34+) with a Wayland desktop session.

```bash
# With uv (recommended)
uv init --bare my-app && cd my-app
uv add ui4me-core

# Or with pip in a virtual environment
python3 -m venv .venv
source .venv/bin/activate
python -m pip install ui4me-core
```

The wheel contains the native host, so you do not need Rust. Check it with `ui4me-host --version`.

</details>

<details>
<summary><b>Install for Python on Windows</b></summary>

Python 3.10+ on Windows x64, in PowerShell or Command Prompt.

```powershell
# With uv (recommended)
uv init --bare my-app; cd my-app
uv add ui4me-core

# Or with pip in a virtual environment
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install ui4me-core
```

The wheel contains the native host, so you do not need Rust. Check it with `ui4me-host --version`.

</details>

<details>
<summary><b>Install for PowerShell</b></summary>

PowerShell 7.2+ on Linux x64 or Windows x64, or Windows PowerShell 5.1. The module bundles the native hosts and does not need Python or Rust.

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
Import-Module UI4ME.Core
& (Get-UI4MEHostPath) --version
```

</details>

<details>
<summary><b>Documentation map</b></summary>

| Topic | Read |
| --- | --- |
| First steps | [Install for Python](docs/guides/install-python.md) · [Install for PowerShell](docs/guides/install-powershell.md) · [Python quickstart](docs/guides/quickstart-python.md) · [PowerShell quickstart](docs/guides/quickstart-powershell.md) |
| Building apps | [Python recipes](docs/guides/patterns.md) · [PowerShell recipes](docs/guides/patterns-powershell.md) · [Lifecycle and events](docs/concepts/lifecycle.md) · [HTML and CSS support](docs/concepts/html-css-scope.md) · [Platforms](docs/guides/platforms.md) |
| Reference | [Python API](docs/reference/python-api.md) · [PowerShell commands](docs/reference/powershell-api.md) · [`ui4me-host` and app.toml](docs/reference/host-cli.md) · [IPC protocol](docs/reference/protocol.md) |
| When something goes wrong | [Troubleshooting](docs/guides/troubleshooting.md) · [Architecture](docs/concepts/architecture.md) |
| Contributing and releases | [Development](docs/guides/development.md) · [Building](docs/release/building.md) · [Publishing](docs/release/publishing.md) · [Release notes](docs/release/RELEASE_NOTES_1.0.1.md) |

</details>

<details>
<summary><b>Teach your coding agent UI4ME</b></summary>

Both packages ship an agent skill for Claude Code, Codex, OpenCode, GitHub Copilot, Cursor and Gemini CLI. The installers write to per-user skill folders, need no administrator rights and never overwrite skills they did not install.

```bash
# Python package
ui4me-skill install --agents all
ui4me-skill status --agents all
```

```powershell
# PowerShell module
Install-UI4MESkill -Agents all -Scope user
```

See [Agent skills](docs/guides/agent-skills.md) for project-scoped installs and the exact folders.

</details>

<a href="docs/images/desktop-workspace.png"><img src="docs/images/desktop-workspace.png" width="520" alt="A UI4ME window on KDE Plasma (Wayland) showing a styled workspace with a sidebar, form controls and a table"></a>

*A real UI4ME window on KDE Plasma (Wayland). Click for full size, or see the [live bitmap example](docs/images/live-bitmap.png).*

## What UI4ME does

You describe the interface with ordinary HTML and CSS files, and handle buttons, forms and other events with ordinary Python functions or PowerShell script blocks. UI4ME's native host, written in Rust, opens real desktop windows and does the layout, rendering, text editing, input methods, accessibility and native dialogs itself.

**No browser or JavaScript engine is involved.** There is no Chromium, WebKit, Electron, embedded webview, Qt or local web server. Your code runs in its own process and talks to the host over a private, authenticated local connection. In practice:

- **Small and self-contained:** `pip install` or `Install-Module` is all it takes. There is no browser runtime to download or keep patched.
- **Your language, all the way down:** event handlers are Python or PowerShell, not JavaScript glue. Use your normal libraries, debugger and tests.
- **Responsive by design:** the window keeps painting, scrolling and editing while your code is busy, because the event loop belongs to the host.
- **Nothing exposed to the network:** documents load from local files and strings only, and there is no HTTP listener.

UI4ME implements a focused desktop profile of HTML and CSS. It is not a general web browser, so it doesn't run web pages or `<script>`. See [HTML and CSS support](docs/concepts/html-css-scope.md).

## Capabilities

| | |
| --- | --- |
| **Layout and styling** | Local HTML and CSS files, selectors, custom properties, web fonts, flexbox, grid, tables, scrolling, `prefers-color-scheme` |
| **Controls and forms** | Buttons, text inputs and text areas, checkboxes, radios, selects, form data, validation, focus and selection |
| **Live updates** | Change text, attributes, classes and HTML fragments; apply several changes as one batch; replace whole documents |
| **Desktop integration** | Multiple windows, native file and folder dialogs, clipboard, opening files and URLs, attention requests, desktop preferences |
| **Custom windows** | Frameless windows with HTML title bars, native dragging and resizing, live RGBA image surfaces |
| **Accessibility** | Native screen-reader trees on both platforms (AT-SPI on Linux, UI Automation on Windows) |

| Platform | Python package `ui4me-core` | PowerShell module `UI4ME.Core` |
| --- | --- | --- |
| Linux x86-64 (glibc 2.34+), Wayland desktop | Python 3.10–3.14 | PowerShell 7.2+ |
| Windows x64 | Python 3.10–3.14 | PowerShell 7.2+ and Windows PowerShell 5.1 |

Qualified on Debian 13 with KDE Plasma 6 (Wayland) and Windows Server 2022. macOS, X11 sessions and ARM64 are not supported. Platform differences are described in [Platforms](docs/guides/platforms.md).

## Your first Python app

Save this as `app.py`:

```python
from ui4me_core import Application

PAGE = """
<main style="font-family: sans-serif; padding: 24px">
  <h1>Hello from Python</h1>
  <label for="name">Your name</label>
  <input id="name" type="text">
  <button id="greet">Greet me</button>
  <p id="message">Type your name and press the button.</p>
</main>
"""


def main():
    with Application.start(name="Hello UI4ME") as app:
        window = app.open_html(PAGE, title="Hello UI4ME", width=520, height=320)
        document = window.document

        def greet(event):
            name = document.get_value("name").strip() or "world"
            document.set_text("message", f"Hello, {name}!")

        document.on("click", greet, element="greet")
        app.run()


if __name__ == "__main__":
    main()
```

Run it like any other script:

```bash
python app.py
# or, in a uv project
uv run app.py
```

`Application.start()` launches the native host, which starts your script a second time as the connected app. Code above `Application.start()` therefore runs twice, so keep your setup inside `main()`. `ui4me-new my-app` creates a project with separate HTML and CSS files.

## Your first PowerShell app

Save this as `app.ps1`:

```powershell
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Hello UI4ME'

$window = New-UI4MEWindow -Application $app -Title 'Hello UI4ME' -Width 520 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html @'
<main style="font-family: sans-serif; padding: 24px">
  <h1>Hello from PowerShell</h1>
  <label for="name">Your name</label>
  <input id="name" type="text">
  <button id="greet">Greet me</button>
  <p id="message">Type your name and press the button.</p>
</main>
'@

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element greet -Callback {
    $name = (Get-UI4MEValue -Document $doc -Element name).Trim()
    if (-not $name) { $name = 'world' }
    Set-UI4MEDocumentText -Document $doc -Element message -Text "Hello, $name!"
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run it:

```powershell
pwsh -File ./app.ps1
# Windows PowerShell 5.1
powershell.exe -NoProfile -File .\app.ps1
```

`Start-UI4MEScript` launches the native host, which starts your script a second time as the connected app. Code above `Start-UI4MEScript` therefore runs twice, so keep your setup below it. `New-UI4MEProject -Path ./my-app` creates a project with separate HTML and CSS files.

## Next steps

- [Python recipes](docs/guides/patterns.md) and [PowerShell recipes](docs/guides/patterns-powershell.md): the same 15 tasks in both languages, from forms, lists and file dialogs to close guards, background work and live bitmaps
- [Python examples](examples/python/README.md) and [PowerShell examples](powershell/examples/README.md): the same five apps (`hello`, `notes`, `counter`, `frameless`, `live-bitmap`) in both languages
- [Lifecycle and events](docs/concepts/lifecycle.md): callbacks, closing and quitting, and replacing documents
- [`ui4me-host` and app.toml](docs/reference/host-cli.md): explicit host launch for installers and deployments

UI4ME Core is MIT licensed; bundled third-party components keep their own licenses ([notices](THIRD_PARTY_NOTICES.md)). Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).
