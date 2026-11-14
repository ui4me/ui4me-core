# Host protocol (private, version 1)

> **Status: private and versioned.** The supported programming interfaces are the [Python](python-api.md) and [PowerShell](powershell-api.md) bindings in this repository. This page documents protocol version 1, as implemented by UI4ME Core 1.0.x, for people who maintain those bindings or write another one. Method names, parameters and results can change in a future protocol version; the `protocol_version` handshake is how such a change is detected. UI4ME Core 1.0.1 does not change the protocol.

The authoritative sources are `crates/ui4me-host/src/protocol.rs` (framing, message profile, error codes), `transport.rs` (connection limits), `host/mod.rs` (handshake, lifecycle, notifications), `host/requests.rs` (window and native services), `host/documents.rs` and `host/document_commands.rs` (documents), `host/bitmap.rs` (live bitmaps), and the two reference clients, `python/ui4me_core/client.py` (Python) and `powershell/UI4ME.Core/private/UI4ME.Transport.cs` with `UI4ME.Core.psm1` (PowerShell).

## Roles

```text
ui4me-host (server)  <── one authenticated connection ──>  backend (client)
  windows, documents, input,                                 application logic,
  accessibility, lifecycle policy                            callbacks
```

The host creates a private endpoint, starts the backend from the [manifest](host-cli.md#the-apptoml-manifest) and waits for it to connect. Both sides send requests and notifications over the same connection:

* **Backend → host:** `host.*`, `app.*`, `window.*`, `document.*`, `bitmap.*`, `clipboard.*`, `desktop.*`, `dialog.*`, `request.cancel`.
* **Host → backend requests** (the backend must answer): `window.before_close`, `app.before_quit`.
* **Host → backend notifications:** `document.event`, `document.error`, `window.event`, `desktop.preferences_changed`, `app.activated`, `app.shutdown`, `request.cancelled`.

## Transport

| Platform | Endpoint (`UI4ME_ENDPOINT`) | Protection |
| --- | --- | --- |
| Linux | Unix stream socket `$XDG_RUNTIME_DIR/ui4me-host/<key>.sock` (falls back to `<tmp>/ui4me-runtime-<uid>/…` without `XDG_RUNTIME_DIR`) | Directory mode `0700` owned by the user (checked, symlinks refused); socket mode `0600` |
| Windows | Named pipe `\\.\pipe\ui4me-host-<key>` | Byte-mode pipe, `PIPE_REJECT_REMOTE_CLIENTS`, protected DACL granting access only to the current logon SID |

`<key>` is a hex hash of the app ID and desktop session for single-instance apps, or random otherwise. There is no TCP listener. Treat the endpoint as an opaque string from the environment; do not construct it.

Connection limits (host side): at most 8 simultaneous connections; a connection must send its first frame within 15 seconds; a connection that has not authenticated with `host.hello` is closed after about 5 seconds. Only one connection can be the authenticated backend.

## Framing

Each message is one frame:

```text
+----------------------------+--------------------------------------+
| length: u32, big-endian    | body: `length` bytes of UTF-8 JSON   |
+----------------------------+--------------------------------------+
```

* `length` counts body bytes only, must be 1 to 2,097,152 (2 MiB), and is checked before the body is read. A zero length, an oversized length, invalid UTF-8 or invalid JSON ends the connection. EOF inside a header or body is an error, not a clean close.
* The body is exactly one JSON object. Batches (arrays) are not supported.
* Writers serialize the complete body before writing the header, so an oversized message is never partially written. Both reference clients additionally reject duplicate object keys and non-finite numbers.

## Message profile (JSON-RPC 2.0 subset)

| Message | Shape |
| --- | --- |
| Request | `{"jsonrpc":"2.0","id":"<string>","method":"<name>","params":{…}}` |
| Notification | `{"jsonrpc":"2.0","method":"<name>","params":{…}}` (no `id`) |
| Success | `{"jsonrpc":"2.0","id":"<string>","result":<any>}` |
| Error | `{"jsonrpc":"2.0","id":"<string>"\|null,"error":{"code":<int>,"message":"<string>","data":<any, optional>}}` |

Rules enforced by the host:

* `jsonrpc` must be exactly `"2.0"`; unknown top-level members are rejected.
* `id` must be a **string** of 1–128 bytes without control characters. Numeric and `null` IDs are rejected.
* `method` is 1–128 bytes of ASCII letters, digits, `.`, `_` or `-`, and must not start with `rpc.`.
* `params` must be an object (named parameters) or omitted (treated as `{}`). Positional arrays are rejected. Every params object is decoded strictly: unknown keys are an `invalid params` error.
* At most 128 requests may be pending. Reusing the ID of a request that is still pending (a native dialog, external open or clipboard read) is a protocol violation: the host disconnects and fails the application.
* A result that would not fit in a 2 MiB frame is replaced by an `unavailable` error. A notification that would not fit is dropped (a `document.event` that is too large is reported as `document.error` instead).

Buffers: the host queues at most 128 outgoing messages / 8 MiB per connection, and a peer that does not read its messages is disconnected. Incoming messages share a 256-event / 8 MiB queue; when it is full the host stops reading from the connection (backpressure) until the GUI thread catches up. The Python client caps its own outgoing queue at 256 frames; both clients allow at most 128 pending requests, and the PowerShell client queues at most 128 incoming messages / 8 MiB (see the [PowerShell limits](powershell-api.md#limits)).

## Backend environment

The host starts the backend with its own environment plus `[backend.env]`, and then sets:

| Variable | Value |
| --- | --- |
| `UI4ME_ENDPOINT` | Socket path or pipe name to connect to |
| `UI4ME_TOKEN` | 64 lowercase hex characters (32 random bytes from the OS) for this launch |
| `UI4ME_APP_ID` | `[app].id` from the manifest |
| `UI4ME_PROTOCOL_VERSION` | `1` |

These four names, and any name beginning with `UI4ME_HOST_`, are reserved: a manifest that sets them in `[backend.env]` is rejected (case-insensitively). The token is never written to a manifest, a command line or the log. A binding should remove these variables from its own process environment after connecting, so that child processes started by the application cannot reuse them (both bindings do). A binding that finds only one of `UI4ME_ENDPOINT`/`UI4ME_TOKEN` should fail rather than guess.

## Handshake: `host.hello`

The first request on a connection must be `host.hello`. Any other request receives error `-32011` ("host.hello required").

```json
{"jsonrpc":"2.0","id":"py-1","method":"host.hello",
 "params":{"token":"<UI4ME_TOKEN>","protocol_version":1,
           "client":{"name":"my-binding","version":"0.1"}}}
```

`token` and `protocol_version` are checked; other keys (such as `client`) are ignored. The token is compared in constant time.

| Outcome | Error |
| --- | --- |
| `protocol_version` is not `1` | `-32010` version mismatch |
| Wrong or missing token | `-32011` unauthenticated |
| A backend is already connected | `-32005` busy |
| Host is no longer in its connecting phase | `-32008` invalid state |

Success result:

```json
{"protocol_version":1,"host_version":"1.0.1","app_id":"org.example.App",
 "capabilities":{…same object as host.capabilities…}}
```

## Lifecycle

```text
starting → connecting ──host.hello──▶ initializing ──app.ready──▶ running
running ──close/quit request──▶ quit_pending ──allow──▶ quitting ──backend exits──▶ stopped
                                      └──deny──▶ running
any phase ──backend crash / disconnect / startup timeout──▶ failed
```

1. **Connect and authenticate** with `host.hello`.
2. **Create at least one window**, load documents, register callbacks.
3. **`app.ready`** (request, `{}` → `{"ready":true}`). Fails with `-32008` if no window exists or the backend is not in the initializing phase. Authentication plus `app.ready` must complete within `startup_timeout_ms`, otherwise the host fails the application.
4. **Running.** Exchange requests and notifications.
5. **Shutdown** as described below.

When the backend disconnects or exits while the application is not quitting, the host enters `failed`: it cancels native work, kills the backend, shows the failure in its windows (desktop mode) and exits with status 1. In `quitting`, `failed` and `stopped`, every method except `host.capabilities`, `host.diagnostics`, `request.cancel` and `app.force_quit` returns `-32008`.

### Closing a window

When the user closes a window (`reason: "native_close"`) or the backend calls `window.request_close` (`reason: "backend_request"`), the host:

1. sends `window.event` with `type: "close_requested"`, `data: {"reason":…}`;
2. sends the **request** `window.before_close` with `{"window_id":"w1","reason":"native_close"}` and an ID such as `"host:decision:7"`.

The backend answers `{"allow": true}` or `{"allow": false}`. Anything else, including an error response, counts as **deny**. On allow the window is destroyed (`window.event` `type: "closed"`) — unless it is the last open window, in which case the host starts an application quit with `reason: "last_window"`. A window closed before `app.ready` cancels startup and shuts down.

### Quitting the application

`app.request_quit` (`{}` → `{"requested":true}`) or a last-window close sends the **request** `app.before_quit` with `{"reason":"backend_request"|"last_window"}`. While it is pending, `window.create` returns `-32005`. Pending window-close decisions are superseded and reported with the notification `request.cancelled` `{"id":"host:decision:N","reason":"application_quit"}`; late answers to them are ignored.

* **Deny** (`{"allow":false}`): the application keeps running.
* **Allow:** the host cancels native jobs and sends the **notification** `app.shutdown` `{"deadline_ms": <shutdown_timeout_ms>}`. The backend should clean up and exit. If it has not exited when the deadline passes, the host kills it and exits with status 1. A backend that exits non-zero during shutdown also gives exit status 1.

`app.force_quit` (`{}` → `{"requested":true}`) skips the decision, kills the backend and stops. The user can do the same with **Ctrl+Shift+Q** in a window.

### Second launches: `app.activated`

For `instance = "single"`, a second launch of the same app sends `app.activate` to the running host on the same private endpoint (see [host CLI](host-cli.md#single-instance)). After `app.ready`, the host focuses a window where the platform allows it and notifies the backend:

```json
{"jsonrpc":"2.0","method":"app.activated",
 "params":{"args":["ui4me-host","run","--app","/opt/app/app.toml"],
           "cwd":"/home/user","activation_token":null}}
```

`args` is the launching host's own command line; `activation_token` is `XDG_ACTIVATION_TOKEN` on Linux, otherwise `null`. Before `app.ready` the forwarder receives `-32002` and retries.

## Host and application methods

| Method | Params | Result |
| --- | --- | --- |
| `host.capabilities` | `{}` | Capability object (below) |
| `host.diagnostics` | `{}` | `phase`, `windows` (IDs), `pending_jobs`, `pending_transfers`, `pid`, `backend_pid`, `containment`, `logging` |
| `app.ready` | `{}` | `{"ready":true}` |
| `app.request_quit` | `{}` | `{"requested":true}` (outcome follows through `app.before_quit`) |
| `app.force_quit` | `{}` | `{"requested":true}` |
| `request.cancel` | `{"id":"<pending request id>"}` | `{"cancelled":bool}`; the cancelled request is answered with `-32006`. Usually sent as a notification. |

`host.capabilities` returns `capability_schema: 2` with top-level flags (`native`, `headless`, `max_windows`, `max_documents` (16), `renderer: "cpu"`, `css_profile: "ui4me-desktop-1.0"`, `platform`, `containment`, `window_positioning`, `limits` {`frame_bytes`, `pending_requests`, …}, `printing: false`, `tray: false`), a `features` map and the current `native_preferences`. Each feature is `{"implemented":bool,"available":true|false|null,"qualification":…,"scope":"…"}`, where `available: null` means "not observed yet". Feature keys: `window_constraints`, `window_observation`, `window_visibility`, `window_focus`, `window_position`, `window_restore`, `attention`, `clipboard_text`, `file_dialogs`, `external_open`, `reveal_file`, `native_preferences`, `ime`, `ime_purpose_hints`, `incoming_drop`, `accessibility`, `printing`, `tray`. Prefer `features` over the legacy top-level booleans.

## Windows

Window IDs are opaque strings assigned by the host (`"w1"`, `"w2"`, …). Sizes are logical CSS pixels, 1–8192 per side.

| Method | Params | Result |
| --- | --- | --- |
| `window.create` | `title` (default: app name, ≤ 4096 bytes), `width` (900), `height` (560), `visible` (true), `resizable` (true), `decorations` (true), `min_width`+`min_height`, `max_width`+`max_height` (each pair together) | Window description |
| `window.get` | `window_id` | Window description |
| `window.list` | `{}` | Array of window descriptions |
| `window.set` | `window_id` plus any of `title`, `width`, `height`, `visible`, `resizable`, `decorations`, `minimized`, `maximized`, `fullscreen`, `focus`, `x`+`y`, `min_width`+`min_height`, `max_width`+`max_height` (a constraint pair set to `null` removes it) | Window description |
| `window.request_attention` | `window_id`, `kind`: `"critical"`, `"informational"` or `null` (clear) | `{"requested":true,"kind":…,"presented":null,"policy":…}` |
| `window.request_close` | `window_id` | `{"requested":true}`; the close decision follows |

A window description contains `window_id`, `title`, `width`, `height`, `visible`, `native`, `scale_factor`, `physical_size`, `requested` {`visible`, `resizable`, `decorations`, `size_constraints`}, `observed` {`focused`, `minimized`, `maximized`, `fullscreen`, `resizable`, `decorations`, `visible`, `theme`, `position` (physical pixels)} and `closing`. Observed values are `null` when the platform cannot report them.

Wayland refusals (error `-32001`, see [platforms](../guides/platforms.md)): creating a hidden window, `visible` on an existing window, `focus: true`, `x`/`y`, `minimized: false`, and `request_attention` with `kind: null`. `window.create` beyond `max_windows` returns `-32005`.

## Documents

Each window holds at most one document; a host admits at most 16 live documents. Document IDs (`"d1"`, `"d2"`, …) are never reused. Every document request names both `window_id` and `document_id`; a stale `document_id` (after a reload or unload) fails with `-32004`, so a delayed callback can never modify a replacement document.

| Method | Params | Result |
| --- | --- | --- |
| `document.load` | `window_id`, exactly one of `html` (string ≤ 1 MiB) or `path` (UTF-8 file ≤ 1 MiB; relative to the backend `cwd`), optional `asset_root` (directory; defaults to the HTML file's directory) | `{"window_id","document_id","loaded":true,"source","asset_root","renderer":"cpu","pending_resources"}` |
| `document.get` | `window_id` | Same description, or `{"document_id":null,"loaded":false}` |
| `document.unload` | `window_id`, `document_id` | `{"document_id":null,"loaded":false}` |
| `document.command` | `window_id`, `document_id`, `command` | Command-specific result |

A failed `document.load` leaves the previous document in place.

### `document.command`

`command` is an object whose `op` selects the operation. `element` is the HTML `id` of an element. `node` is an opaque, generation-checked node handle returned by `query_nodes`, `node_of`, `get_node` or `node_children`; it never targets a node of a replaced document. Many operations accept either `element` or `node`.

| Area | `op` values |
| --- | --- |
| Text, values, attributes | `set_text`, `get_text`, `append_text`, `get_value`, `set_value`, `get_checked`, `set_checked`, `set_attribute`, `remove_attribute`, `get_attribute`, `get_attributes`, `add_class`, `remove_class`, `has_element` |
| Trusted markup | `replace_children_html`, `insert_html` (`position`: `beforebegin`, `afterbegin`, `beforeend`, `afterend`), `remove_element`, `move_element` (`target`, `position`), `trim_children` (`keep`, `from`: `start`/`end`), `get_html` (`outer`, byte `offset`/`limit` paging) |
| Batches | `batch` (`ops`: up to 1024 of `set_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `remove_element`, `move_element`, `append_text`, `trim_children` by `element`), `node_batch` (the same operations by `node`, with `target_node` for moves) |
| Queries and nodes | `query` / `query_nodes` (`selector`, `offset`, `limit` 1–4096, default 1024), `node_of`, `get_node`, `node_children` (`offset`, `limit` default 256), `element_at_point`, `text_at_point`, `text_rects`, `get_rect` |
| Styles | `add_stylesheet` (`css`), `replace_stylesheet` (`sheet`, `css`), `remove_stylesheet`, `list_stylesheets`, `get_computed_style` (`properties`), `css_profile`, `explain` (`property`) |
| Focus and editing | `focus`, `blur`, `get_focused_element`, `get_control_selection`, `set_control_selection` (`start`, `end`, `direction`), `replace_control_range`, `edit_command` (`undo`, `redo`, `cut`, `copy`, `paste`, `select_all`, `delete`), `get_selection`, `clear_selection`, `select_text`, `copy_selection` |
| Forms | `get_validity`, `check_validity`, `set_custom_validity`, `reset_form`, `request_submit`, `get_form_data` (optional `submitter`/`submitter_node`), `get_options`, `set_selected_values`, `get_files` |
| Default actions | `set_default_action` (`element`, `event`, optional `key`, `shift`, `ctrl`, `alt`, `meta`, `prevent`) |
| Scrolling | `get_scroll`, `scroll_to` (`x`, `y`), `scroll_by` (`dx`, `dy`), `scroll_into_view` (`block`, `inline`: default `nearest`), `set_scroll_follow` (`follow`); `behavior`: `auto`, `instant` or `smooth` |
| Dialogs and popovers | `show_dialog` (`modal`, default true), `close_dialog` (`return_value`), `get_dialog`, `show_popover`, `hide_popover`, `toggle_popover`, `get_popover` |
| Pointer | `set_pointer_capture` (`pointer_id` default 1, `capture`), `get_pointer_capture` |
| Environment | `set_environment` (`color_scheme`, `reduced_motion` overrides), `get_environment` |
| Resources | `register_resource_bundle` (`name`, `entries`: up to 256 `{path, data_url}`, ≤ 1 MiB decoded per registration), `unregister_resource_bundle`, `get_resource_stats`, `purge_unused_resources` (`url`) |
| Inspection | `get_accessibility_tree`, `get_diagnostics` |

`set_text`, `append_text` and `set_value` treat their input as plain text. `insert_html` and `replace_children_html` parse markup and must only receive trusted HTML. Renderer errors carry `data.engine_code` and map to the protocol codes below. Any result larger than the 2 MiB frame returns `-32002`; page through `get_html` or `query_nodes` instead.

### Document notifications

```json
{"jsonrpc":"2.0","method":"document.event",
 "params":{"window_id":"w1","document_id":"d3","event":{"type":"click", …}}}
```

`event` is a renderer event object with a `type` field; events from a replaced document are not delivered. The event types are listed in the [HTML and CSS scope](../concepts/html-css-scope.md#events-delivered-to-the-backend). The host also emits `{"type":"environment_changed","environment":{…}}` when the desktop preferences change.

`document.error` `{"window_id","document_id","message"}` (message ≤ 2048 characters) reports asynchronous document failures, such as a failed resource load or an event that exceeded the frame limit.

## Live bitmaps

A backend can stream RGBA pixels into an `<img src="ui4me-bitmap://<bitmap_id>">` of a loaded document through a shared file.

| Method | Params | Result |
| --- | --- | --- |
| `bitmap.create` | `window_id`, `document_id`, `bitmap_id` (1–64 of `A-Z a-z 0-9 _ -`), `width`, `height` (1–4096), `slots` (2–3, default 2), `pixel_format` (`"RGBA8"` only) | `{"bitmap_id","width","height","stride","pixel_format":"RGBA8","slots","slot_bytes","path","owner"}` |
| `bitmap.present` | `window_id`, `document_id`, `bitmap_id`, `sequence` (> 0, increasing), `slot` | `{"sequence","presented_to_engine":bool,"slot_released":true}` |
| `bitmap.close` | `window_id`, `document_id`, `bitmap_id` | `{"closed":bool}` |

The backend writes unpremultiplied sRGB RGBA8 rows (`stride = width × 4`) into `path` at byte offset `slot × slot_bytes`, then calls `bitmap.present`. The host reads the slot before answering, so the slot may be reused after the response. A `sequence` not greater than the last presented one is ignored (`presented_to_engine: false`). The file lives in `$XDG_RUNTIME_DIR` (Linux) or the temp directory and is unlinked after the first present, so the backend must keep its own handle open from the start. Limits: 8 bitmaps and 64 MiB of slots per document.

## Native services

| Method | Params | Result |
| --- | --- | --- |
| `clipboard.write_text` | `text` (≤ 1 MiB) | `{"written":true}` |
| `clipboard.read_text` | `{}` (must be a request) | `{"text":"…"}`; times out with `-32009` |
| `dialog.open_files` | `window_id`, optional `title`, `directory`, `filters` [`{name, extensions}`], `multiple` (default true) | `{"cancelled":bool,"paths":[…]}` |
| `dialog.save_file` | `window_id`, optional `title`, `directory`, `file_name`, `filters` | `{"cancelled":bool,"paths":[…]}` |
| `dialog.choose_folder` | `window_id`, optional `title`, `directory` | `{"cancelled":bool,"paths":[…]}` |
| `desktop.open_external` | `target`: `http`/`https`/`mailto`/`file` URI or absolute path; optional `window_id` (parent) | `{"accepted":bool,"cancelled":bool,"parent_forwarded":bool,"presented":null}` |
| `desktop.reveal_file` | `path` (absolute), optional `window_id` | Same shape as `open_external` |
| `desktop.paths` | `{}` | Per-app `config`, `data`, `cache`, `state`, `runtime` directories (XDG on Linux; Roaming/Local AppData on Windows) |
| `desktop.preferences` | `{}` | `{"color_scheme":"dark"\|"light"\|null,"reduced_motion","high_contrast","available","source","revision","scope"}` |

Dialogs, external open and reveal are answered when the user or the OS service finishes; they have no implicit timeout. Each window can have one native dialog at a time (`-32005` otherwise), and at most 16 such jobs run at once. Dialog results are limited to 1024 paths and 1 MiB, and paths must be Unicode. On Windows, `open_external` refuses executable file types (`-32003`); other URI schemes are refused everywhere. In headless mode these services return `-32001`.

`desktop.preferences_changed` is a notification with the same object as `desktop.preferences`, sent whenever the observed desktop preferences change.

## Window events

```json
{"jsonrpc":"2.0","method":"window.event",
 "params":{"window_id":"w1","type":"resized",
           "data":{"physical_width":1800,"physical_height":1120,"scale_factor":2.0}}}
```

| `type` | `data` |
| --- | --- |
| `resized` | `physical_width`, `physical_height`, `scale_factor` |
| `scale_changed` | `scale_factor` |
| `focused` | `focused` |
| `key` | `key`, `state` (`pressed`/`released`), `repeat`, `physical_key` |
| `close_requested` | `reason` |
| `closed` | `{}` |
| `text`, `dropped_text`, `dropped_files` | Only for the built-in diagnostic probe surface (a window without a document) |

Drops onto a document are delivered as `document.event`.

## Error codes

| Code | Name | Typical cause |
| --- | --- | --- |
| -32700 | parse error | Reserved by JSON-RPC; framing errors close the connection instead |
| -32600 | invalid request | Not a JSON-RPC 2.0 object, bad `id` or `method`, unknown top-level member |
| -32601 | method not found | Unknown method (`data.method`) |
| -32602 | invalid params | Unknown or malformed parameter, value out of range |
| -32603 | internal error | Unexpected host or renderer failure |
| -32001 | unsupported | Not possible on this platform or in headless mode |
| -32002 | unavailable | Native service missing or failed; result too large for a frame; primary instance still starting |
| -32003 | denied | Refused by policy (URI scheme, executable file type) |
| -32004 | closed | Window, document or bitmap no longer exists, or is closing |
| -32005 | busy | A limit was reached (windows, documents, pending requests, one dialog per window, queues) |
| -32006 | cancelled | The request was cancelled with `request.cancel` |
| -32007 | disconnected | Connection lost (client side) |
| -32008 | invalid state | Wrong lifecycle phase (`app.ready` too early or twice, application shutting down) |
| -32009 | timeout | A native data transfer did not complete |
| -32010 | version mismatch | `host.hello` with a protocol version other than 1 |
| -32011 | unauthenticated | Request before `host.hello`, or invalid token |

## Writing a binding: checklist

* Read `UI4ME_ENDPOINT` and `UI4ME_TOKEN`, connect, send `host.hello` first, then scrub the variables from the process environment.
* Run a reader and a writer independently of application callbacks, and do not block the reader on user code. The host may send `window.before_close` or `app.before_quit` at any time; answer them (default `{"allow": true}`) promptly.
* Use string request IDs that are unique while pending; send `request.cancel` when abandoning a request.
* Create a window and send `app.ready` within `startup_timeout_ms`.
* On `app.shutdown`, finish quickly and exit the process.
* Treat document and node identities as short-lived; expect `-32004` after reloads.

See also: [lifecycle](../concepts/lifecycle.md), [architecture](../concepts/architecture.md), [host CLI and manifest](host-cli.md).
