# UI4ME Core documentation

**Build native desktop windows from local HTML and CSS, and control them with Python or PowerShell.** The UI runs in Rust rather than a browser. You do not need JavaScript, Electron, Qt, WebKit, a web server or a Rust toolchain to use the published packages.

## Start here

| I want to… | Start with | Next |
| --- | --- | --- |
| Build a Python desktop app | [Install for Python](guides/install-python.md) | [Your first Python window](guides/quickstart-python.md) |
| Build a PowerShell desktop app | [Install for PowerShell](guides/install-powershell.md) | [Your first PowerShell window](guides/quickstart-powershell.md) |
| Use a coding assistant | [Install agent skills](guides/agent-skills.md) | [Python API](reference/python-api.md) or [PowerShell commands](reference/powershell-api.md) |
| Understand capabilities | [What UI4ME supports](guides/platforms.md) | [HTML/CSS scope](concepts/html-css-scope.md) |
| Solve an installation or launch issue | [Troubleshooting](guides/troubleshooting.md) | [Runtime architecture](concepts/architecture.md) |
| Integrate, develop or release Core | [Development](guides/development.md) | [Release procedure](release/publishing.md) |

## Learn by building

Every Python page has a PowerShell twin with the same sections and equivalent examples:

| Topic | Python | PowerShell |
| --- | --- | --- |
| Install | [Install for Python](guides/install-python.md) | [Install for PowerShell](guides/install-powershell.md) |
| First app | [Your first Python app](guides/quickstart-python.md) | [Your first PowerShell app](guides/quickstart-powershell.md) |
| Recipes: events, forms, document replacement, styling, dialogs, background work, live bitmaps | [Python recipes](guides/patterns.md) | [PowerShell recipes](guides/patterns-powershell.md) |
| Every call | [Python API reference](reference/python-api.md) | [PowerShell command reference](reference/powershell-api.md) |
| Example apps (`hello`, `notes`, `counter`, `frameless`, `live-bitmap`) | [`examples/python`](https://github.com/ui4me/ui4me-core/tree/main/examples/python) | [`powershell/examples`](https://github.com/ui4me/ui4me-core/tree/main/powershell/examples) |

## How it works

[Architecture](concepts/architecture.md) explains the native host, supervised language process, authenticated local IPC and what happens when you run a Python or PowerShell script directly. [Lifecycle](concepts/lifecycle.md) explains callbacks, safe shutdown and document generations. [HTML/CSS scope](concepts/html-css-scope.md) sets realistic expectations: this is not a full browser. For lower-level integrators, see the [native host CLI](reference/host-cli.md) and [private IPC protocol](reference/protocol.md).

## Maintainers

Read [building and testing](guides/development.md), [release and publishing](release/publishing.md), [Linux and Windows build notes](release/building.md), and the [1.0.1 change notes](release/RELEASE_NOTES_1.0.1.md). Both PyPI and the PowerShell Gallery are manually published; no automatic publishing from a push.

**Source:** [UI4ME Core on GitHub](https://github.com/ui4me/ui4me-core) · [PyPI](https://pypi.org/project/ui4me-core/) · [PowerShell Gallery](https://www.powershellgallery.com/packages/UI4ME.Core)
