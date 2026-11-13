# PowerShell API reference — UI4ME.Core 1.0.0

`UI4ME.Core` is the PowerShell backend binding for the UI4ME Core native host. The host
(one Rust process) owns native windows, input, IME, HTML/CSS layout and rendering,
accessibility and dialogs. Your PowerShell script is the application backend: it runs in a
separate process that the host starts from `app.toml`, and the two talk over a private,
authenticated, length-prefixed JSON-RPC channel (a Unix-domain socket on Linux, a named
pipe on Windows). No Python, browser engine, JavaScript, Node.js or HTTP server is
involved.

Supported backends: **PowerShell 7.2+** on Linux x64 and Windows x64, and **Windows
PowerShell 5.1** on Windows x64. See [the guide](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/powershell.md) for a walkthrough
and [platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md) for qualified desktops.

- [Program structure](#program-structure)
- [Callbacks, ordering and closures](#callbacks-ordering-and-closures)
- [Errors](#errors)
- [Commands](#commands)
- [Python parity](#parity-with-the-python-api)
- [Limits](#limits)

## Program structure

```powershell
# backend.ps1 — started by the host, never directly
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core
$app = Connect-UI4ME                      # reads UI4ME_ENDPOINT / UI4ME_TOKEN from the host
try {
    $window = New-UI4MEWindow -Application $app -Title 'Hello' -Width 900 -Height 600
    $doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element save -Callback {
        param($e)
        Set-UI4MEDocumentText -Document $doc -Element status -Text 'Saved'
    }
    Set-UI4MEReady -Application $app      # startup finished (needs at least one window)
    Wait-UI4ME -Application $app -UntilShutdown
} finally {
    Close-UI4ME -Application $app
}
```

`app.toml` names the backend program. Use `pwsh` for PowerShell 7 or `powershell.exe` for
Windows PowerShell 5.1:

```toml
[app]
id = "org.example.Hello"
name = "Hello"
version = "1.0.0"

[backend]
program = "pwsh"
args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "{app_dir}/backend.ps1"]
cwd = "{app_dir}"
```

Run it with `Start-UI4MEHost -Manifest ./app.toml`. `New-UI4MEProject` generates this layout.

**Objects.** `Connect-UI4ME` returns a `UI4ME.Application`. `New-UI4MEWindow` returns a
`UI4ME.Window`. `Open-UI4MEFile`/`Open-UI4MEHtml` return a `UI4ME.Document`, which is bound to
one document generation: loading another document into the window, unloading it, a fatal
document error or closing the window makes it **stale**, and every later command on it throws
`UI4ME.StaleObjectException` locally. The same applies to nodes (`UI4ME.Node`), live bitmaps
and event subscriptions of that document. Useful properties: `$app.ShutdownRequested`,
`$app.Hello`, `$window.Closed`, `$window.Document`, `$doc.Retired`, `$bitmap.Sequence`.

**Results.** Commands that return host data return objects whose properties are the JSON
field names (`$state.observed.maximized`, `$result.paths`). Numbers arrive as `[int]`, `[long]`
or `[double]`; strings are never converted to dates.

## Callbacks, ordering and closures

All callbacks run on the backend's own PowerShell runspace, never on a .NET thread pool
thread. A background reader thread only queues messages; they are processed when your script
pumps: inside `Wait-UI4ME`, and inside any command that waits for a host reply.

- **Document and notification callbacks run one at a time, in host order.** If a callback
  issues commands (which pump while waiting), later notifications are deferred until it
  returns; they never run nested inside it.
- **Lifecycle requests still run while a callback waits.** `Set-UI4MEBeforeClose` and
  `Set-UI4MEBeforeQuit` policies, and `app.shutdown`, are answered from inside such waits.
  A click handler that waits for a native dialog therefore does not stop the user from
  closing the window; the host cancels the dialog and the handler gets an error.
- A long-running callback delays later events. Keep callbacks short; split long work
  into steps that run between `Wait-UI4ME` calls.
- Do not call UI4ME commands from other runspaces, jobs or threads.

Event callbacks receive the event object: `type`, `element` (nearest element id), `path`
(ids along the route), type-specific fields such as `value`, `button`, `x`, `y`, `key` and
modifiers, plus `window_id` and `document_id`. Callbacks run **after** native dispatch; use
`Set-UI4MEPreventDefault` beforehand to stop a default action.

**Scriptblocks and variables.** A scriptblock defined at the top level of `backend.ps1` sees
the script's variables when it runs (`$doc`, `$app`, `$script:counter`). A scriptblock defined
*inside a function* loses the function's local variables once the function returns. Capture
them by calling `.GetNewClosure()` **in that function**:

```powershell
function Add-SaveHandler($Document, [string]$Label) {
    $log = $script:log                   # capture objects you will use as locals
    Register-UI4MEDocumentEvent -Document $Document -Type click -Element save -Callback ({
        param($e)
        [void]$log.Add("$Label clicked")  # $Label and $log were captured
    }.GetNewClosure())
}
```

Inside a `.GetNewClosure()` scriptblock, `$script:` refers to the closure's own new scope,
not your script. Pass collections or other objects in as captured locals (as above), or use
`$global:` deliberately.

**Policy callbacks** (`Set-UI4MEBeforeClose`, `Set-UI4MEBeforeQuit`) decide by their *last*
output value: `$true` allows, `$false` or no output vetoes. Stray output earlier in the block
is ignored.

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
method/operation, `-32004` closed window/document, `-32005` busy or bounded, `-32008` invalid
state (for example during shutdown), `-32003` refused by policy.

Errors thrown inside event callbacks are written to stderr (the host log) and do not stop the
application. Errors inside policy callbacks are reported to the host as a failed decision.

## Commands

Parameters shown in brackets are optional. `-Element` takes an HTML `id`. Commands that also
accept `-Node` take a `UI4ME.Node` from the same document.

### Connection and raw protocol

| Command | Description |
| --- | --- |
| `Connect-UI4ME [-Endpoint] [-Token] [-ConnectTimeoutMilliseconds 15000]` | Connect and authenticate (`host.hello`, protocol 1). Defaults come from the host's environment variables. |
| `Wait-UI4ME -Application [-TimeoutMilliseconds 100] [-Once]` | Pump events and run callbacks. Waits up to the timeout for the first message, then drains up to 64. Returns the number handled. |
| `Wait-UI4ME -Application -UntilShutdown` | Pump until the host requests shutdown. |
| `Close-UI4ME -Application` | Close the connection; retire all handles. Idempotent; use in `finally`. |
| `Invoke-UI4MERpc -Application -Method [-Parameters @{}] [-TimeoutMilliseconds]` | Call any host method. Default timeout 15 s; `dialog.*` methods wait without a deadline; `-1` waits forever. |
| `Start-UI4MERpc -Application -Method [-Parameters]` | Send a request without waiting; returns a `UI4ME.Request`. |
| `Wait-UI4MERpc -Request [-TimeoutMilliseconds 15000]` | Wait for a request's result while pumping. |
| `Stop-UI4MERpc -Request` | Ask the host to cancel (for example close a native dialog). Not a rollback. |
| `Send-UI4MENotification -Application -Method [-Parameters]` | Send a notification (no reply). |

### Application and windows

| Command | Description |
| --- | --- |
| `New-UI4MEWindow -Application [-Title] [-Width 900] [-Height 600] [-Visible $true] [-Resizable $true] [-Decorations $true] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight]` | Create a native window (sizes in CSS px). Constraints are set in pairs. |
| `Set-UI4MEWindow -Window [-Title] [-Width -Height] [-Visible] [-Resizable] [-Decorations] [-Minimized] [-Maximized] [-Fullscreen] [-Focus] [-X -Y] [-MinWidth -MinHeight] [-MaxWidth -MaxHeight] [-Properties @{}]` | Change window state using the host's field names (`minimized`, `maximized`, `fullscreen` are booleans). `-Properties @{ min_width = $null; min_height = $null }` clears a constraint. |
| `Get-UI4MEWindow -Window` | Requested and observed state: `title`, `width`, `height`, `scale_factor`, `physical_size`, `observed.{focused,minimized,maximized,fullscreen,position,theme}`. |
| `Get-UI4MEWindowList -Application` | All open windows of the app. |
| `Close-UI4MEWindow -Window` | Request a graceful close (the before-close policy may veto it). |
| `Request-UI4MEAttention -Window [-Kind informational\|critical\|none]` | Ask the desktop to flag the window. |
| `Set-UI4MEBeforeClose -Window -Callback { param($request) $true }` | Close policy for this window; `$null` removes it. |
| `Set-UI4MEBeforeQuit -Application -Callback { param($request) $true }` | Quit policy; `$null` removes it. |
| `Set-UI4MEReady -Application` | Report that startup finished. Required once, after the first window exists. |
| `Request-UI4MEQuit -Application` | Request an orderly quit (the quit policy may veto it). |
| `Register-UI4MENotification -Application -Method -Callback` | Callback for a host notification method, such as `desktop.preferences_changed` or `window.event`. One per method; `$null` removes it. |

Platform notes: on Wayland a client cannot focus, position or un-minimize its own window;
`-Focus` is refused there (`-32001`) and `observed.position` is `$null`. Use
`Request-UI4MEAttention` instead.

### Documents

| Command | Description |
| --- | --- |
| `Open-UI4MEFile -Window -Path [-AssetRoot]` | Load an HTML file. Relative stylesheets, images and fonts resolve against its folder (or `-AssetRoot`). Atomic: the previous document stays if loading fails. |
| `Open-UI4MEHtml -Window -Html [-AssetRoot]` | Load generated HTML. Use `-AssetRoot` for relative URLs. |
| `Remove-UI4MEDocument -Document` | Unload the document; the window stays open. |
| `Register-UI4MEDocumentEvent -Document -Type [-Element] -Callback` | Subscribe to an event type, optionally delegated to an element id on the event's `path`. Returns a subscription. At most 256 per document. |
| `Unregister-UI4MEDocumentEvent -Subscription` | Remove a subscription. |
| `Invoke-UI4MEDocumentCommand -Document -Operation [-Parameters @{}]` | Run **any** [document command](https://github.com/ui4me/ui4me-core/blob/main/docs/PROTOCOL_DOCUMENTS.md) and return its result. |
| `Invoke-UI4MEBatch -Document -Operations @(@{op=...}, ...)` | Apply id-addressed operations as one atomic transaction (all or nothing). |

### Content, attributes and classes

| Command | Description |
| --- | --- |
| `Set-UI4MEDocumentText -Document -Element -Text` | Replace content with text. Never parsed as HTML; safe for user input. |
| `Get-UI4MEDocumentText -Document -Element` | Text content. |
| `Set-UI4MEDocumentHtml -Document -Element -Html [-Position replace\|beforebegin\|afterbegin\|beforeend\|afterend]` | Replace children with, or insert, **trusted** HTML. |
| `Remove-UI4MEElement -Document -Element` | Remove an element. |
| `Set-UI4MEDocumentAttribute -Document -Element -Name [-Value] [-Remove]` | Set or remove an attribute. |
| `Set-UI4MEClass -Document -Element -Name [-Remove]` | Add or remove a class. |
| `Find-UI4MEElement -Document -Selector [-Offset] [-Limit]` | CSS selector query; paginated ids. |

### Forms, controls and focus

| Command | Description |
| --- | --- |
| `Get-UI4MEValue` / `Set-UI4MEDocumentValue -Document -Element [-Value]` | Control value. |
| `Get-UI4MEChecked` / `Set-UI4MEChecked -Document -Element [-Checked]` | Checkbox/radio state. |
| `Get-UI4MESelectOptions -Document -Element` | `<select>` options. |
| `Set-UI4MESelectedValues -Document -Element -Values` | Select options by value. |
| `Get-UI4MESelection` / `Set-UI4MESelection -Document (-Element\|-Node) [-Start -End -Direction]` | Text-control selection in UTF-16 code units. |
| `Invoke-UI4MEEditCommand -Document -Command [-Element\|-Node]` | Native editing command (`undo`, `redo`, `cut`, `copy`, `paste`, `selectAll`, `delete`, ...), through the editor's history/IME path. |
| `Get-UI4MEFormData -Document (-Form\|-FormNode) [-Submitter\|-SubmitterNode]` | Ordered entries (repeated names and file metadata preserved). |
| `Submit-UI4MEForm` / `Reset-UI4MEForm -Document (-Form\|-FormNode)` | Request submission (validation, then a `submit` event) or reset. |
| `Set-UI4MEFocus -Document (-Element\|-Node)`, `Set-UI4MEFocus -Document [-Element] -Blur` | Move or remove focus. |
| `Get-UI4MEFocusedElement -Document` | Focused element. |
| `Set-UI4MEPreventDefault -Document -Element -EventType [-Prevent $true] [-Key] [-Shift] [-Ctrl] [-Alt] [-Meta]` | Register, before input, whether a supported default action is prevented. |

### Styles, environment, scrolling, dialogs

| Command | Description |
| --- | --- |
| `Add-UI4MEStylesheet -Document -Css` | Add a stylesheet; returns `{sheet}`. |
| `Set-UI4MEStylesheet -Document -Sheet -Css` / `-Sheet -Remove` | Replace or remove a stylesheet. |
| `Get-UI4MEStylesheets -Document` | List stylesheets. |
| `Get-UI4MEComputedStyle -Document -Properties [-Element\|-Node]` | Computed values. |
| `Get-UI4MEEnvironment` / `Set-UI4MEEnvironment -Document [-ColorScheme] [-ReducedMotion]` | Media preference overrides for this document. |
| `Get-UI4MEScroll` / `Set-UI4MEScroll -Document [-Element] [-X] [-Y] [-Behavior]` | Scroll positions. |
| `Show-UI4MEHtmlDialog -Document -Element [-Modal $true]`, `... -Close [-ReturnValue]` | HTML `<dialog>`. |
| `Show-UI4MEPopover -Document -Element [-Action show\|hide\|toggle]` | Popovers; `toggle` returns the new open state. |

### Nodes (generation-bound references)

| Command | Description |
| --- | --- |
| `Get-UI4MENode -Document [-Element] [-Reference]` | Resolve an id (or the root) to a node, or wrap a host reference. |
| `Get-UI4MENodes -Document -Selector [-Offset] [-Limit]` | Node references for a selector, including elements without ids. |
| `Get-UI4MENodeInfo -Node`, `Get-UI4MENodeChildren -Node [-Offset] [-Limit]` | Inspect. |
| `Invoke-UI4MENodeOperation -Node -Operation [-Parameters]` | `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `trim_children`, `move_element` (`target_node` may be a `UI4ME.Node`), `remove_element`. |
| `Invoke-UI4MENodeBatch -Document -Operations` | Several node operations atomically. |

The host rejects a node reference from an earlier document generation even if you bypass the
local check.

### Desktop services

| Command | Description |
| --- | --- |
| `Get-UI4MEClipboard` / `Set-UI4MEClipboard -Application [-Text]` | System clipboard text (up to 1 MiB). On Wayland the app must have received input before it may set the clipboard. |
| `Show-UI4MEFileDialog -Window [-Kind OpenFiles\|SaveFile\|ChooseFolder] [-Title] [-Directory] [-FileName] [-Filters] [-Multiple $false] [-NoWait]` | Native dialog. Returns `{cancelled, paths}`; waits without a deadline. `-NoWait` returns a request for `Wait-UI4MERpc`/`Stop-UI4MERpc`. Filters: `@(@{name='Images'; extensions=@('png','jpg')})`. |
| `Open-UI4MEExternal -Application -Target [-Window]` | Open a URL (`http`, `https`, `mailto`, `file`) or absolute path with the default handler. On Windows, executables, scripts and shortcuts are refused. |
| `Open-UI4MEExternal -Application -Target -Reveal` | Show a file in the file manager. |
| `Get-UI4MEPreferences -Application` | Observed desktop preferences and their source. |
| `Get-UI4MECapabilities -Application` | Host features and limits. |

### Resources and live bitmaps

| Command | Description |
| --- | --- |
| `Register-UI4MEResources -Document -Name -Entries @{ 'icon.png' = @{ Bytes = $bytes; Mime = 'image/png' } }` | Atomically replace the document-local bundle `ui4me-res://NAME/`. Values are byte arrays or `@{Bytes;Mime}`. At most 256 entries and 1 MiB per registration. |
| `Unregister-UI4MEResources -Document -Name` | Remove a bundle. |
| `Get-UI4MEResourceStats`, `Clear-UI4MEUnusedResources [-Url]`, `Get-UI4MEDiagnostics` | Resource lifetime and engine diagnostics. |
| `New-UI4MELiveBitmap -Document -Id -Width -Height [-Slots 2]` | Create a live image for `<img src="ui4me-bitmap://ID">` (up to 4096×4096, 2 or 3 slots, 64 MiB). |
| `Send-UI4MELiveBitmap -Bitmap -Pixels [-Try]` | Write one straight-alpha RGBA8 frame (`width*height*4` bytes, tight rows) and present it. Returns once the host has taken the frame, so a fast producer is paced by the host. `-Try` returns `$null` instead of re-entering a present of the same bitmap. |
| `Remove-UI4MELiveBitmap -Bitmap [-Quiet]` | Unmap and release. Replacing or closing the document releases its bitmaps automatically. |

Pixel buffers are ordinary `[byte[]]`. Per-pixel PowerShell loops are slow; build frames from
precomputed rows with `[Buffer]::BlockCopy` or `[Array]::Copy`, or produce them with a .NET
library. The host creates the shared file in `$XDG_RUNTIME_DIR` (Linux) or `%TEMP%` (Windows)
and removes it once both sides have mapped or closed it.

### Hosting, projects and agent skills

| Command | Description |
| --- | --- |
| `Get-UI4MEHostPath` | The bundled native host for this platform (`UI4ME_HOST_BINARY` overrides it). Restores the Linux execute bit if package extraction dropped it. |
| `Start-UI4MEHost -Manifest [-NoWait]` | Run an app manifest; waits for exit unless `-NoWait` (returns the process). |
| `New-UI4MEProject -Path [-Name] [-AppId] [-WindowsPowerShell] [-Force]` | Generate `app.toml`, `backend.ps1`, `index.html`, `style.css`. Refuses to overwrite without `-Force`. |
| `Install-UI4MESkill [-Action install\|status\|uninstall] [-Agents all\|claude,codex,opencode,copilot,cursor,gemini] [-Scope user\|project] [-ProjectPath] [-Force] [-DryRun]` | Manage the bundled `ui4me-powershell` coding-agent skill. Never replaces or removes folders it did not install (or that were edited) without `-Force`. |

Skill locations: user scope `~/.claude/skills`, `~/.agents/skills` (Codex),
`~/.config/opencode/skills`, `~/.copilot/skills`, `~/.cursor/skills`, `~/.gemini/skills`;
project scope `.claude/skills`, `.agents/skills`, `.opencode/skills`, `.github/skills`,
`.cursor/skills`, `.gemini/skills`.

## Parity with the Python API

Every host method and document operation is reachable from PowerShell through
`Invoke-UI4MERpc` and `Invoke-UI4MEDocumentCommand`. Typed commands cover the common ones:

| Python (`ui4me_core`) | PowerShell |
| --- | --- |
| `Application.from_environment()` / `run()` | `Connect-UI4ME`; `Set-UI4MEReady` + `Wait-UI4ME -UntilShutdown` |
| `app.create_window(...)` | `New-UI4MEWindow` |
| `app.open_file(...)` / `app.open_html(...)` | `New-UI4MEWindow` + `Open-UI4MEFile` / `Open-UI4MEHtml` |
| `window.load_file` / `load_html` / `set` / `state` / `request_close` / `request_attention` / `on_before_close` | `Open-UI4MEFile` / `Open-UI4MEHtml` / `Set-UI4MEWindow` / `Get-UI4MEWindow` / `Close-UI4MEWindow` / `Request-UI4MEAttention` / `Set-UI4MEBeforeClose` |
| `window.open_files` / `save_file` / `choose_folder` (`wait=False`) | `Show-UI4MEFileDialog -Kind ...` (`-NoWait`) |
| `app.request_quit` / `on_before_quit` / `ready` | `Request-UI4MEQuit` / `Set-UI4MEBeforeQuit` / `Set-UI4MEReady` |
| `app.clipboard_text` / `set_clipboard_text` | `Get-UI4MEClipboard` / `Set-UI4MEClipboard` |
| `app.capabilities` / `desktop_preferences` / `on_desktop_preferences` | `Get-UI4MECapabilities` / `Get-UI4MEPreferences` / `Register-UI4MENotification desktop.preferences_changed` |
| `app.open_external` / `reveal_file` | `Open-UI4MEExternal` / `Open-UI4MEExternal -Reveal` |
| `doc.command(op, ...)` / `doc.on(...)` / `doc.batch` / `doc.unload` | `Invoke-UI4MEDocumentCommand` / `Register-UI4MEDocumentEvent` / `Invoke-UI4MEBatch` / `Remove-UI4MEDocument` |
| `set_text`, `get_text`, `set_value`, `get_value`, `set_checked`, `get_checked` | `Set-/Get-UI4MEDocumentText`, `Set-UI4MEDocumentValue`/`Get-UI4MEValue`, `Set-/Get-UI4MEChecked` |
| `set_attribute`, `remove_attribute`, `add_class`, `remove_class` | `Set-UI4MEDocumentAttribute [-Remove]`, `Set-UI4MEClass [-Remove]` |
| `replace_html`, `insert_html`, `remove`, `query` | `Set-UI4MEDocumentHtml [-Position]`, `Remove-UI4MEElement`, `Find-UI4MEElement` |
| `focus`, `blur`, `focused_element`, `prevent_default` | `Set-UI4MEFocus [-Blur]`, `Get-UI4MEFocusedElement`, `Set-UI4MEPreventDefault` |
| `control_selection`, `set_control_selection`, `edit` | `Get-/Set-UI4MESelection`, `Invoke-UI4MEEditCommand` |
| `form_data`, `request_submit`, `reset_form`, `options`, `set_selected_values` | `Get-UI4MEFormData`, `Submit-UI4MEForm`, `Reset-UI4MEForm`, `Get-UI4MESelectOptions`, `Set-UI4MESelectedValues` |
| `add_stylesheet`, `replace_stylesheet`, `remove_stylesheet`, `stylesheets`, `computed_style` | `Add-UI4MEStylesheet`, `Set-UI4MEStylesheet [-Remove]`, `Get-UI4MEStylesheets`, `Get-UI4MEComputedStyle` |
| `environment`, `set_environment`, `scroll`, `scroll_to` | `Get-/Set-UI4MEEnvironment`, `Get-/Set-UI4MEScroll` |
| `show_dialog`, `close_dialog`, `show_popover`, `hide_popover`, `toggle_popover` | `Show-UI4MEHtmlDialog [-Close]`, `Show-UI4MEPopover -Action` |
| `register_resources`, `unregister_resources`, `resource_stats`, `purge_unused_resources`, `diagnostics` | `Register-/Unregister-UI4MEResources`, `Get-UI4MEResourceStats`, `Clear-UI4MEUnusedResources`, `Get-UI4MEDiagnostics` |
| `node`, `query_nodes`, `node_batch`, `Node.inspect` / `children` / `set_text` / ... / `move_to` | `Get-UI4MENode`, `Get-UI4MENodes`, `Invoke-UI4MENodeBatch`, `Get-UI4MENodeInfo` / `Get-UI4MENodeChildren` / `Invoke-UI4MENodeOperation` |
| `create_bitmap`, `LiveBitmap.present` / `try_present` / `close` | `New-UI4MELiveBitmap`, `Send-UI4MELiveBitmap [-Try]`, `Remove-UI4MELiveBitmap` |
| `RpcClient.call` / `call_async` / `future.cancel()` / `notify` | `Invoke-UI4MERpc` / `Start-UI4MERpc` + `Wait-UI4MERpc` / `Stop-UI4MERpc` / `Send-UI4MENotification` |

Through `Invoke-UI4MEDocumentCommand` only (no typed wrapper): `element_at_point`,
`replace_control_range`, `set_custom_validity`, `get_validity`, `check_validity`,
`get_files`, `scroll_into_view`, `scroll_by`, `get_rect`, `get_html`, `text_rects`,
`text_at_point`, `get_attribute(s)`, `has_element`, `get_dialog`, `get_popover`,
`set_pointer_capture`, `get_selection`, `select_text`, `copy_selection`,
`set_scroll_follow`, `get_accessibility_tree`, `css_profile`, `explain`, `move_element`,
`append_text`, `trim_children`. Through `Invoke-UI4MERpc` only: `host.diagnostics`,
`desktop.paths`, `app.force_quit`.

Differences by design: PowerShell callbacks are serialized on one runspace (Python runs
lifecycle policies on separate threads; both answer them while an event callback waits);
policy callbacks decide by their last output value; the timeout exception is
`UI4ME.RpcTimeoutException`.

## Limits

| Bound | Value |
| --- | --- |
| Frame size (either direction) | 2 MiB after JSON encoding |
| Queued host notifications/requests | 128 messages and 8 MiB. Overflowing notifications are dropped and counted (`$app.Transport.DroppedNotifications`); overflowing requests get error `-32005`. Window close/quit/shutdown requests are always admitted. |
| Deferred notifications during one callback | 128 (excess counted in `$app.DroppedNotifications`) |
| Outstanding requests | 128 |
| Nested callback depth | 32 |
| Event subscriptions per document | 256 |
| HTML source / DOM batch / query page | 1 MiB / 1024 operations / 4096 entries |
