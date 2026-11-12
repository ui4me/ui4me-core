# Hosted documents: UI4ME Core

This is the document extension to the authenticated, bounded local protocol in
[PROTOCOL.md](PROTOCOL.md). The transport protocol remains version **1**; the host
and candidate product version is **1.0.0-rc.1**. The complete added node, editing,
form and resource operations are in [CORE_1.0_PROTOCOL.md](CORE_1.0_PROTOCOL.md).
The Rust host owns the native
event loop, window lifetime, editing, focus, rendering and desktop services.
Python is an ordinary managed backend process. It does not own a native window
or import a native rendering library.

## The smallest application

Copy `client.py` and `application.py` beside your Python backend, then launch it
through an application manifest. `tools/run_renderer.py` and `tools/install.py`
already supply complete examples and pin the actual Python interpreter.

```python
from pathlib import Path
from application import Application

with Application.from_environment() as app:
    window = app.open_file(Path(__file__).with_name("index.html"),
                           title="My desktop application", width=900, height=600)
    document = window.document
    document.on("click", lambda event: document.set_text("message", "Hello from Python"),
                element="hello")
    app.run()
```

```html
<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><link rel="stylesheet" href="style.css"></head>
  <body><button id="hello">Say hello</button><p id="message">Ready</p></body>
</html>
```

HTML/CSS and local assets describe the document. Backend callbacks perform
application actions. There is no embedded JavaScript interpreter, HTTP server,
browser navigation context, remote asset loader or webview.

## Document lifetime

| Method | Parameters | Result |
|---|---|---|
| `document.load` | `window_id`, exactly one of `html` or `path`; optional `asset_root` | The loaded document descriptor below |
| `document.get` | `window_id` | Current descriptor, or `{window_id, document_id:null, loaded:false}` |
| `document.unload` | `window_id`, `document_id` | `{window_id, document_id:null, loaded:false}` |
| `document.command` | `window_id`, `document_id`, `command` | Operation-specific result |

Example load result:

```json
{
  "window_id": "w-1",
  "document_id": "document-1",
  "loaded": true,
  "source": "/path/to/index.html",
  "asset_root": "/path/to",
  "renderer": "cpu",
  "pending_resources": true
}
```

IDs are opaque strings; the spellings above are illustrative. Every successful
load allocates a new host document ID. A command must name both its window and
document. A delayed callback for an older document cannot mutate its replacement,
even if the new HTML reuses all the same element IDs. An unloaded/closed/retired
document produces an explicit error. The Python `Document` also rejects locally
retired references with `StaleDocumentError` and discards their queued events.

The old document remains installed if source reading, parsing, admission or
viewport setup fails. Asynchronous image/font/stylesheet failure after an
accepted load is reported through diagnostics; it does not restore an older
document. `pending_resources` indicates outstanding loads, not visual correctness.

`path` names a UTF-8 regular HTML file. Relative paths resolve against the
manifest's backend working directory. The default asset root for a file is its
parent directory. In-memory `html` has no implicit asset root; supply a directory
explicitly for relative local assets. Paths are canonicalized and must be Unicode.
Keep every intended asset inside that root. Loading an HTML file is an
application operation, not arbitrary browser navigation.

Unknown request fields are errors. In particular, donor fields such as `window`
inside a command are not accepted.

## Commands

The common envelope is:

```json
{
  "window_id": "opaque-window-id",
  "document_id": "opaque-document-id",
  "command": {"op": "set_text", "element": "message", "text": "Updated"}
}
```

Unless stated otherwise, mutations return `{updated:true}`. `element` is an HTML
`id`, not a CSS selector. Node references are opaque, generation-checked references
obtained from inspection operations. Do not manufacture or persist them across
document replacement or node removal.

### Content and selectors

| Operation | Fields | Result / semantics |
|---|---|---|
| `set_text`, `append_text` | `element`, `text` | Replace child content with text / append text |
| `get_text` | `element` | `{text}` |
| `set_attribute` | `element`, `name`, `value` | Set a string attribute |
| `remove_attribute` | `element`, `name` | Remove the attribute |
| `get_attribute` | `element`, `name` | `{value:string|null}` |
| `get_attributes` | `element` | `{attributes:{name:value,...}}` |
| `add_class`, `remove_class` | `element`, `name` | Update a class token |
| `has_element` | `element` | `{exists:boolean}` |
| `query` | `selector`, optional `offset=0`, `limit=1024` | `{ids,total,matched_nodes,offset,limit,truncated}` |
| `replace_children_html` | `element`, `html` | Parse a fragment in its target context |
| `insert_html` | `element`, `position`, `html` | Position: `beforebegin`, `afterbegin`, `beforeend`, `afterend` |
| `remove_element` | `element` | Remove the target subtree |
| `move_element` | `element`, `target`, `position` | Move an existing node; the same insertion positions apply |
| `trim_children` | `element`, `keep`, optional `from="start"` | Remove excess children from `start` or `end` |
| `get_html` | optional `element` or `node`, `outer=false`, `offset=0`, `limit` | `{html,total_bytes,offset,next_offset,truncated}` |
| `batch` | `ops` | One atomic transaction of the allowed DOM operations below |

