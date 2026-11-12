# UI4ME host protocol, wire version 1

UI4ME Core 0.2 retains this native-service wire contract and adds the document
methods in [PROTOCOL_DOCUMENTS.md](PROTOCOL_DOCUMENTS.md). Product version and
wire framing version are separate. `host.capabilities` now advertises
`html_css: true`, the `ui4me-desktop-0.2` CSS profile, CPU rendering, and a
16-document limit. The native probe remains available.


This document describes the implemented local protocol. It is independent of
Python, Rust FFI and any future HTML/CSS renderer.

## Connection and framing

The host launches one backend with these environment values:

| Name | Meaning |
| --- | --- |
| UI4ME_ENDPOINT | Private Unix socket path or Windows named-pipe name |
| UI4ME_TOKEN | Fresh backend authentication credential; never log it |
| UI4ME_APP_ID | Application identity from the manifest |
| UI4ME_PROTOCOL_VERSION | Protocol version 1 |

stdout and stderr are ordinary application logs. Neither is an RPC channel.
There is no newline-delimited framing and no HTTP.

Each frame consists of:

1. Four bytes: unsigned big-endian length of the following UTF-8 byte sequence.
2. Exactly that many bytes containing one JSON object.

An empty frame is invalid. The maximum body length is 2,097,152 bytes.
Reject an excessive length before allocating or reading the body. EOF before a
new header is a disconnect; EOF inside a header/body is a truncated frame.
Clients must handle partial reads and writes.

The JSON-RPC 2.0 profile accepts named parameter objects and nonempty string
request IDs. Batches, numeric/null request IDs and positional parameters are
not supported. Omit the id field for a notification. Unknown envelope fields,
invalid UTF-8 and invalid JSON are rejected.

IDs are at most 128 bytes. Methods are at most 128 bytes and use ASCII letters,
digits, dot, underscore or hyphen. The reserved rpc. prefix is not accepted.
Do not reuse an ID while it is pending. Duplicate pending IDs terminate the
backend session to prevent ambiguous responses.

The reference client has independent reader, writer and callback execution.
A callback may make another RPC and wait for its result because the reader
keeps processing responses. Callbacks are not invoked on the transport reader.

## Authenticate first

Send a request such as:

    {
      "jsonrpc": "2.0",
      "id": "python:1",
      "method": "host.hello",
      "params": {
        "token": "<from UI4ME_TOKEN>",
        "protocol_version": 1,
        "client": {"name": "my-backend", "version": "0.1"}
      }
    }

The result contains protocol_version, host_version, app_id and capabilities.
Only one backend may authenticate. An invalid credential or version does not
grant access. Unauthenticated connections are short-lived.

After authentication, create the initial window(s), register handlers for
window.before_close, app.before_quit and app.shutdown, then call app.ready.
Spawning a process or connecting is not application readiness.

## Request/response examples

    {"jsonrpc":"2.0","id":"p:2","method":"window.create","params":{"title":"Example","width":900,"height":560}}

    {"jsonrpc":"2.0","id":"p:2","result":{"window_id":"w1","title":"Example","width":900,"height":560,"visible":true,"native":true,"scale_factor":1.0,"physical_size":{"width":900,"height":560},"closing":false}}

    {"jsonrpc":"2.0","id":"p:3","method":"clipboard.read_text","params":{}}

    {"jsonrpc":"2.0","id":"p:3","result":{"text":"Hello"}}

Window descriptions report both requested/current logical dimensions and the
actual native physical surface size. A compositor may constrain size/focus.
Treat native events as the authoritative indication of observed changes.

## Backend-to-host methods

