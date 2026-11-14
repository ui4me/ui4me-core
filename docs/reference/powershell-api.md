# PowerShell command reference (UI4ME.Core 1.0.1)

`UI4ME.Core` is the PowerShell binding for the UI4ME Core native host. The host (one Rust
process) owns the native windows, input, IME, HTML/CSS layout and rendering, accessibility and
dialogs. Your PowerShell script is the application backend: it runs as a separate process that
the host supervises, and the two talk over a private, authenticated, length-prefixed JSON-RPC
channel (a Unix-domain socket on Linux, a named pipe on Windows). No browser engine,
JavaScript, Python, Node.js or HTTP server is involved.

Supported shells: **PowerShell 7.2+** on Linux x64 and Windows x64, and **Windows PowerShell
5.1** on Windows x64. New to UI4ME? Start with the
[PowerShell quickstart](../guides/quickstart-powershell.md).

The module exports 83 functions. `Get-Help <command>` shows each one's help, and
`(Get-Command <command>).Parameters.Keys` lists its parameters.

- [Program structure](#program-structure)
- [Objects and results](#objects-and-results)
- [Callbacks, ordering and closures](#callbacks-ordering-and-closures)
- [Errors](#errors)
- [Commands](#commands)
- [Parity with the Python API](#parity-with-the-python-api)
- [Environment variables](#environment-variables)
- [Limits](#limits)

## Program structure

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
- **Lifetime.** Ctrl+C in the launcher stops the host (exit code 130). If the launcher
  process dies, the host is stopped too (a kill-on-close job on Windows; `setpriv
  --pdeathsig` on Linux when `setpriv` is available).
- **Single instance.** With `-Instance single` (the default), launching the same app again
  while it runs forwards the launch to the running app, which receives an `app.activated`
  notification (observe it with `Register-UI4MENotification`); the second launch exits with
  code 0. `-Instance multiple` allows several copies.
- **App identity.** Without `-AppId`, the identity is a stable `local.ui4me.<name>.h<hash>`
  derived from the script path. Pass a reverse-DNS `-AppId` such as `org.example.MyApp` for
  real applications.
- **Errors.** An uncaught error in the backend ends it: PowerShell prints the error, the host
  shows its application-error window, and the launcher exits with a nonzero code. In a
  `Start-UI4MEScript` backend, a premature `Close-UI4ME` (no shutdown was requested yet, for
  example in a `finally` block while an error unwinds) retires the handles but leaves the
  connection open until the process exits, so the error is printed before the host stops the
  failed backend.
- Inside a backend that was started from a manifest (`Start-UI4MEHost -Manifest`,
  `ui4me-host run --app`), `Start-UI4MEScript` simply connects, so the same script works
  both ways.

### Manifest-based apps

For launchers, installers and deployments that start the host explicitly, an `app.toml`
names the backend program. The backend connects with `Connect-UI4ME` or `Start-UI4MEScript`:

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
to `app.ps1`. See [host CLI](host-cli.md) for the manifest fields.

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

## Callbacks, ordering and closures

All callbacks run on the backend's own PowerShell runspace, never on a .NET thread-pool
thread. A background reader thread only parses and queues messages; they are processed when
your script **pumps**: inside `Wait-UI4ME`, and inside any command that waits for a host
reply.

- **Document and notification callbacks run one at a time, in host order.** If a callback
  issues commands (which pump while waiting), later notifications are deferred until it
  returns; they never run nested inside it.
- **Lifecycle requests are still answered while a callback waits.** `Set-UI4MEBeforeClose`
  and `Set-UI4MEBeforeQuit` policies and `app.shutdown` are handled inside such waits. A
  click handler that waits for a native dialog therefore does not stop the user from closing
  the window; the host cancels the dialog and the handler gets an error.
- A long-running callback delays later events. Keep callbacks short; split long work into
  steps that run between `Wait-UI4ME -TimeoutMilliseconds ...` calls.
- Do not call UI4ME commands from other runspaces, jobs or threads.

Event callbacks receive the event object as their first argument (`param($e)`): `type`,
`element` (nearest element id), `path` (ids along the event route), type-specific fields such
as `value`, `button`, `x`, `y`, `key` and modifier flags, plus `window_id` and `document_id`.
Callbacks run **after** native dispatch (the text was typed, the button was pressed); use
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
not your script. Pass collections or other objects in as captured locals (as above).

**Policy callbacks** (`Set-UI4MEBeforeClose`, `Set-UI4MEBeforeQuit`) decide by their *last*
output value: `$true` allows, `$false` or no output vetoes. Earlier output in the block is
ignored. A policy callback that throws counts as a failed decision.

## Errors

| Exception | When |
| --- | --- |
| `UI4ME.RpcException` | The host refused a request. Properties: `Code`, `RpcMessage`, `RpcData`, `Method` (also `Exception.Data['RpcCode']`). |
| `UI4ME.StaleObjectException` | A window, document, node, bitmap or request handle is no longer current. |
| `UI4ME.RpcTimeoutException` | A wait expired. Cancellation was requested; **the native outcome is unknown**. |
| `UI4ME.DisconnectedException` | The host connection closed. `Wait-UI4ME -UntilShutdown` throws it only if the host disappeared without an orderly shutdown. |

```powershell
try { Set-UI4MEDocumentText -Document $doc -Element missing -Text x }
catch [UI4ME.RpcException] { "Host refused: $($_.Exception.Code) $($_.Exception.RpcMessage)" }
```

Common codes: `-32602` invalid parameters (including unknown element ids), `-32601` unknown
method or operation, `-32004` closed window or document, `-32005` busy or bounded, `-32008`
invalid state (for example during shutdown), `-32003` refused by policy, `-32001` not
supported on this platform.

Errors thrown inside event and notification callbacks are written to standard error (shown
by the launcher, or in the host log) as `UI4ME: event handler for 'click' failed: ...`; the
application keeps running.

## Commands

Parameters in brackets are optional. `-Element` takes an HTML `id`. Commands that also
accept `-Node` take a `UI4ME.Node` from the same document; pass either `-Element` or
`-Node`, not both.

### Starting, connecting and pumping

| Command | Description |
| --- | --- |
| `Start-UI4MEScript [-Name] [-AppId] [-Version] [-Icon] [-Instance single\|multiple] [-StartupTimeoutMilliseconds 30000] [-ShutdownTimeoutMilliseconds 5000] [-MaxWindows 16] [-ArgumentList] [-ScriptPath]` | Run the calling `.ps1` as an app: in the launcher, start the native host and exit with its exit code; in the backend, connect and return the `UI4ME.Application`. See [How `Start-UI4MEScript` starts an app](#how-start-ui4mescript-starts-an-app). `-Name` defaults to the script file name and `-Version` is optional (each 1-128 characters, no control characters); `-Icon` is an image file path; the startup timeout accepts 1000-300000 ms, the shutdown timeout 100-120000 ms, `-MaxWindows` 1-64. `-ScriptPath` names the script when it cannot be detected. |
| `Connect-UI4ME [-Endpoint] [-Token] [-ConnectTimeoutMilliseconds 15000]` | Connect and authenticate (`host.hello`, protocol 1) in a backend started from a manifest. Defaults come from `UI4ME_ENDPOINT` and `UI4ME_TOKEN`, which only the host sets; without them it throws. |
| `Set-UI4MEReady -Application` | Report that startup finished. Call it once, after the first window exists. |
| `Wait-UI4ME -Application -UntilShutdown` | Pump events and run callbacks until the host requests shutdown (the user closed the last window, or a quit succeeded). Throws `UI4ME.DisconnectedException` if the host disappears first. |
| `Wait-UI4ME -Application [-TimeoutMilliseconds 100] [-Once]` | Pump once: wait up to the timeout (`-1` = no limit) for the first message, then handle up to 64 queued messages (`-Once`: at most one). Returns the number handled. Use it in your own loop, for example for animation. |
| `Close-UI4ME -Application` | Close the connection and retire every window, document and bitmap handle. Idempotent; `$null` is accepted. |
| `Request-UI4MEQuit -Application` | Request an orderly quit; the quit policy may veto it. |
| `Set-UI4MEBeforeQuit -Application -Callback { param($request) $true }` | Quit policy (last output decides). `-Callback $null` removes it. |
| `Register-UI4MENotification -Application -Method -Callback { param($params) }` | Run a scriptblock for a host notification such as `app.activated`, `desktop.preferences_changed` or `window.event`. One callback per method; `-Callback $null` removes it. Built-in window and document bookkeeping runs first. |

### Windows

| Command | Description |
| --- | --- |
| `New-UI4MEWindow -Application [-Title] [-Width 900] [-Height 600] [-Visible $true] [-Resizable $true] [-Decorations $true] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight]` | Create a native window (sizes in CSS px, 1-8192). Size constraints are set in pairs. Load content with `Open-UI4MEFile` or `Open-UI4MEHtml`. |
| `Set-UI4MEWindow -Window [-Title] [-Width -Height] [-Visible] [-Resizable] [-Decorations] [-Minimized] [-Maximized] [-Fullscreen] [-Focus] [-X -Y] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight] [-Properties @{}]` | Change window state. Boolean state takes `$true`/`$false` (`-Maximized $true`). `-Properties` passes host field names directly; `@{ min_width = $null; min_height = $null }` clears a constraint. Returns the host result. |
| `Get-UI4MEWindow -Window` | Requested and observed state: `title`, `width`, `height`, `scale_factor`, `physical_size`, `observed.{focused,minimized,maximized,fullscreen,position,theme}`. |
| `Get-UI4MEWindowList -Application` | All open windows of the app. |
| `Close-UI4MEWindow -Window` | Request a graceful close (the before-close policy may veto it). |
| `Request-UI4MEAttention -Window [-Kind informational\|critical\|none]` | Ask the desktop to flag the window; `none` clears the request. |
| `Set-UI4MEBeforeClose -Window -Callback { param($request) $true }` | Close policy for this window (last output decides). `-Callback $null` removes it. |

Platform notes: on Wayland a client cannot focus, position or un-minimize its own window;
`-Focus` is refused there (`-32001`) and `observed.position` is `$null`. Use
`Request-UI4MEAttention` instead. See [platforms](../guides/platforms.md).

### Documents and events

| Command | Description |
| --- | --- |
| `Open-UI4MEFile -Window -Path [-AssetRoot]` | Load an HTML file. Relative stylesheets, images and fonts resolve against its folder (or `-AssetRoot`). Atomic: if loading fails, the previous document stays. Returns the new `UI4ME.Document`; the old one becomes stale. Relative paths resolve against the current PowerShell location, so prefer `Join-Path $PSScriptRoot 'index.html'`. |
| `Open-UI4MEHtml -Window -Html [-AssetRoot]` | Load generated HTML (same rules). Use `-AssetRoot` (for example `$PSScriptRoot`) when the HTML has relative URLs. |
| `Remove-UI4MEDocument -Document` | Unload the document; the window stays open and empty. |
| `Register-UI4MEDocumentEvent -Document -Type -Callback [-Element]` | Subscribe to an event type (`click`, `input`, `change`, `keydown`, `focus`, `submit`, `contextmenu`, `auxclick`, composition events, ...). With `-Element`, only events whose route (`path`) contains that id. Returns a `UI4ME.Subscription`; at most 256 per document. |
| `Unregister-UI4MEDocumentEvent -Subscription` | Remove a subscription. |
| `Invoke-UI4MEDocumentCommand -Document -Operation [-Parameters @{}]` | Run **any** document operation by name and return its result (see [the list below](#operations-without-a-typed-command)). |
| `Invoke-UI4MEBatch -Document -Operations @(@{ op = ...; element = ... }, ...)` | Apply id-addressed operations as one atomic transaction (all or nothing). Operations: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `trim_children`, `move_element`, `remove_element`. |

### Content, attributes and classes

| Command | Description |
| --- | --- |
| `Set-UI4MEDocumentText -Document -Element -Text` | Replace the element's content with text. Never parsed as HTML; safe for user input. |
| `Get-UI4MEDocumentText -Document -Element` | Text content, as a string. |
| `Set-UI4MEDocumentHtml -Document -Element -Html [-Position replace\|beforebegin\|afterbegin\|beforeend\|afterend]` | Replace the children with (default), or insert, **trusted** HTML. Encode untrusted values with `[Net.WebUtility]::HtmlEncode()`. |
| `Remove-UI4MEElement -Document -Element` | Remove an element and its subtree. |
| `Set-UI4MEDocumentAttribute -Document -Element -Name [-Value ''] [-Remove]` | Set or remove an attribute (`-Value ''` sets a boolean attribute such as `disabled`). |
| `Set-UI4MEClass -Document -Element -Name [-Remove]` | Add or remove a class. |
| `Find-UI4MEElement -Document -Selector [-Offset 0] [-Limit 1024]` | CSS selector query. Returns `ids` (element ids of matches that have one), `total`, `offset`, `limit`, `truncated`. Limit 1-4096. |

### Forms, controls and focus

| Command | Description |
| --- | --- |
| `Get-UI4MEValue -Document -Element` | A control's value, as a string. |
| `Set-UI4MEDocumentValue -Document -Element -Value` | Set a control's value. |
| `Get-UI4MEChecked -Document -Element` | Checkbox or radio state, as `[bool]`. |
| `Set-UI4MEChecked -Document -Element -Checked` | Set it (`-Checked $true`). |
| `Get-UI4MESelectOptions -Document -Element` | A `<select>`'s options (`value`, `label`, `selected`, `disabled`). |
| `Set-UI4MESelectedValues -Document -Element -Values` | Select options by value (`@()` clears). |
| `Get-UI4MESelection -Document (-Element\|-Node)` | A text control's selection, in UTF-16 code units. |
| `Set-UI4MESelection -Document (-Element\|-Node) -Start -End [-Direction none\|forward\|backward]` | Set it. |
| `Invoke-UI4MEEditCommand -Document -Command [-Element\|-Node]` | Native editing command (`undo`, `redo`, `cut`, `copy`, `paste`, `selectAll`, `delete`, ...) through the editor's history and IME path. Clipboard commands complete asynchronously; acceptance does not mean the OS transfer succeeded. |
| `Get-UI4MEFormData -Document (-Form\|-FormNode) [-Submitter\|-SubmitterNode]` | Ordered form `entries` (repeated names and file metadata preserved). `-Form` takes the form's id. |
| `Submit-UI4MEForm -Document (-Form\|-FormNode) [-Submitter\|-SubmitterNode]` | Request submission: validation, then a `submit` event. |
| `Reset-UI4MEForm -Document (-Form\|-FormNode)` | Reset a form. |
| `Set-UI4MEFocus -Document (-Element\|-Node)` | Move keyboard focus inside the document. |
| `Set-UI4MEFocus -Document -Blur [-Element\|-Node]` | Remove focus. |
| `Get-UI4MEFocusedElement -Document` | The focused element. |
| `Set-UI4MEPreventDefault -Document -Element -EventType [-Prevent $true] [-Key] [-Shift] [-Ctrl] [-Alt] [-Meta]` | Register, before input happens, whether a supported default action is prevented (for example form `submit`, or `keydown` with `-Key Enter`). Alias: `-Event`. Modifier filters take `$true`/`$false`. |

### Styles, environment, scrolling, dialogs and popovers

| Command | Description |
| --- | --- |
| `Add-UI4MEStylesheet -Document -Css` | Add a stylesheet; returns `{ sheet }`, a numeric handle. |
| `Set-UI4MEStylesheet -Document -Sheet -Css` | Replace a stylesheet's CSS. |
| `Set-UI4MEStylesheet -Document -Sheet -Remove` | Remove it. |
| `Get-UI4MEStylesheets -Document` | List stylesheets. |
| `Get-UI4MEComputedStyle -Document -Properties [-Element\|-Node]` | Computed values of the named CSS properties for an element or node. |
| `Get-UI4MEEnvironment -Document` | Media preferences in force and their source: `color_scheme`, `reduced_motion`, `high_contrast`, `scale`, ... |
| `Set-UI4MEEnvironment -Document [-ColorScheme light\|dark\|system] [-ReducedMotion reduce\|no-preference\|system]` | Override `prefers-color-scheme` / `prefers-reduced-motion` for this document; `system` returns to the desktop setting. |
| `Get-UI4MEScroll -Document [-Element]` | Scroll offset and range of the viewport or an element. |
| `Set-UI4MEScroll -Document [-Element] [-X] [-Y] [-Behavior auto\|instant\|smooth]` | Scroll to a position (CSS px). |
| `Show-UI4MEHtmlDialog -Document -Element [-Modal $true]` | Show an HTML `<dialog>` (modal by default). |
| `Show-UI4MEHtmlDialog -Document -Element -Close [-ReturnValue]` | Close it. |
| `Show-UI4MEPopover -Document -Element [-Action show\|hide\|toggle]` | Show, hide or toggle a popover; `toggle` returns the new open state. |

### Nodes (generation-bound references)

| Command | Description |
| --- | --- |
| `Get-UI4MENode -Document [-Element] [-Reference]` | Resolve an element id (or, with neither parameter, the root) to a `UI4ME.Node`, or wrap a node reference string returned by the host. |
| `Get-UI4MENodes -Document -Selector [-Offset 0] [-Limit 1024]` | Node references for a selector, including elements without ids. |
| `Get-UI4MENodeInfo -Node` | Describe a node. |
| `Get-UI4MENodeChildren -Node [-Offset 0] [-Limit 256]` | A page of child nodes. |
| `Invoke-UI4MENodeOperation -Node -Operation [-Parameters @{}]` | One node operation: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `trim_children`, `move_element` (`target_node` may be a `UI4ME.Node`), `remove_element`. |
| `Invoke-UI4MENodeBatch -Document -Operations @(@{ op = ...; node = ... }, ...)` | Several node operations atomically (node references as strings, `$node.Id`). |

The host rejects a node reference from an earlier document generation even if you bypass the
local check.

### Desktop services

| Command | Description |
| --- | --- |
| `Get-UI4MEClipboard -Application` | System clipboard text. |
| `Set-UI4MEClipboard -Application -Text` | Set it (up to 1 MiB). On Wayland the app must have received keyboard or pointer input first. |
| `Show-UI4MEFileDialog -Window [-Kind OpenFiles\|SaveFile\|ChooseFolder] [-Title] [-Directory] [-FileName] [-Filters @()] [-Multiple $false] [-NoWait]` | Native file dialog. Returns `{ cancelled, paths }` and waits without a deadline while still answering lifecycle requests. `-FileName` applies to `SaveFile`, `-Multiple` to `OpenFiles`. Filters: `@(@{ name = 'Images'; extensions = @('png', 'jpg') })`. `-NoWait` returns a `UI4ME.Request` for `Wait-UI4MERpc` (result) or `Stop-UI4MERpc` (closes the dialog). |
| `Open-UI4MEExternal -Application -Target [-Window]` | Open a URL (`http`, `https`, `mailto`, `file`) or a document path with the desktop's default handler. Executables, scripts and shortcuts are refused. |
| `Open-UI4MEExternal -Application -Target -Reveal [-Window]` | Show a file in the file manager. |
| `Get-UI4MEPreferences -Application` | Observed desktop preferences (color scheme, contrast, motion, ...) and their source. Changes arrive as the `desktop.preferences_changed` notification. |
| `Get-UI4MECapabilities -Application` | Host features and limits on this platform. |

### Resources, diagnostics and live bitmaps

| Command | Description |
| --- | --- |
| `Register-UI4MEResources -Document -Name -Entries @{ 'icon.png' = @{ Bytes = $bytes; Mime = 'image/png' } }` | Atomically replace the document-local bundle `ui4me-res://NAME/`. Values are `[byte[]]` or `@{ Bytes; Mime }`. At most 256 entries and 1 MiB per registration. |
| `Unregister-UI4MEResources -Document -Name` | Remove a bundle. |
| `Get-UI4MEResourceStats -Document` | Resource cache statistics. |
| `Clear-UI4MEUnusedResources -Document [-Url]` | Release cached resources that nothing uses (or one URL). |
| `Get-UI4MEDiagnostics -Document` | Renderer diagnostics for the document. |
| `New-UI4MELiveBitmap -Document -Id -Width -Height [-Slots 2]` | Create a live image for `<img src="ui4me-bitmap://ID">` (Id: 1-64 of `A-Z a-z 0-9 _ -`; up to 4096x4096; 2 or 3 slots). |
| `Send-UI4MELiveBitmap -Bitmap -Pixels [-Try]` | Write one straight-alpha RGBA8 frame (`width*height*4` bytes, tightly packed rows) and present it. Returns once the host has taken the frame, so a fast producer is paced by the host. `-Try` returns `$null` instead of failing when a present of the same bitmap is already in progress (for example from a callback that runs during it). |
| `Remove-UI4MELiveBitmap -Bitmap [-Quiet]` | Unmap and release. Idempotent. Replacing or closing the document releases its bitmaps automatically. |

Pixel buffers are ordinary `[byte[]]` and travel through a shared-memory file, not JSON.
Per-pixel PowerShell loops are slow; build frames from precomputed rows with
`[Buffer]::BlockCopy` or `[Array]::Copy`, or produce them with a .NET library. The host
creates the shared file in `$XDG_RUNTIME_DIR` (Linux) or `%TEMP%` (Windows) and removes it once
both sides have mapped or closed it.

### Raw protocol

| Command | Description |
| --- | --- |
| `Invoke-UI4MERpc -Application -Method [-Parameters @{}] [-TimeoutMilliseconds 15000]` | Call any host method and return its result. `dialog.*` methods wait without a deadline by default; `-1` waits forever. Events keep being processed while waiting. |
| `Start-UI4MERpc -Application -Method [-Parameters @{}]` | Send a request without waiting; returns a `UI4ME.Request`. |
| `Wait-UI4MERpc -Request [-TimeoutMilliseconds 15000]` | Wait for a request's result while pumping. On timeout the request is cancelled and `UI4ME.RpcTimeoutException` is thrown. A request can be awaited once. |
| `Stop-UI4MERpc -Request` | Ask the host to cancel (for example, close a native dialog). Not a rollback. |
| `Send-UI4MENotification -Application -Method [-Parameters @{}]` | Send a notification (no reply). |

### Hosting, projects and agent skills

| Command | Description |
| --- | --- |
| `Get-UI4MEHostPath` | Path of the bundled native host for this platform (`UI4ME_HOST_BINARY` overrides it). Restores the Linux execute bit if package extraction dropped it. x64 only. |
| `Start-UI4MEHost -Manifest [-NoWait]` | Run an `app.toml` through the native host and wait for it to exit (throws on a nonzero exit code). `-NoWait` returns the host process object instead. |
| `New-UI4MEProject -Path [-Name 'UI4ME PowerShell App'] [-AppId org.example.PowerShellApp] [-WindowsPowerShell] [-Force]` | Generate `app.ps1` (uses `Start-UI4MEScript`; run it directly), `index.html`, `style.css` and an optional `app.toml` whose backend runs `app.ps1` with `pwsh`, or `powershell.exe` with `-WindowsPowerShell`. `app.ps1` is saved as UTF-8 with a BOM. Refuses to overwrite these files without `-Force`. Returns the folder. |
| `Install-UI4MESkill [-Action install\|status\|uninstall] [-Agents all] [-Scope user\|project] [-ProjectPath] [-Force] [-DryRun]` | Install, inspect or remove the bundled `ui4me-powershell` coding-agent skill. `-Agents` takes `all` or any of `claude`, `codex`, `opencode`, `copilot`, `cursor`, `gemini` (array or comma list). `-ProjectPath` (an existing folder) is required with `-Scope project`. Never replaces or removes a folder it did not install, or that was edited since, without `-Force`. `-DryRun` reports what it would do. Returns one object per agent (`Agent`, `Scope`, `State`, `Path`, `Action`). |

Skill locations: user scope `~/.claude/skills`, `~/.agents/skills` (Codex),
`~/.config/opencode/skills`, `~/.copilot/skills`, `~/.cursor/skills`, `~/.gemini/skills`;
project scope `.claude/skills`, `.agents/skills`, `.opencode/skills`, `.github/skills`,
`.cursor/skills`, `.gemini/skills`. Each gets a `ui4me-powershell` folder.

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
`Invoke-UI4MERpc`): `host.diagnostics`, `desktop.paths`, `app.force_quit`. See the
[protocol overview](protocol.md).

## Parity with the Python API

| Python (`ui4me_core`) | PowerShell |
| --- | --- |
| `Application.start()` | `Start-UI4MEScript` |
| `Application.from_environment()` / `run()` | `Connect-UI4ME`; `Set-UI4MEReady` + `Wait-UI4ME -UntilShutdown` |
| `app.create_window(...)` | `New-UI4MEWindow` |
| `window.load_file` / `load_html` / `set` / `state` / `request_close` / `request_attention` / `on_before_close` | `Open-UI4MEFile` / `Open-UI4MEHtml` / `Set-UI4MEWindow` / `Get-UI4MEWindow` / `Close-UI4MEWindow` / `Request-UI4MEAttention` / `Set-UI4MEBeforeClose` |
| `window.open_files` / `save_file` / `choose_folder` (`wait=False`) | `Show-UI4MEFileDialog -Kind ...` (`-NoWait`) |
| `app.request_quit` / `on_before_quit` / `ready` | `Request-UI4MEQuit` / `Set-UI4MEBeforeQuit` / `Set-UI4MEReady` |
| `app.clipboard_text` / `set_clipboard_text` | `Get-UI4MEClipboard` / `Set-UI4MEClipboard` |
| `app.capabilities` / `desktop_preferences` / `on_desktop_preferences` | `Get-UI4MECapabilities` / `Get-UI4MEPreferences` / `Register-UI4MENotification -Method desktop.preferences_changed` |
| `app.open_external` / `reveal_file` | `Open-UI4MEExternal` / `Open-UI4MEExternal -Reveal` |
| `doc.command(op, ...)` / `doc.on(...)` / `doc.batch` / `doc.unload` | `Invoke-UI4MEDocumentCommand` / `Register-UI4MEDocumentEvent` / `Invoke-UI4MEBatch` / `Remove-UI4MEDocument` |
| `set_text`, `get_text`, `set_value`, `get_value`, `set_checked`, `get_checked` | `Set-`/`Get-UI4MEDocumentText`, `Set-UI4MEDocumentValue`/`Get-UI4MEValue`, `Set-`/`Get-UI4MEChecked` |
| `set_attribute`, `remove_attribute`, `add_class`, `remove_class` | `Set-UI4MEDocumentAttribute [-Remove]`, `Set-UI4MEClass [-Remove]` |
| `replace_html`, `insert_html`, `remove`, `query` | `Set-UI4MEDocumentHtml [-Position]`, `Remove-UI4MEElement`, `Find-UI4MEElement` |
| `focus`, `blur`, `focused_element`, `prevent_default` | `Set-UI4MEFocus [-Blur]`, `Get-UI4MEFocusedElement`, `Set-UI4MEPreventDefault` |
| `control_selection`, `set_control_selection`, `edit` | `Get-`/`Set-UI4MESelection`, `Invoke-UI4MEEditCommand` |
| `form_data`, `request_submit`, `reset_form`, `options`, `set_selected_values` | `Get-UI4MEFormData`, `Submit-UI4MEForm`, `Reset-UI4MEForm`, `Get-UI4MESelectOptions`, `Set-UI4MESelectedValues` |
| `add_stylesheet`, `replace_stylesheet`, `remove_stylesheet`, `stylesheets`, `computed_style` | `Add-UI4MEStylesheet`, `Set-UI4MEStylesheet [-Remove]`, `Get-UI4MEStylesheets`, `Get-UI4MEComputedStyle` |
| `environment`, `set_environment`, `scroll`, `scroll_to` | `Get-`/`Set-UI4MEEnvironment`, `Get-`/`Set-UI4MEScroll` |
| `show_dialog`, `close_dialog`, `show_popover`, `hide_popover`, `toggle_popover` | `Show-UI4MEHtmlDialog [-Close]`, `Show-UI4MEPopover -Action` |
| `register_resources`, `unregister_resources`, `resource_stats`, `purge_unused_resources`, `diagnostics` | `Register-`/`Unregister-UI4MEResources`, `Get-UI4MEResourceStats`, `Clear-UI4MEUnusedResources`, `Get-UI4MEDiagnostics` |
| `node`, `query_nodes`, `node_batch`, `Node.inspect` / `children` / `set_text` / ... / `move_to` | `Get-UI4MENode`, `Get-UI4MENodes`, `Invoke-UI4MENodeBatch`, `Get-UI4MENodeInfo` / `Get-UI4MENodeChildren` / `Invoke-UI4MENodeOperation` |
| `create_bitmap`, `LiveBitmap.present` / `try_present` / `close` | `New-UI4MELiveBitmap`, `Send-UI4MELiveBitmap [-Try]`, `Remove-UI4MELiveBitmap` |
| `RpcClient.call` / `call_async` / `future.cancel()` / `notify` | `Invoke-UI4MERpc` / `Start-UI4MERpc` + `Wait-UI4MERpc` / `Stop-UI4MERpc` / `Send-UI4MENotification` |

Differences by design: PowerShell callbacks are serialized on one runspace (Python runs
lifecycle policies on separate threads; both answer them while an event callback waits);
policy callbacks decide by their last output value; the timeout exception is
`UI4ME.RpcTimeoutException`.

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
| Queued host notifications/requests | 128 messages and 8 MiB. Overflowing notifications are dropped and counted (`$app.Transport.DroppedNotifications`); overflowing requests get error `-32005`. Window close, quit and shutdown requests are always admitted. |
| Deferred notifications during one callback | 128 (excess counted in `$app.DroppedNotifications`) |
| Outstanding requests | 128 |
| Nested callback depth | 32 |
| Event subscriptions per document | 256 |
| Selector query page | 1-4096 entries (default 1024) |
| Resource bundle | 256 entries, 1 MiB per registration |
| Live bitmap | 4096x4096, 2 or 3 slots |