`query` returns only matched elements which have IDs. `total` is the number of
those identified matches, while `matched_nodes` also counts anonymous matches.
Pagination applies to `ids`; `limit` is 1–4096. For anonymous elements use the
new `query_nodes` and node mutation API rather than assuming `matched_nodes == total`.

`get_html` offsets and limits are **UTF-8 bytes**, not Python characters or UTF-16
units. An offset must be a valid character boundary within the serialized text.
The optional limit is 4–262144 bytes; the host rounds an ending boundary back to a
complete character. Continue at the returned `next_offset`, which is null at
the end. Omitting `limit` requests the complete remainder. An encoded result
which cannot fit the transport frame fails; the host never silently truncates it.
Avoid mutating the document between pages if a consistent serialization is needed.

The atomic batch whitelist is `set_text`, `append_text`, `set_attribute`,
`remove_attribute`, `add_class`, `remove_class`, `replace_children_html`,
`insert_html`, `remove_element`, `move_element`, and `trim_children`, with the
same fields as their individual commands. All entries are validated before
commit. If any entry fails, earlier changes in the transaction are rolled back.
A batch admits at most 1024 entries. Form values, focus, clipboard, file dialogs,
stylesheets and operating-system side effects are deliberately outside this
transaction. A client timeout still means an unknown outcome, not rollback.

### Controls, focus and selection

| Operation | Fields | Result / semantics |
|---|---|---|
| `set_value`, `get_value` | `element`; setter also `value` | Setter updates the control; getter returns `{value}` |
| `set_checked`, `get_checked` | `element`; setter also `checked` | Checkbox/radio state; getter returns `{checked}` |
| `focus` | `element` or `node` | Native document focus, independent of Python callbacks |
| `get_options` | `element` | `{options:[{value,label,selected,disabled},...]}` |
| `set_selected_values` | `element`, `values:[string,...]` | Select control values |
| `get_validity` | `element` | `{will_validate,valid,flags,message}`; no events |
| `check_validity` | `element` | `{valid,invalid:[identified control IDs],count}`; emits `invalid`, does not submit or focus |
| `get_files` | `element` | `{files:[Unicode local paths]}` selected through the native file input picker |
| `get_selection` | none | `{text}` from the current plain-text selection |
| `select_text` | optional `element` | Select rendered text in the target/document |
| `clear_selection` | none | Clear the selection |
| `copy_selection` | none | `{text}` and a queued native clipboard write |
| `set_pointer_capture` | `element`, optional `pointer_id=1`, `capture=true` | `{accepted}`; requires an active supported pointer button |
| `get_pointer_capture` | none | `{element:string|null}` |

Native editing, keyboard navigation and selection operate on the host thread.
`set_value` is a programmatic value change, not a substitute for exercising real
keyboard/IME input. File paths cannot be injected through `set_value` as a file
picker shortcut. Ordinary desktop clipboard calls remain the host's
`clipboard.*` methods; there is no second document clipboard transport. Rich
HTML clipboard and rich `contenteditable` editing are outside this release.

### Scrolling and document overlays

| Operation | Fields | Result / semantics |
|---|---|---|
| `get_scroll` | optional `element` | Scroll state |
| `scroll_to` | optional `element`, `x`, `y`, `behavior="auto"` | Updated scroll state |
| `scroll_by` | optional `element`, `dx=0`, `dy=0`, `behavior="auto"` | Updated scroll state |
| `scroll_into_view` | `element`, optional `block="nearest"`, `inline="nearest"`, `behavior="auto"` | Align target within its scroll ancestors |
| `set_scroll_follow` | optional `element`, `follow=true` | Follow new content where supported |
| `show_dialog` | `element`, optional `modal=true` | Show an HTML `<dialog>` |
| `close_dialog` | `element`, optional `return_value` | Close an HTML dialog |
| `get_dialog` | `element` | `{open,modal,return_value}` |
| `show_popover`, `hide_popover` | `element` | Open/close a document popover |
| `toggle_popover` | `element` | `{open}` |
| `get_popover` | `element` | `{open,kind}` |