| Method | Parameters | Successful result |
| --- | --- | --- |
| host.hello | token, protocol_version, optional client metadata | Version, identity and capability object |
| host.capabilities | Empty object | Capabilities and limits |
| host.diagnostics | Empty object | Lifecycle phase, windows, pending work counts, process identities and containment |
| app.ready | Empty object | ready: true |
| app.request_quit | Empty object | requested: true; decision happens asynchronously |
| app.force_quit | Empty object | Terminal emergency operation; its acknowledgement is best effort because the requesting backend is terminated |
| window.create | title optional; width/height optional; visible/resizable optional | Window description |
| window.set | window_id plus supported fields listed below | Window description |
| window.get | window_id | Window description |
| window.list | Empty object | Array of window descriptions |
| window.request_close | window_id | requested: true; decision happens asynchronously |
| probe.set_text | window_id, text | updated: true |
| probe.get_text | window_id | text, cursor, anchor, preedit, focused |
| clipboard.write_text | text | written: true |
| clipboard.read_text | Empty object, request ID required | text |
| dialog.open_files | Dialog options below | cancelled and paths array |
| dialog.save_file | Dialog options below | cancelled and paths array (zero or one path) |
| dialog.choose_folder | Dialog options below | cancelled and paths array (zero or one path) |
| desktop.open_external | target | accepted: true, meaning OS handoff accepted |
| desktop.paths | Empty object | Per-application config/data/cache/state paths and platform-available runtime path |
| request.cancel | id of an outstanding backend-to-host request | cancelled: boolean, if sent as a request |

### Windows

Creation defaults to 900 × 560 logical units, visible and resizable. Logical
dimensions must be within 160..8192 by 120..8192; the diagnostic surface sets a
native minimum so its basic controls remain usable. The manifest limits the
number of application windows.

window.set accepts title, width, height, visible, resizable, minimized,
maximized, fullscreen, focus, and x/y. Position requires both x and y and is
only available on Windows. Unsupported placement on Wayland is an error, as is
minimized: false (xdg-shell cannot restore a minimized window; the user or
compositor does). A focus request on Wayland uses xdg-activation and may be
declined by compositor focus-stealing prevention.
Properties are native requests, not a promise that compositor policy can be
overridden.

The v0.1 protocol does not expose raw OS handles or arbitrary custom child/modal
window classes. Native file dialogs implement their own parent/modal behavior.

### Dialogs

Dialog parameters:

    {
      "window_id": "w1",
      "title": "Choose documents",
      "directory": "/home/user/Documents",
      "file_name": "report.txt",
      "filters": [
        {"name":"Text documents","extensions":["txt","md"]},
        {"name":"All files","extensions":["*"]}
      ]
    }

Only window_id is required. Use platform-native directory paths.
file_name is a filename, not a directory traversal path, and is used for save.
Extensions are literal suffixes without a leading dot; tar.gz is valid.
The special * extension includes all files. Arbitrary wildcard expressions
are not accepted.

At most one native dialog is active per window. User decisions have no short
RPC timeout by default. Cancellation is a real native cancellation request,
including cancellation caused by parent close, approved application quit or
backend failure. The host retains the parent until dialog teardown completes.

Normal user cancellation produces:

    {"cancelled":true,"paths":[]}

A request cancelled through request.cancel receives the cancelled RPC error;
late native completion is suppressed. A closing parent receives the closed
error. A save result chooses a path only; the application writes the data.

Protocol v1 requires Unicode paths. A selected non-Unicode native filename
returns an explicit unsupported error rather than a lossy path. A selection
is capped at 1024 entries and 1 MiB of path text.

### Clipboard and transfers

Plain text is capped at 1 MiB. Headless mode returns unsupported.
Native transfer requests have a deadline so vanished clipboard owners cannot
leave requests pending forever. Wayland can require an active focused surface
and a real input serial for clipboard ownership; tests must exercise real
cross-application transfer in the target desktop.

Incoming file drops emit Unicode local file paths. Other dropped URI schemes
and non-Unicode paths are not treated as usable file paths. Plain-text drops
emit dropped_text and are inserted into the diagnostic editor when they fit.

### Cancellation

The reference client's Future.cancel() sends:

    {"jsonrpc":"2.0","method":"request.cancel","params":{"id":"p:42"}}

If that operation is pending, its result becomes a cancelled error. Cancellation
does not undo an OS operation that already completed. Native teardown can finish
asynchronously, and continues counting against the host's resource limit until
the worker releases its resources.

