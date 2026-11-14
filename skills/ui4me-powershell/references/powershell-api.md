# PowerShell command reference (UI4ME.Core 1.0.1)

`UI4ME.Core` is the PowerShell binding for the UI4ME Core native host. The host (one Rust
process) owns the native windows, input, IME, HTML/CSS layout and rendering, accessibility and
dialogs. Your PowerShell script is the application backend: it runs as a separate process that
the host supervises, and the two talk over a private, authenticated, length-prefixed JSON-RPC
channel (a Unix-domain socket on Linux, a named pipe on Windows). No browser engine,
JavaScript, Python, Node.js or HTTP server is involved.

Supported shells: **PowerShell 7.2+** on Linux x64 and Windows x64, and **Windows PowerShell
5.1** on Windows x64. New to UI4ME? Start with the
[PowerShell quickstart](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/quickstart-powershell.md), then use the
[PowerShell recipes](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns-powershell.md). The Python binding has the same
structure: see the [Python API reference](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/python-api.md).

The module exports 83 functions. `Get-Help <command>` shows each one's help, and
`(Get-Command <command>).Parameters.Keys` lists its parameters.

## Contents

- [Starting an application](#starting-an-application)
- [Objects and results](#objects-and-results)
- [Application commands](#application-commands)
- [Windows](#windows)
- [Documents](#documents)
- [Events](#events)
- [Nodes](#nodes)
- [Live bitmaps](#live-bitmaps)
- [Errors](#errors)
- [Callbacks, ordering and closures](#callbacks-ordering-and-closures)
- [Raw protocol](#raw-protocol)
- [Environment variables](#environment-variables)
- [Limits](#limits)
- [Hosting, projects and agent skills](#hosting-projects-and-agent-skills)
- [Parity with the Python API](#parity-with-the-python-api)

Parameters in brackets are optional. `-Element` takes an HTML `id`. Commands that also
accept `-Node` take a `UI4ME.Node` from the same document; pass either `-Element` or
`-Node`, not both.

## Starting an application

```powershell
# app.ps1 - run it directly: pwsh -File ./app.ps1
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core

$app = Start-UI4MEScript -Name 'Hello UI4ME'
$window = New-UI4MEWindow -Application $app -Title 'Hello UI4ME' -Width 520 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html '<h1 id="title">Hello</h1><button id="go">Go</button>'
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element go -Callback {
    Set-UI4MEDocumentText -Document $doc -Element title -Text 'Clicked'
}
Set-UI4MEReady -Application $app          # startup finished (needs at least one window)
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
```

Run it with `pwsh -File ./app.ps1` (PowerShell 7.2+) or `powershell.exe -NoProfile -File
.\app.ps1` (Windows PowerShell 5.1). Running `./app.ps1` from an open prompt also works.

### How `Start-UI4MEScript` starts an app

```text
Start-UI4MEScript [-Name] [-AppId] [-Version] [-Icon] [-Instance single|multiple]
                  [-StartupTimeoutMilliseconds 30000] [-ShutdownTimeoutMilliseconds 5000]
                  [-MaxWindows 16] [-ArgumentList] [-ScriptPath]   -> UI4ME.Application
```

| Parameter | Meaning |
| --- | --- |
| `-Name` | Application and default window name, 1-128 characters without control characters. Default: the script file name. |
| `-AppId` | Reverse-DNS identity such as `org.example.Notes`: ASCII, at most 128 characters, at least two dot-separated parts, each starting with a letter and containing only letters, digits, `_` or `-`. Default: a stable `local.ui4me.<name>.h<hash>` derived from the script path. Pass an explicit `-AppId` for a real application. |
| `-Version` | Optional version text (same rules as `-Name`). |
| `-Icon` | Optional path to an application icon file. It must exist. |
| `-Instance` | `single` (default): launching the same app again hands the launch to the running instance, and the second launch exits with code 0. `multiple`: every launch is a separate application. |
| `-StartupTimeoutMilliseconds` | 1000-300000. How long the host waits for the backend to start. |
| `-ShutdownTimeoutMilliseconds` | 100-120000. How long an orderly shutdown may take before the host stops the backend. |
| `-MaxWindows` | 1-64. Maximum number of windows this application may open. |
| `-ArgumentList` | Arguments for the backend run (default: see below). |
| `-ScriptPath` | Names the script when it cannot be detected. |

Invalid values throw before anything starts.

The same script runs in two roles:

1. **Launcher** (you ran the script; `UI4ME_ENDPOINT` and `UI4ME_TOKEN` are not set).
   `Start-UI4MEScript` writes a private temporary manifest that names the same PowerShell
   edition you are using (`$PSHOME/pwsh`, or `powershell.exe` on Windows PowerShell 5.1) with
   `-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <script> <arguments>`,
   and starts the packaged native host. While the app runs it relays the backend's output
   (`Write-Output`, `Write-Host`, errors) to your console as plain text. Host warnings and
   errors appear as `ui4me-host error: ...`; set `$env:UI4ME_VERBOSE = '1'` to see every host
   record. When the host exits, the manifest is deleted and the launcher **`exit`s the
   script with the host's exit code**: the code below `Start-UI4MEScript` never runs in the
   launcher.
2. **Backend** (the host started the script again, with a connection in the environment).
   `Start-UI4MEScript` calls `Connect-UI4ME`, removes the connection variables from the
   environment (processes your script starts do not inherit the secret) and returns the
   `UI4ME.Application`. Your code below the call then builds the user interface.

Consequences:

- **Everything above `Start-UI4MEScript` runs twice.** Keep `param()`, `Import-Module` and
  plain assignments there; put side effects (files, network, logging) below the call.
- It must be called from a `.ps1` file (not pasted into a prompt).
- **Arguments.** By default the backend receives the arguments that followed the script on
  the `pwsh`/`powershell.exe` command line: `pwsh -File app.ps1 -Name x` runs the backend
  with `-Name x`, so `param()` blocks work in both roles. A script started from an already
  open prompt (`./app.ps1 -Name x`) passes none unless you give `-ArgumentList`.
- The backend has no console input, so `Read-Host` cannot be used. Output still reaches your
  terminal through the launcher.
- **Lifetime.** Ctrl+C in the launcher stops the host (exit code 130). If the launcher
  process dies, the host is stopped too (a kill-on-close job on Windows; `setpriv
  --pdeathsig` on Linux when `setpriv` is available).
- **Errors.** An uncaught error in the backend ends it: PowerShell prints the error, the host
  shows its application-error window, and the launcher exits with a nonzero code. In a
  `Start-UI4MEScript` backend, a premature `Close-UI4ME` (no shutdown was requested yet, for
  example in a `finally` block while an error unwinds) retires the handles but leaves the
  connection open until the process exits, so the error is printed before the host stops the
  failed backend.
- Inside a backend that was started from a manifest (`Start-UI4MEHost -Manifest`,
  `ui4me-host run --app`), `Start-UI4MEScript` simply connects, so the same script works
  both ways.

**Exit code** of `pwsh -File app.ps1`: 0 after an orderly shutdown; nonzero when the backend
failed (the host then keeps its own application-error window open until the user closes it);
130 after Ctrl+C.

**Single instance.** With `-Instance single`, a second launch of the same app identity brings
the running instance's window forward and the host sends an `app.activated` notification to
the running backend. Observe it with `Register-UI4MENotification`:

```powershell
Register-UI4MENotification -Application $app -Method app.activated -Callback {
    param($params)
    Write-Host 'The app was launched again'
}
```

### Manifest-based apps

For launchers, installers and deployments that start the host explicitly, an `app.toml`
names the backend program. The backend connects with `Start-UI4MEScript` (which then only
connects) or `Connect-UI4ME`:

```toml
[app]
id = "org.example.Hello"
name = "Hello"
version = "1.0.0"

[backend]
program = "pwsh"            # "powershell.exe" for Windows PowerShell 5.1
args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "{app_dir}/app.ps1"]
cwd = "{app_dir}"

[host]
startup_timeout_ms = 30000
shutdown_timeout_ms = 5000
max_windows = 8
instance = "single"
```

Run it with `Start-UI4MEHost -Manifest ./app.toml`. `New-UI4MEProject` writes this file next
to `app.ps1`. See [host CLI](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/host-cli.md) for the manifest fields.

**`Connect-UI4ME [-Endpoint] [-Token] [-ConnectTimeoutMilliseconds 15000]`** is the
advanced entry point for a backend that the host starts from an explicit manifest. It
connects and authenticates (`host.hello`, protocol 1) and does not launch anything. Defaults
come from `UI4ME_ENDPOINT` and `UI4ME_TOKEN`, which only the host sets; without them it
throws. Unlike `Start-UI4MEScript`, it leaves the variables in the environment.

## Objects and results

`Start-UI4MEScript` and `Connect-UI4ME` return a `UI4ME.Application`. `New-UI4MEWindow`
returns a `UI4ME.Window`. `Open-UI4MEFile` and `Open-UI4MEHtml` return a `UI4ME.Document`,
which is bound to one document generation: loading another document into the window,
unloading it, a fatal document error or closing the window makes it **stale**, and every
later command on it throws `UI4ME.StaleObjectException` locally. The same applies to the
nodes (`UI4ME.Node`), live bitmaps (`UI4ME.LiveBitmap`) and event subscriptions
(`UI4ME.Subscription`) of that document. After `Close-UI4ME` every handle is stale.

Useful properties: `$app.ShutdownRequested`, `$app.Hello` (the host's handshake reply),
`$window.Id`, `$window.Closed`, `$window.Document` (the current document object or `$null`),
`$doc.Retired`, `$bitmap.Sequence`.

Commands that return host data return objects whose properties are the JSON field names
(`$state.observed.maximized`, `$result.paths`). Numbers arrive as `[int]`, `[long]` or
`[double]`; strings are never converted to dates.

## Application commands

| Command | Description |
| --- | --- |
| `New-UI4MEWindow -Application ...` | Create a native window; see [Windows](#windows). Load content with `Open-UI4MEFile` or `Open-UI4MEHtml`. |
| `Set-UI4MEReady -Application` | Report that startup finished. Call it once, after the first window exists. |
| `Wait-UI4ME -Application -UntilShutdown` | Pump events and run callbacks until the host requests shutdown (the user closed the last window, or a quit succeeded). Throws `UI4ME.DisconnectedException` if the host disappears first. |
| `Wait-UI4ME -Application [-TimeoutMilliseconds 100] [-Once]` | Pump once: wait up to the timeout (`-1` = no limit) for the first message, then handle up to 64 queued messages (`-Once`: at most one). Returns the number handled. Use it in your own loop, for example for animation. |
| `Request-UI4MEQuit -Application` | Request an orderly quit; the quit policy may veto it. |
| `Set-UI4MEBeforeQuit -Application -Callback { param($request) $true }` | Quit policy (last output decides). `-Callback $null` removes it. |
| `Register-UI4MENotification -Application -Method -Callback { param($params) }` | Run a scriptblock for a host notification such as `app.activated`, `desktop.preferences_changed` or `window.event` (`$params.type` is `closed`, `close_requested`, `resized`, `focused`, ...). One callback per method; `-Callback $null` removes it. Built-in window and document bookkeeping runs first, so a `closed` window is already retired. |
| `Get-UI4MEClipboard -Application` | System clipboard text. |
| `Set-UI4MEClipboard -Application -Text` | Set it (up to 1 MiB). On Wayland the app must have received keyboard or pointer input first. |
| `Get-UI4MECapabilities -Application` | Host features and limits on this platform. |
| `Get-UI4MEPreferences -Application` | Observed desktop preferences: `color_scheme` (`dark`, `light` or `$null`), `reduced_motion`, `high_contrast` (`$null` when unknown), `available`, `source`, `revision`, `scope`. Changes arrive as the `desktop.preferences_changed` notification with the same structure. |
| `Open-UI4MEExternal -Application -Target [-Window]` | Open a URL (`http`, `https`, `mailto`, `file`) or a document path with the desktop's default handler. Executables, scripts and shortcuts are refused. Result: `accepted`, `cancelled`, `parent_forwarded`, `presented`. |
| `Open-UI4MEExternal -Application -Target -Reveal [-Window]` | Show a file in the file manager. |
| `Get-UI4MEWindowList -Application` | All open windows of the app. |
| `Close-UI4ME -Application` | Close the connection and retire every window, document and bitmap handle. Idempotent; `$null` is accepted. |

Closing the last window is a request to quit: the quit policy runs, and the application ends
if it allows. `Request-UI4MEQuit` from a callback is the normal way to end a headless test.

## Windows

| Command | Description |
| --- | --- |
| `New-UI4MEWindow -Application [-Title] [-Width 900] [-Height 600] [-Visible $true] [-Resizable $true] [-Decorations $true] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight]` | Create a native window (sizes in CSS px, 1-8192). Size constraints are set in pairs. |
| `Open-UI4MEFile` / `Open-UI4MEHtml` | Load a document into the window; see [Documents](#documents). |
| `Set-UI4MEWindow -Window [-Title] [-Width -Height] [-Visible] [-Resizable] [-Decorations] [-Minimized] [-Maximized] [-Fullscreen] [-Focus] [-X -Y] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight] [-Properties @{}]` | Change window state. Boolean state takes `$true`/`$false` (`-Maximized $true`). `-Properties` passes host field names directly; `@{ min_width = $null; min_height = $null }` clears a constraint. Returns the host result. A request is not proof that the desktop applied it: read `(Get-UI4MEWindow).observed`. |
| `Get-UI4MEWindow -Window` | Requested and observed state: `title`, `width`, `height`, `scale_factor`, `physical_size`, `observed.{focused,minimized,maximized,fullscreen,position,theme}`. |
| `Request-UI4MEAttention -Window [-Kind informational\|critical\|none]` | Ask the desktop to flag the window; `none` clears the request. |
| `Close-UI4MEWindow -Window` | Request a graceful close (the before-close policy may veto it). |
| `Set-UI4MEBeforeClose -Window -Callback { param($request) $true }` | Close policy for this window (last output decides). `-Callback $null` removes it. |
| `Show-UI4MEFileDialog -Window [-Kind OpenFiles\|SaveFile\|ChooseFolder] [-Title] [-Directory] [-FileName] [-Filters @()] [-Multiple $false] [-NoWait]` | Native file dialog (see below). |

Platform notes: on Wayland a client cannot focus, position or un-minimize its own window;
`-Focus` is refused there (`-32001`) and `observed.position` is `$null`. Use
`Request-UI4MEAttention` instead. Windows supports both. See [platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md).

**File dialogs.** `Show-UI4MEFileDialog` returns `{ cancelled, paths }` and waits without a
deadline while still answering lifecycle requests. `-FileName` applies to `SaveFile`,
`-Multiple` to `OpenFiles`. Filters: `@(@{ name = 'Images'; extensions = @('png', 'jpg') })`;
extensions have no leading dot (`'*'` matches everything). `-NoWait` returns a
`UI4ME.Request` immediately: `Wait-UI4MERpc` gives the result, and `Stop-UI4MERpc` asks the
host to close the native dialog.

**Frameless windows.** With `-Decorations $false`, mark a title-bar area in HTML with
`data-ui4me-drag` and resize edges with `data-ui4me-resize="n"`, `"s"`, `"e"`, `"w"`, `"ne"`,
`"nw"`, `"se"` or `"sw"`. The host handles these synchronously during the real pointer press
(a PowerShell callback would be too late to start a native move). Buttons, inputs, links and
anything inside `data-ui4me-no-drag` are never drag regions.

## Documents

A `UI4ME.Document` is one loaded page in one window. Elements are addressed by **HTML `id`
values** (`-Element`), not CSS selectors.

### Generations and stale documents

Every successful `Open-UI4MEFile`, `Open-UI4MEHtml` or `Remove-UI4MEDocument` creates a new
document generation. The previous document object, its nodes, its event subscriptions and its
live bitmaps are retired. Using them throws `UI4ME.StaleObjectException` instead of changing
the new page, even if the new HTML reuses the same IDs. After loading, use the returned
document (or `$window.Document`) and register callbacks again. A failed load leaves the
previous document in place.

**Loading rejects active content.** `Open-UI4MEFile` and `Open-UI4MEHtml` throw
`UI4ME.RpcException` (code -32602) for HTML that contains `<script>`, `<iframe>`, `<object>`
or `<embed>`, any `on...` event-handler attribute or `srcdoc`, `contenteditable` (other than
`false`), `<input type="color">` or date/time input types, or duplicate or invalid `id`
values. Keep behavior in PowerShell and use `<input>`/`<textarea>` for editing.

| Command | Description |
| --- | --- |
| `Open-UI4MEFile -Window -Path [-AssetRoot]` | Load an HTML file. Relative stylesheets, images and fonts resolve against its folder (or `-AssetRoot`). Atomic: if loading fails, the previous document stays. Returns the new `UI4ME.Document`; the old one becomes stale. Relative paths resolve against the current PowerShell location, so prefer `Join-Path $PSScriptRoot 'index.html'`. |
| `Open-UI4MEHtml -Window -Html [-AssetRoot]` | Load generated HTML (same rules). Use `-AssetRoot` (for example `$PSScriptRoot`) when the HTML has relative URLs. |
| `Remove-UI4MEDocument -Document` | Unload the document; the window stays open and empty. |
| `Invoke-UI4MEDocumentCommand -Document -Operation [-Parameters @{}]` | Run **any** document operation by name and return its result (see [below](#operations-without-a-typed-command)). |
| `Get-UI4MEDiagnostics -Document` | Renderer diagnostics for the document: CSS warnings, resource errors, `css_profile_version`. |

### Content, attributes and classes

| Command | Description |
| --- | --- |
| `Get-UI4MEDocumentText -Document -Element` / `Set-UI4MEDocumentText -Document -Element -Text` | Read/replace the element's text content. `Set-UI4MEDocumentText` never parses HTML, so it is safe for user input. |
| `Set-UI4MEDocumentAttribute -Document -Element -Name [-Value ''] [-Remove]` | Set or remove an attribute (`-Value ''` sets a boolean attribute such as `disabled`). |
| `Set-UI4MEClass -Document -Element -Name [-Remove]` | Add or remove a class. |
| `Set-UI4MEDocumentHtml -Document -Element -Html [-Position replace\|beforebegin\|afterbegin\|beforeend\|afterend]` | Replace the children with (default), or insert, **trusted** HTML. |
| `Remove-UI4MEElement -Document -Element` | Remove an element and its subtree. |
| `Invoke-UI4MEBatch -Document -Operations @(@{ op = ...; element = ... }, ...)` | Apply several changes as one atomic transaction (see below). |
| `Find-UI4MEElement -Document -Selector [-Offset 0] [-Limit 1024]` | CSS selector query. Returns `ids` (element ids of matches that have one), `total`, `offset`, `limit`, `truncated`. Limit 1-4096. |

To put user text inside generated HTML, encode it first with `[Net.WebUtility]::HtmlEncode()`.

**Batches.** `Invoke-UI4MEBatch` validates every entry before applying any of them; if one
fails, nothing changes. At most 1024 entries. Each entry is a hashtable with an `op` and the
same fields as the single operation: `set_text`, `append_text` (`element`, `text`);
`set_attribute` (`element`, `name`, `value`); `remove_attribute`, `add_class`, `remove_class`
(`element`, `name`); `insert_html` (`element`, `position`, `html`); `replace_children_html`
(`element`, `html`; what `Set-UI4MEDocumentHtml` sends); `remove_element` (`element`);
`move_element` (`element`, `target`, `position`); `trim_children` (`element`, `keep`, optional
`'from'` = `'start'`, the default, or `'end'`; quote the key, `from` is a reserved word). Form
values, checked state, focus, stylesheets, dialogs and clipboard are not batch operations;
call their commands separately.

```powershell
Invoke-UI4MEBatch -Document $doc -Operations @(
    @{ op = 'set_text'; element = 'count'; text = '3' },
    @{ op = 'add_class'; element = 'count'; name = 'changed' },
    @{ op = 'set_attribute'; element = 'save'; name = 'disabled'; value = '' }
)
```

### Form controls

| Command | Description |
| --- | --- |
| `Get-UI4MEValue -Document -Element` / `Set-UI4MEDocumentValue -Document -Element -Value` | Value of an `<input>`, `<textarea>` or `<select>`, as a string. Setting it is a programmatic change, not simulated typing. |
| `Get-UI4MEChecked -Document -Element` / `Set-UI4MEChecked -Document -Element -Checked` | Checkbox or radio state, as `[bool]` (`-Checked $true`). |
| `Get-UI4MESelectOptions -Document -Element` | A `<select>`'s options (`value`, `label`, `selected`, `disabled`). |
| `Set-UI4MESelectedValues -Document -Element -Values` | Select options by value (`@()` clears). |
| `Get-UI4MEFormData -Document (-Form\|-FormNode) [-Submitter\|-SubmitterNode]` | Ordered form `entries` (repeated names and file metadata preserved). `-Form` takes the form's id. |
| `Submit-UI4MEForm -Document (-Form\|-FormNode) [-Submitter\|-SubmitterNode]` | Request submission like a user would: validation, then a `submit` event. An invalid form is not submitted. |
| `Reset-UI4MEForm -Document (-Form\|-FormNode)` | Reset controls to their defaults. |

No typed command (use [`Invoke-UI4MEDocumentCommand`](#operations-without-a-typed-command)):
`get_files` (paths chosen through an `<input type="file">`), `get_validity`,
`check_validity`, `set_custom_validity` (an empty `message` clears it).

### Focus, selection and editing

| Command | Description |
| --- | --- |
| `Set-UI4MEFocus -Document (-Element\|-Node)` | Move keyboard focus inside the document. |
| `Set-UI4MEFocus -Document -Blur [-Element\|-Node]` | Remove focus; without `-Element`/`-Node`, from whatever has focus. |
| `Get-UI4MEFocusedElement -Document` | The focused element (`node`, `element`), or `$null`. |
| `Get-UI4MESelection -Document (-Element\|-Node)` | A text control's selection (`start`, `end`, `direction`, `length`, `node`). |
| `Set-UI4MESelection -Document (-Element\|-Node) -Start -End [-Direction none\|forward\|backward]` | Select a range. |
| `Invoke-UI4MEEditCommand -Document -Command [-Element\|-Node]` | Native editing command (`undo`, `redo`, `cut`, `copy`, `paste`, `selectAll`, `delete`, ...) through the editor's history and IME path, on the given or focused control. |

No typed command: `element_at_point` and `replace_control_range` (replace text through the
native editor, keeping undo/redo).

**Selection offsets are UTF-16 code units.** That is also how .NET strings count, so `start`
and `end` line up with `$text.Substring(...)`: an emoji counts as 2 in both.

Clipboard edit commands complete asynchronously: acceptance does not mean the operating
system finished the transfer. A later failure arrives as an [`error` event](#events).

### Styles and appearance

| Command | Description |
| --- | --- |
| `Add-UI4MEStylesheet -Document -Css` | Add a stylesheet; returns `{ sheet, diagnostics }`; keep `sheet`, a numeric handle. |
| `Set-UI4MEStylesheet -Document -Sheet -Css` | Replace a stylesheet's CSS. |
| `Set-UI4MEStylesheet -Document -Sheet -Remove` | Remove it. |
| `Get-UI4MEStylesheets -Document` | List stylesheets. |
| `Get-UI4MEComputedStyle -Document -Properties [-Element\|-Node]` | Computed values of the named CSS properties for an element or node; each reports a `status` of `supported`, `unsupported` or `invalid`. |
| `Get-UI4MEEnvironment -Document` | Media preferences in force and their source: `color_scheme`, `reduced_motion`, `high_contrast`, `scale`, ... |
| `Set-UI4MEEnvironment -Document [-ColorScheme light\|dark\|system] [-ReducedMotion reduce\|no-preference\|system]` | Override `prefers-color-scheme` / `prefers-reduced-motion` for this document; `system` returns to the desktop setting. |

See [HTML/CSS scope](https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md) for supported CSS.

### Scrolling, dialogs and popovers

| Command | Description |
| --- | --- |
| `Get-UI4MEScroll -Document [-Element]` | Scroll offset and range of the viewport or an element. |
| `Set-UI4MEScroll -Document [-Element] [-X] [-Y] [-Behavior auto\|instant\|smooth]` | Scroll to a position (CSS px). |
| `Show-UI4MEHtmlDialog -Document -Element [-Modal $true]` | Show an HTML `<dialog>` (modal by default). |
| `Show-UI4MEHtmlDialog -Document -Element -Close [-ReturnValue]` | Close it. |
| `Show-UI4MEPopover -Document -Element [-Action show\|hide\|toggle]` | Show, hide or toggle a popover; `toggle` returns the new open state. |

No typed command: `scroll_into_view` (see the example below). HTML `<dialog>` elements are
part of the page. Native operating-system dialogs are `Show-UI4MEFileDialog`.

### Resource bundles

| Command | Description |
| --- | --- |
| `Register-UI4MEResources -Document -Name -Entries @{ 'icon.png' = @{ Bytes = $bytes; Mime = 'image/png' } }` | Atomically replace the document-local bundle `ui4me-res://NAME/`. Values are `[byte[]]` or `@{ Bytes; Mime }`. |
| `Unregister-UI4MEResources -Document -Name` | Remove a bundle. |
| `Get-UI4MEResourceStats -Document` | Resource cache statistics. |
| `Clear-UI4MEUnusedResources -Document [-Url]` | Release cached resources that nothing uses (or one URL). |
| `New-UI4MELiveBitmap -Document ...` | Create a [live bitmap](#live-bitmaps) for `<img src="ui4me-bitmap://ID">`. |

Resource paths are relative keys with `/` separators (no `..`, no absolute paths). One
registration may hold at most 256 entries and 1 MiB of data; a document may hold up to
16 MiB in 64 bundles. A rejected registration leaves the previous bundle working.

### Node lookup

| Command | Description |
| --- | --- |
| `Get-UI4MENode -Document [-Element] [-Reference]` | Resolve an element id (or, with neither parameter, the root) to a `UI4ME.Node`, or wrap a node reference string returned by the host. |
| `Get-UI4MENodes -Document -Selector [-Offset 0] [-Limit 1024]` | Node references for a selector (`nodes`, `total`, `offset`), including elements without ids. |
| `Invoke-UI4MENodeBatch -Document -Operations @(@{ op = ...; node = ... }, ...)` | Atomic batch addressed by node reference (see [Nodes](#nodes)). |

### Operations without a typed command

Every document operation is reachable with `Invoke-UI4MEDocumentCommand`, for example:

```powershell
Invoke-UI4MEDocumentCommand -Document $doc -Operation scroll_into_view -Parameters @{ element = 'row-40' }
$html = Invoke-UI4MEDocumentCommand -Document $doc -Operation get_html -Parameters @{ element = 'status'; outer = $true }
```

Operations with no typed command: `element_at_point`, `replace_control_range`,
`set_custom_validity`, `get_validity`, `check_validity`, `get_files`, `scroll_into_view`,
`scroll_by`, `get_rect`, `get_html`, `text_rects`, `text_at_point`, `get_attribute`,
`get_attributes`, `has_element`, `get_dialog`, `get_popover`, `set_pointer_capture`,
`get_pointer_capture`, `get_selection`, `clear_selection`, `select_text`, `copy_selection`,
`set_scroll_follow`, `get_accessibility_tree`, `css_profile`, `explain`, `move_element`,
`append_text`, `trim_children`. Host methods without a typed command (use
[`Invoke-UI4MERpc`](#raw-protocol)): `host.diagnostics`, `desktop.paths`, `app.force_quit`.
See the [protocol overview](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/protocol.md).

## Events

| Command | Description |
| --- | --- |
| `Register-UI4MEDocumentEvent -Document -Type -Callback [-Element]` | Subscribe to an event type. With `-Element`, the callback runs for events whose target is that element **or** that bubble through it (its id is on the event's `path`), so one handler on a list container sees clicks on its items. Returns a `UI4ME.Subscription`; at most 256 per document. |
| `Unregister-UI4MEDocumentEvent -Subscription` | Remove a subscription. |
| `Set-UI4MEPreventDefault -Document -Element -EventType [-Prevent $true] [-Key] [-Shift] [-Ctrl] [-Alt] [-Meta]` | Register a default-action rule in advance (see below). Alias: `-Event`. |

Common event types: `click`, `dblclick`, `auxclick`, `contextmenu`, `input`, `change`,
`focus`, `blur`, `focusin`, `focusout`, `keydown`, `keyup`, pointer events (`pointerdown`,
`pointerup`, `pointermove`, ...), `wheel`, `scroll`, `submit`, `reset`, `invalid`, `close`
(HTML dialogs), popover toggles, composition events, CSS transition/animation events, `drop`,
and the UI4ME events `link_activated` and `environment_changed`. The
[HTML/CSS scope](https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md#events-delivered-to-the-backend) page lists
them all. Text editing emits one `input` per edit and one `change` when the value is
committed (for example when the field loses focus).

Event callbacks receive the event object as their first argument (`param($e)`). Every event
has:

| Field | Meaning |
| --- | --- |
| `type` | Event type. |
| `element` | ID of the nearest element with an `id`, starting from the target (`''` if none). |
| `path` | IDs of identified elements the event passes through, innermost first. |
| `node` | Opaque node reference of the actual target. |
| `shift`, `ctrl`, `alt`, `meta` | Modifier keys. |
| `cancelable` | Whether the event has a default action. |
| `window_id`, `document_id` | Added by the module. |

Depending on the type and target: `value`, `checked`, `selection`, `files` (controls); `key`,
`code`, `repeat`, `is_composing` (keyboard); `x`, `y`, `button`, `buttons`, `pointer_id`,
`pointer_type`, `pressure` (pointer, viewport CSS pixels); `delta_x`, `delta_y` (wheel);
`data`, `input_type` (input); `submitter` (submit); `validity` (invalid); `return_value`
(dialog close); `default_prevented`.

**Events arrive after the native action.** By the time a `click` or `keydown` callback runs,
the host has already performed the default action (the character is typed, the form is
submitted, the link is followed). A callback cannot cancel it. To suppress a default action,
register a rule in advance with `Set-UI4MEPreventDefault`. `-Key` and the modifier filters
(`$true`/`$false`) narrow the rule; `-Prevent $false` removes it. Only default actions the
engine supports can be prevented (such as keyboard shortcuts, `submit`, `contextmenu` and
dialog `cancel`). A document holds at most 256 rules.

```powershell
Set-UI4MEPreventDefault -Document $doc -Element editor -EventType keydown -Key s -Ctrl $true   # Ctrl+S no longer types

$null = Register-UI4MEDocumentEvent -Document $doc -Type keydown -Element editor -Callback {
    param($e)
    if ($e.ctrl -and $e.key -eq 's') {
        Set-UI4MEDocumentText -Document $doc -Element status -Text 'Saved'
    }
}
```

**Errors.** `Register-UI4MEDocumentEvent -Type error` receives asynchronous failures for this
document (for example a failed clipboard transfer) as `{ type = 'error'; message; fatal }`.
`fatal` means the host unloaded the document after a rendering failure; the window stays
open, and you may load a replacement from the callback.

**Links never navigate.** Activating an `<a href>` sends a `link_activated` event with `href`
and the resolved `url`; decide in PowerShell what to do, for example
`Open-UI4MEExternal -Application $app -Target $e.url` for a web link or `Open-UI4MEFile` for
another local page.

**Drag and drop from other applications.** Mark a receiving element with
`data-ui4me-drop="files"`, `"text"` or `"files text"`; `"none"` blocks a subtree. A completed
drop arrives as a `drop` event with `x`, `y`, `external = $true` and either `paths` (local
file paths) or `text`.

## Nodes

A `UI4ME.Node` is an opaque, generation-bound reference to one node in one document,
including elements without an `id` and text nodes. Get one from `Get-UI4MENode`,
`Get-UI4MENodes` (wrap the `node` string with `Get-UI4MENode -Reference`), or an event's
`node` field. Never build reference strings yourself.

| Command | Description |
| --- | --- |
| `$node.Id` | Reference string. |
| `Get-UI4MENodeInfo -Node` | Describe a node: type, name, `id`, attributes or text, parent and child count. |
| `Get-UI4MENodeChildren -Node [-Offset 0] [-Limit 256]` | A page of child nodes (`children`, `total`); limit 1-4096. |
| `Invoke-UI4MENodeOperation -Node -Operation [-Parameters @{}]` | One node operation: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `trim_children`, `move_element` (`target_node` may be a `UI4ME.Node`), `remove_element`. |
| `Invoke-UI4MENodeBatch -Document -Operations @(@{ op = ...; node = ... }, ...)` | Several node operations atomically (node references as strings, `$node.Id`). |
| `Set-UI4MEFocus -Document -Node` / `Set-UI4MEFocus -Document -Blur -Node` | Focus. |

`Invoke-UI4MENodeBatch` takes the same operations as `Invoke-UI4MEBatch`, with `node` instead
of `element` and `target_node` instead of `target`. A node keeps its identity when moved but
not after removal. The host rejects a node reference from an earlier document generation even
if you bypass the local check.

## Live bitmaps

For pixels that change continuously (previews, plots, video-like output), use a live bitmap
instead of re-encoding images. The pixels go through a shared-memory file, not through JSON.

```html
<img id="preview" src="ui4me-bitmap://preview" width="320" height="200" alt="Preview">
```

```powershell
$bitmap = New-UI4MELiveBitmap -Document $doc -Id preview -Width 320 -Height 200 -Slots 2
$frame = New-Object byte[] (320 * 200 * 4)          # RGBA, straight alpha, tight rows
for ($i = 0; $i -lt $frame.Length; $i += 4) { $frame[$i] = 30; $frame[$i + 1] = 120; $frame[$i + 2] = 220; $frame[$i + 3] = 255 }
$null = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame   # returns when the host took it
```

| Command | Description |
| --- | --- |
| `New-UI4MELiveBitmap -Document -Id -Width -Height [-Slots 2]` | Create a live image for `<img src="ui4me-bitmap://ID">`. |
| `Send-UI4MELiveBitmap -Bitmap -Pixels` | Write one complete frame and present it. Returns once the host has taken the frame, so a fast producer is paced by the host. |
| `Send-UI4MELiveBitmap -Bitmap -Pixels -Try` | Same, but returns `$null` instead of failing when a present of the same bitmap is already in progress (for example from a callback that runs during it). |
| `Remove-UI4MELiveBitmap -Bitmap [-Quiet]` | Unmap and release. Idempotent. |
| `$bitmap.Sequence` | Number of the last presented frame. |

A frame is straight-alpha RGBA8, row by row, exactly `width*height*4` bytes as a `[byte[]]`.
`-Id` is 1-64 of `A-Z a-z 0-9 _ -`; each side is 1-4096 pixels; `-Slots` is 2 or 3. A document
can have 8 bitmaps using up to 64 MiB together. Frames of the same size update the image
without re-running layout. A bitmap belongs to its document: replacing or closing the
document releases it automatically.

Per-pixel PowerShell loops (as in the one-off frame above) are slow; for animation build
frames from precomputed rows with `[Buffer]::BlockCopy` or `[Array]::Copy`, or produce them
with a .NET library, and present them from a loop around `Wait-UI4ME -TimeoutMilliseconds`;
see the [live bitmap recipe](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns-powershell.md#13-stream-pixels-with-a-live-bitmap).
The host creates the shared file in `$XDG_RUNTIME_DIR` (Linux) or `%TEMP%` (Windows) and
removes it once both sides have mapped or closed it.

## Errors

| Exception | When |
| --- | --- |
| `UI4ME.RpcException` | The host refused a request. Properties: `Code`, `RpcMessage`, `RpcData`, `Method` (also `Exception.Data['RpcCode']`). |
| `UI4ME.StaleObjectException` | A window, document, node, bitmap or request handle is no longer current. |
| `UI4ME.RpcTimeoutException` | A wait expired. Cancellation was requested; **the native outcome is unknown**. |
| `UI4ME.DisconnectedException` | The host connection closed. `Wait-UI4ME -UntilShutdown` throws it only if the host disappeared without an orderly shutdown. |
| Startup errors (plain exceptions) | `Start-UI4MEScript` cannot run: not called from a `.ps1` file; host binary missing; only one of `UI4ME_ENDPOINT`/`UI4ME_TOKEN` set; host failed to launch. Also `Connect-UI4ME` without a host connection. |
| Parameter errors | Invalid arguments detected locally (parameter validation). |

```powershell
try { Set-UI4MEDocumentText -Document $doc -Element missing -Text x }
catch [UI4ME.RpcException] { "Host refused: $($_.Exception.Code) $($_.Exception.RpcMessage)" }
```

Common codes: `-32602` invalid parameters (including unknown element ids), `-32601` unknown
method or operation, `-32001` not supported on this platform, `-32002` unavailable right now,
`-32003` refused by policy, `-32004` closed window or document, `-32005` busy or bounded,
`-32006` cancelled, `-32008` invalid state (for example during shutdown). Platform-dependent
features such as Wayland window positioning fail with an explicit error instead of pretending
to work, so catch `UI4ME.RpcException` around them.

Errors thrown inside event and notification callbacks are written to standard error (shown
by the launcher, or in the host log) as `UI4ME: event handler for 'click' failed: ...`; the
application keeps running. A policy callback that throws counts as a failed decision: the
window stays open or the application keeps running.

## Callbacks, ordering and closures

All callbacks run on the backend's own PowerShell runspace, never on a .NET thread-pool
thread. A background reader thread only parses and queues messages; they are processed when
your script **pumps**: inside `Wait-UI4ME`, and inside any command that waits for a host
reply.

- **Document and notification callbacks run one at a time, in host order.** If a callback
  issues commands (which pump while waiting), later notifications are deferred until it
  returns; they never run nested inside it.
- A long-running callback delays later events. Keep callbacks short; split long work into
  steps that run between `Wait-UI4ME -TimeoutMilliseconds ...` calls, or run it in a job.
- **Lifecycle requests are still answered while a callback waits.** `Set-UI4MEBeforeClose`
  and `Set-UI4MEBeforeQuit` policies and `app.shutdown` are handled inside such waits. A
  click handler that waits for a native dialog therefore does not stop the user from closing
  the window; the host cancels the dialog and the handler gets an error.
- Do not call UI4ME commands from other runspaces, jobs or threads.
- An event is a snapshot. If the page changed after the event was queued, its `element` ID
  may no longer exist; do not treat it as proof that the element is still there.

Event callbacks receive the event object as their first argument (`param($e)`). Callbacks run
**after** native dispatch (the text was typed, the button was pressed); use
`Set-UI4MEPreventDefault` beforehand to stop a default action.

**Scriptblocks and variables.** A scriptblock defined at the top level of the script sees the
script's variables when it runs (`$doc`, `$app`, `$script:counter`). A scriptblock defined
*inside a function* loses the function's local variables once the function returns. Capture
them by calling `.GetNewClosure()` **in that function**:

```powershell
function Add-SaveHandler($Document, [string]$Label) {
    $log = $script:log                   # capture objects you will use as locals
    $null = Register-UI4MEDocumentEvent -Document $Document -Type click -Element save -Callback ({
        param($e)
        [void]$log.Add("$Label clicked")  # $Label and $log were captured
    }.GetNewClosure())
}
```

Inside a `.GetNewClosure()` scriptblock, `$script:` refers to the closure's own new scope,
not your script. Pass collections or other objects in as captured locals (as above). To
*assign* a script variable from a top-level callback, write `$script:name = ...` (or keep the
state in a hashtable).

**Policy callbacks** (`Set-UI4MEBeforeClose`, `Set-UI4MEBeforeQuit`) decide by their *last*
output value: `$true` allows, `$false` or no output vetoes. Earlier output in the block is
ignored. A policy callback that throws counts as a failed decision.

## Raw protocol

| Command | Description |
| --- | --- |
| `Invoke-UI4MERpc -Application -Method [-Parameters @{}] [-TimeoutMilliseconds 15000]` | Call any host method and return its result. `dialog.*` methods wait without a deadline by default; `-1` waits forever. Events keep being processed while waiting. |
| `Start-UI4MERpc -Application -Method [-Parameters @{}]` | Send a request without waiting; returns a `UI4ME.Request`. |
| `Wait-UI4MERpc -Request [-TimeoutMilliseconds 15000]` | Wait for a request's result while pumping. On timeout the request is cancelled and `UI4ME.RpcTimeoutException` is thrown. A request can be awaited once. |
| `Stop-UI4MERpc -Request` | Ask the host to cancel (for example, close a native dialog). Not a rollback. |
| `Send-UI4MENotification -Application -Method [-Parameters @{}]` | Send a notification (no reply). |
| `Register-UI4MENotification -Application -Method -Callback` | One callback per host notification method (see [Application commands](#application-commands)). |
| `$app.ShutdownRequested`, `$app.Transport.DroppedNotifications` | Shutdown requested by the host; notifications dropped because the bounded queue overflowed. |

The wire protocol is described in [protocol](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/protocol.md); normal applications do not need it.

## Environment variables

| Variable | Meaning |
| --- | --- |
| `UI4ME_ENDPOINT`, `UI4ME_TOKEN` | Set by the host for its backend only. Never set them yourself. |
| `UI4ME_HOST_BINARY` | Use this native host instead of the bundled one (for example a source build). Point it only at a trusted file. |
| `UI4ME_VERBOSE` | `1` makes the `Start-UI4MEScript` launcher print every host log record, not only warnings and errors. |
| `UI4ME_TEST_MODE` | `1` runs the host headless (no windows on screen), for protocol tests without a desktop. A headless app does not quit on its own; your test must call `Request-UI4MEQuit`. Testing aid only. |

## Limits

| Bound | Value |
| --- | --- |
| Frame size (either direction) | 2 MiB after JSON encoding |
| HTML source per load | 1 MiB |
| Text per `Set-UI4MEDocumentText`/`Set-UI4MEDocumentValue`/`append_text` | 16 KiB |
| Nodes / nesting depth | 65,536 / 256 |
| Stylesheets added at runtime | 64, each up to 1 MiB |
| Default-action rules | 256 per document |
| Live documents per host | 16 |
| Batch / node batch | 1024 operations |
| Selector query page (`Find-UI4MEElement`, `Get-UI4MENodes`, `Get-UI4MENodeChildren`) | 1-4096 entries |
| Event subscriptions per document | 256 |
| Outstanding requests | 128 |
| Queued host notifications/requests | 128 messages and 8 MiB. Overflowing notifications are dropped and counted (`$app.Transport.DroppedNotifications`); overflowing requests get error `-32005`. Window close, quit and shutdown requests are always admitted. |
| Deferred notifications during one callback | 128 (excess counted in `$app.DroppedNotifications`) |
| Nested callback depth | 32 |
| Resource registration | 256 entries, 1 MiB per call; 16 MiB, 64 bundles per document |
| Live bitmaps | 8 per document, 64 MiB total, 4096 px per side, 2 or 3 slots |
| File dialog result | 1024 paths |

A result that would exceed the message limit is an error, never silently truncated; use
paging (`-Offset`/`-Limit`) instead.

## Hosting, projects and agent skills

| Command | Description |
| --- | --- |
| `New-UI4MEProject -Path [-Name 'UI4ME PowerShell App'] [-AppId org.example.PowerShellApp] [-WindowsPowerShell] [-Force]` | Generate `app.ps1` (uses `Start-UI4MEScript`; run it directly), `index.html`, `style.css` and an optional `app.toml` whose backend runs `app.ps1` with `pwsh`, or `powershell.exe` with `-WindowsPowerShell`. `app.ps1` is saved as UTF-8 with a BOM. Refuses to overwrite these files without `-Force`. Returns the folder. |
| `Start-UI4MEHost -Manifest [-NoWait]` | Run an `app.toml` through the native host and wait for it to exit (throws on a nonzero exit code). `-NoWait` returns the host process object instead. |
| `Get-UI4MEHostPath` | Path of the bundled native host for this platform (`UI4ME_HOST_BINARY` overrides it). Restores the Linux execute bit if package extraction dropped it. x64 only. Run `& (Get-UI4MEHostPath) doctor` to check the runtime and desktop session, or `--version` for the host version. |
| `Install-UI4MESkill [-Action install\|status\|uninstall] [-Agents all] [-Scope user\|project] [-ProjectPath] [-Force] [-DryRun]` | Install, inspect or remove the bundled `ui4me-powershell` coding-agent skill. `-Agents` takes `all` or any of `claude`, `codex`, `opencode`, `copilot`, `cursor`, `gemini` (array or comma list). `-ProjectPath` (an existing folder) is required with `-Scope project`. Never replaces or removes a folder it did not install, or that was edited since, without `-Force`. `-DryRun` reports what it would do. Returns one object per agent (`Agent`, `Scope`, `State`, `Path`, `Action`). See [agent skills](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/agent-skills.md). |

Skill locations: user scope `~/.claude/skills`, `~/.agents/skills` (Codex),
`~/.config/opencode/skills`, `~/.copilot/skills`, `~/.cursor/skills`, `~/.gemini/skills`;
project scope `.claude/skills`, `.agents/skills`, `.opencode/skills`, `.github/skills`,
`.cursor/skills`, `.gemini/skills`. Each gets a `ui4me-powershell` folder.

## Parity with the Python API

| Python (`ui4me_core`) | PowerShell |
| --- | --- |
| `Application.start()` | `Start-UI4MEScript` |
| `Application.from_environment()` / `run()` | `Connect-UI4ME`; `Set-UI4MEReady` + `Wait-UI4ME -UntilShutdown` |
| `app.close()` / `app.on_shutdown` | `Close-UI4ME` / code after `Wait-UI4ME -UntilShutdown` |
| `app.create_window(...)` | `New-UI4MEWindow` |
| `app.open_file` / `open_html` | `New-UI4MEWindow` + `Open-UI4MEFile` / `Open-UI4MEHtml` |
| `window.load_file` / `load_html` / `set` / `state` / `request_close` / `request_attention` / `on_before_close` | `Open-UI4MEFile` / `Open-UI4MEHtml` / `Set-UI4MEWindow` / `Get-UI4MEWindow` / `Close-UI4MEWindow` / `Request-UI4MEAttention` / `Set-UI4MEBeforeClose` |
| `window.open_files` / `save_file` / `choose_folder` (`wait=False`) | `Show-UI4MEFileDialog -Kind ...` (`-NoWait`) |
| `app.rpc.call("window.list")` | `Get-UI4MEWindowList` |
| `app.request_quit` / `on_before_quit` / `ready` | `Request-UI4MEQuit` / `Set-UI4MEBeforeQuit` / `Set-UI4MEReady` |
| `app.on_window_event` / `app.rpc.on_notification` | `Register-UI4MENotification -Method window.event` / `Register-UI4MENotification` |
| `app.clipboard_text` / `set_clipboard_text` | `Get-UI4MEClipboard` / `Set-UI4MEClipboard` |
| `app.capabilities` / `desktop_preferences` / `on_desktop_preferences` | `Get-UI4MECapabilities` / `Get-UI4MEPreferences` / `Register-UI4MENotification -Method desktop.preferences_changed` |
| `app.open_external` / `reveal_file` | `Open-UI4MEExternal` / `Open-UI4MEExternal -Reveal` |
| `doc.command(op, ...)` / `doc.on(...)` + unsubscribe function / `doc.batch` / `doc.unload` | `Invoke-UI4MEDocumentCommand` / `Register-UI4MEDocumentEvent` + `Unregister-UI4MEDocumentEvent` / `Invoke-UI4MEBatch` / `Remove-UI4MEDocument` |
| `set_text`, `get_text`, `set_value`, `get_value`, `set_checked`, `get_checked` | `Set-`/`Get-UI4MEDocumentText`, `Set-UI4MEDocumentValue`/`Get-UI4MEValue`, `Set-`/`Get-UI4MEChecked` |
| `set_attribute`, `remove_attribute`, `add_class`, `remove_class` | `Set-UI4MEDocumentAttribute [-Remove]`, `Set-UI4MEClass [-Remove]` |
| `replace_html`, `insert_html`, `remove`, `query` | `Set-UI4MEDocumentHtml [-Position]`, `Remove-UI4MEElement`, `Find-UI4MEElement` |
| `focus`, `blur`, `focused_element`, `prevent_default` | `Set-UI4MEFocus [-Blur]`, `Get-UI4MEFocusedElement`, `Set-UI4MEPreventDefault` |
| `control_selection`, `set_control_selection`, `edit` | `Get-`/`Set-UI4MESelection`, `Invoke-UI4MEEditCommand` |
| `form_data`, `request_submit`, `reset_form`, `options`, `set_selected_values` | `Get-UI4MEFormData`, `Submit-UI4MEForm`, `Reset-UI4MEForm`, `Get-UI4MESelectOptions`, `Set-UI4MESelectedValues` |
| `element_at_point`, `replace_control_range`, `files`, `validity`, `check_validity`, `set_custom_validity`, `scroll_into_view` | No typed command: `Invoke-UI4MEDocumentCommand -Operation element_at_point` / `replace_control_range` / `get_files` / `get_validity` / `check_validity` / `set_custom_validity` / `scroll_into_view` |
| `add_stylesheet`, `replace_stylesheet`, `remove_stylesheet`, `stylesheets`, `computed_style` | `Add-UI4MEStylesheet`, `Set-UI4MEStylesheet [-Remove]`, `Get-UI4MEStylesheets`, `Get-UI4MEComputedStyle` |
| `environment`, `set_environment`, `scroll`, `scroll_to` | `Get-`/`Set-UI4MEEnvironment`, `Get-`/`Set-UI4MEScroll` |
| `show_dialog`, `close_dialog`, `show_popover`, `hide_popover`, `toggle_popover` | `Show-UI4MEHtmlDialog [-Close]`, `Show-UI4MEPopover -Action` |
| `register_resources`, `unregister_resources`, `resource_stats`, `purge_unused_resources`, `diagnostics` | `Register-`/`Unregister-UI4MEResources`, `Get-UI4MEResourceStats`, `Clear-UI4MEUnusedResources`, `Get-UI4MEDiagnostics` |
| `node`, `query_nodes`, `node_batch`, `Node.inspect` / `children` / `set_text` / ... / `move_to` | `Get-UI4MENode`, `Get-UI4MENodes`, `Invoke-UI4MENodeBatch`, `Get-UI4MENodeInfo` / `Get-UI4MENodeChildren` / `Invoke-UI4MENodeOperation` |
| `create_bitmap`, `LiveBitmap.present` / `try_present` / `close` | `New-UI4MELiveBitmap`, `Send-UI4MELiveBitmap [-Try]`, `Remove-UI4MELiveBitmap` |
| `RpcClient.call` / `call_async` / `future.cancel()` / `notify` | `Invoke-UI4MERpc` / `Start-UI4MERpc` + `Wait-UI4MERpc` / `Stop-UI4MERpc` / `Send-UI4MENotification` |
| `ui4me_core.host_binary()` / `ui4me-host run --app` | `Get-UI4MEHostPath` / `Start-UI4MEHost -Manifest` |
| `ui4me-new` / `ui4me-skill` | `New-UI4MEProject` / `Install-UI4MESkill` |
| A worker `threading.Thread` | `Wait-UI4ME -TimeoutMilliseconds` in your own loop, or a job (PowerShell commands must stay on the script's runspace) |

Differences by design: PowerShell callbacks are serialized on one runspace (Python runs
lifecycle policies on separate threads; both answer them while an event callback waits);
policy callbacks decide by their last output value (Python: by their return value); the
timeout exception is `UI4ME.RpcTimeoutException` (Python: `TimeoutError`).

Source: [UI4ME.Core.psm1](https://github.com/ui4me/ui4me-core/blob/main/powershell/UI4ME.Core/UI4ME.Core.psm1), [UI4ME.Transport.cs](https://github.com/ui4me/ui4me-core/blob/main/powershell/UI4ME.Core/private/UI4ME.Transport.cs).