Scroll state is `{x,y,min_x,min_y,max_x,max_y,width,height,scrollable,animating}`.
Coordinates are CSS pixels. Scroll behavior is `auto`, `instant` or `smooth`;
alignment values are `start`, `center`, `end` or `nearest`. Omitting `element`
selects the document viewport where applicable. HTML dialogs/popovers belong to
the document's top layer; `dialog.open_files` and other native desktop dialogs
remain separate host methods with their existing parenting/cancellation rules.

### Styles, preferences and diagnostics

| Operation | Fields | Result / semantics |
|---|---|---|
| `add_stylesheet` | `css` | `{sheet,diagnostics}`; retain the integer attachment ID |
| `replace_stylesheet` | `sheet`, `css` | `{sheet,diagnostics}` |
| `remove_stylesheet` | `sheet` | Detach that application stylesheet |
| `list_stylesheets` | none | `{sheets:[{kind,sheet,node,element,href,media,length},...]}` |
| `get_environment` | none | Effective preferences, sources, scale and high contrast |
| `set_environment` | optional `color_scheme`, `reduced_motion` | Effective environment after changing application overrides |
| `get_computed_style` | optional `element` or `node`; `properties:[name,...]` | `{schema:1,properties:{name:{status,value?,inherited?,feature?,reason?}}}` |
| `get_rect` | `element` | `{rect:null|{x,y,width,height}}` |
| `text_rects` | optional `element` or `node`, `start`, `end` | `{text,length,rects:[{x,y,width,height,visible},...]}` |
| `text_at_point` | `x`, `y` | `{hit:null|{element,offset,block}}` |
| `get_accessibility_tree` | none | `{tree}` semantic snapshot; does not prove native screen-reader behavior |
| `get_diagnostics` | none | `{diagnostics,css_profile_version}` bounded engine snapshot |
| `css_profile` | none | `{version,features:[{id,state},...]}` implemented/excluded CSS profile |
| `explain` | optional `element` or `node`, `property` | Structured style/layout explanation from the engine |

`color_scheme` accepts `light`, `dark` or `system`. `reduced_motion` accepts
`reduce`, `no-preference` or `system`; `system` removes the application override.
The effective result identifies whether the engine actually has a platform
value or is using its default. Do not infer unsupported platform preference
integration from the existence of a setter. Font substitution and rasterization
also depend on the fonts available in the target environment; bundled local
fonts make intended typography reproducible.

Text geometry offsets (`start`, `end`, returned `length`/hit `offset`) are **UTF-16
units**, matching the engine's text/accessibility model. Rectangles use viewport
CSS pixels; the host translates them to native device/IME coordinates. These
offsets are different from `get_html`'s UTF-8 byte offsets.

Computed style distinguishes `supported`, `unsupported` and `invalid` properties.
Parsing CSS syntax is not a promise that every browser property paints identically.
Use the profile, diagnostics and the browser-reference fixtures together. Internal
diagnostic and explanation detail can evolve; application behavior should rely on
document operations and explicit error codes instead of parsing diagnostic prose.

### Node inspection

| Operation | Fields | Result |
|---|---|---|
| `node_of` | optional `element` | `{node}`; omitted element means document node |
| `get_node` | `node` | Node reference/type/parent/child count, siblings where applicable, and kind-specific name/ID/attributes/text/template content |
| `node_children` | `node`, optional `offset=0`, `limit=256` | `{children:[{node,type,name,id},...],total}`; limit 1–4096 |

Use these APIs when anonymous text/elements matter. A CSS query intentionally
does not synthesize HTML IDs.

## Event delivery and default behavior

The host emits:

```json
{
  "jsonrpc": "2.0",
  "method": "document.event",
  "params": {
    "window_id": "opaque-window-id",
    "document_id": "opaque-document-id",
    "event": {"type": "input", "element": "name", "path": ["name", "entry-form"], "value": "Edited"}
  }
}
```