## Host-to-backend requests

### window.before_close

    {"jsonrpc":"2.0","id":"decision:...","method":"window.before_close","params":{"window_id":"w1","reason":"native_close"}}

Answer:

    {"jsonrpc":"2.0","id":"decision:...","result":{"allow":true}}

Return allow: false to veto. A malformed result or error inside an otherwise
valid response is treated as a veto, not permission to destroy state.
A malformed response envelope is ignored and leaves the preflight pending.

### app.before_quit

This asks for one application-wide decision:

    {"jsonrpc":"2.0","id":"decision:...","method":"app.before_quit","params":{"reason":"backend_request"}}

Answer with the same allow result. The host does not close windows piecemeal
while waiting for this answer. Previously pending individual close decisions
are superseded. Stale responses cannot reopen or close a window.

A preflight may need user interaction. The host therefore does not auto-approve
a stalled callback. Its native input and emergency quit action remain available.

## Host notifications

| Method | Meaning |
| --- | --- |
| app.shutdown | Approved shutdown has begun; deadline_ms gives the grace period |
| app.activated | Another launch of this application in the same desktop session; includes launch args/cwd and optional activation token |
| request.cancelled | A host-origin decision was superseded; id and reason identify it |
| window.event | Native/probe event with window_id, type and data |

window.event types include resized, scale_changed, focused, key, text,
close_requested, closed, dropped_files and dropped_text. Key data includes key, state
(pressed/released), repeat and a diagnostic physical_key string.
Text data includes the committed text plus cursor/anchor UTF-8 byte offsets.
probe.get_text separately exposes current preedit for integration diagnostics.

The reference backend handles app.shutdown both as a notification and as a
request for client interoperability; this host sends a notification.

## Error codes

| Code | Name |
| ---: | --- |
| -32700 | Parse error |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid parameters |
| -32603 | Internal error |
| -32001 | Unsupported capability |
| -32002 | Unavailable service |
| -32003 | Denied operation |
| -32004 | Closed resource |
| -32005 | Busy / resource limit |
| -32006 | Cancelled request |
| -32007 | Disconnected peer |
| -32008 | Invalid lifecycle state |
| -32009 | Timeout |
| -32010 | Protocol version mismatch |
| -32011 | Unauthenticated |

Errors have code, message and optional data. Error text is diagnostic; branch
on the code rather than parsing English text.

## Failure and trust model

The backend is the user's application code, with the normal user's OS
permissions. This local protocol is not a sandbox for hostile application
logic. The private endpoint, session restriction and credential prevent
accidental cross-session control; resource bounds limit protocol accidents.

Disconnect, invalid framing, process crash and startup timeout are failure
events. A native host displays its own error state and remains able to quit.
A headless host exits unsuccessfully. There is no implicit reconnect/replay
or automatic restart of potentially side-effectful application logic.

Capability flags describe which implementation paths are enabled. The windows
flag includes the logical window registry used by headless tests; native: false
is authoritative about the absence of native surfaces. These flags are not
native QA certificates; consult the separate acceptance evidence.

Invalid UTF-8, invalid JSON and invalid frame lengths close the transport.
The parse-error constant is reserved in the shared error model; this transport
does not attempt to send -32700 after such a framing failure. If a valid service
result would exceed the frame limit after JSON escaping, it is replaced with a
small unavailable error. Oversized notifications are logged and dropped.

## Version 0.2 additions

See [PROTOCOL_DOCUMENTS.md](PROTOCOL_DOCUMENTS.md) for `document.load`,
`document.get`, `document.unload`, `document.command`, `document.event`, and
`document.error`. `dialog.open_files` additionally accepts `multiple`
(default `true`); HTML file inputs set this from the `multiple` attribute.

The protocol does not expose arbitrary renderer commands, pagination, printing,
rich contenteditable, JavaScript, or networking. Native clipboard data is plain
text. HTML event callbacks are asynchronous notifications; default-action rules
are registered before input, not returned from a later Python callback.