An event carries its native engine type, nearest identified target `element`,
identified propagation `path`, and type-specific fields such as `value`, key,
pointer coordinates or modifiers. Non-bubbling events have only their target
route. Internal node identity fields can accompany diagnostics; use the public
element/path and host document identity for routing. Types include click,
input/change, focus/blur/focusin/focusout, keyboard, pointer, wheel, scroll,
composition, form submit/reset/invalid, dialog/popover and CSS animation events.
Hosted documents use canonical input/change behavior: text editing emits one
`input` notification per edit; a committed value change emits `change` at the
appropriate commit/blur boundary. There is no extra synthetic `change` per
keystroke or renamed `commit` alias. No browser JavaScript event API is embedded
in the process.

Asynchronous native failures emit `document.error` with
`{window_id,document_id,message,fatal?}`. Examples include a failed native clipboard or
file operation and an event which cannot fit the bounded encoded transport.
The source Python facade logs a bounded message to stderr and dispatches it as
`document.on("error", callback)`, with `type:"error"`, `message` and `fatal`. Errors from
retired document generations are discarded. They are not successful operation
results; record and investigate them during native qualification.

`fatal:true` means the host has unloaded that failed renderer after a paint
failure and restored its diagnostic surface. The native window remains alive.
After the error callback, the Python facade retires the failed document; if the
callback successfully loaded a replacement document, that replacement is kept.
A nonfatal error does not unload the document.

The source Python facade dispatches application event notifications in transport
order on **one notification worker** with a bounded queue. Its independent
reader/writer threads continue processing replies, so a callback may perform a
synchronous document RPC. A separate four-worker host-request pool processes
close/quit policy and shutdown independently. Waiting in a notification callback
for a native picker therefore does not prevent native window close or quit from
canceling that picker. `app.shutdown` bypasses the notification queue, and client
close does not wait for arbitrary blocked application callbacks.

Application event callbacks are serial; **lifecycle policy callbacks can run
concurrently with them**. Protect shared application state accordingly, and keep
close/quit decisions brief. Long notification callbacks delay later application
notifications; put long application work on an application worker and post
bounded document updates. They do not own or stop the Rust native event loop.

An event payload is a snapshot of the native event, not a live node handle. After
an intervening DOM mutation, its routing `element` ID may have been removed or
reused before a later queued callback runs. The host checks native node generation
when queuing events, but cannot undo application mutations made afterward. Node
inspection references remain generation-checked; document replacement filtering
also remains active. Do not interpret an old routing ID as proof that its original
node is still present.

The lower-level `RpcClient` preserves its original callback-pool behavior unless
`ordered_notifications=True` is selected. Applications using it directly must
account for concurrency. Its queues are bounded; consult
`dropped_notifications` when diagnosing an overloaded backend rather than assuming
unlimited reliable event buffering.

Notifications arrive after native dispatch. Returning a value from a Python event
callback cannot cancel a native action that already happened. Register prevention
before the relevant input:

```python
document.prevent_default("entry-form", "submit")
document.on("submit", save_form, element="entry-form")
```

The raw `set_default_action` fields are `element`, `event`, optional `key`, `shift`,
`ctrl`, `alt`, `meta`, and `prevent=true`. A false `prevent` removes the rule.
Only the engine's supported default actions/rules are accepted. This is not an
arbitrary cross-process synchronous DOM event interception mechanism.

## Bounds and failure behavior

The existing frame maximum is 2 MiB **after JSON encoding**. HTML source is bounded
to 1 MiB, the host admits at most 16 live documents, DOM batches to 1024 operations,
and paged query/child inspection to 4096 entries. Renderer DOM/resource/cache
bounds are enforced inside the engine as well. The manifest may impose a lower
window bound. A finite result too large for the frame is an error, not a partial
success. Query and HTML pagination let callers request a smaller response.

Native document state and mutations run on the host owner thread. Loading a
document does not transfer ownership of the application lifecycle to its backend.
Closing/replacing documents invalidates their resources, callbacks and native
targets. Queued asynchronous results must be checked against the current native
window/document generation before use. OS dialogs, shutdown, supervision,
single-instance forwarding and the emergency quit path retain the v0.1 host
contracts; verify them again with the renderer attached.

Diagnostic snapshots can retain messages from earlier pipeline stages; a recorded
`RESOURCE_PENDING` message can remain after that asset has finished. Use the
descriptor's live `pending_resources` field to determine whether loading is still
outstanding, and inspect the rendered result and any actual resource errors.

Headless mode constructs and exercises the real parser/style/layout/document
engine. It cannot qualify native input, desktop clipboard, IME positioning,
Wayland presentation, Windows TSF/UIA, screen-reader speech or lifecycle behavior
under an actual compositor. The standalone `render` CLI produces a CPU PNG for
development/reference comparison; it is not a printing/PDF application API.
