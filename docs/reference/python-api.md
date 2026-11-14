# Python API reference

The `ui4me_core` package is a pure-Python client (standard library only, Python 3.10+) for the native UI4ME host. The Rust host owns the windows, the HTML/CSS engine, input, editing and the event loop; your Python code runs as a separate supervised process and talks to the host over authenticated local IPC. There is no browser, no JavaScript and no web view: everything in this page is a Python call that the host carries out natively.

```python
from ui4me_core import (Application, Window, Document, Node, LiveBitmap,
                        StaleDocumentError, StartupError,
                        RpcError, ProtocolError, DisconnectedError, host_binary)
```

New to UI4ME? Start with the [Python quickstart](../guides/quickstart-python.md), then use the [patterns guide](../guides/patterns.md) for recipes. This page lists every public call and what it does.

## Contents

- [Starting an application](#starting-an-application)
- [`Application`](#application)
- [`Window`](#window)
- [`Document`](#document)
- [Events](#events)
- [`Node`](#node)
- [`LiveBitmap`](#livebitmap)
- [Errors](#errors)
- [Threading and callbacks](#threading-and-callbacks)
- [Limits](#limits)
- [Command-line tools in the package](#command-line-tools-in-the-package)

## Starting an application

```python
from ui4me_core import Application


def main():
    with Application.start(name="My App") as app:
        app.open_html("<h1>Hello UI4ME</h1>")
        app.run()


if __name__ == "__main__":
    main()
```

Run it the way you run any Python script:

```sh
python app.py
uv run app.py
uv run python app.py
python -m mypackage.app
```

### `Application.start(...)`

```text
Application.start(*, name=None, app_id=None, version=None, icon=None,
                  instance="single", startup_timeout_ms=30000,
                  shutdown_timeout_ms=5000, max_windows=16) -> Application
```

| Option | Meaning |
| --- | --- |
| `name` | Application and default window name, 1–128 characters without control characters. Default: the script's file name (or the last part of the `-m` module name). |
| `app_id` | Reverse-DNS identity such as `"org.example.Notes"`: ASCII, at most 128 characters, at least two dot-separated parts, each starting with a letter and containing only letters, digits, `_` or `-`. Default: a stable ID derived from the script path, `local.ui4me.<name>.h<hash>`. Pass an explicit `app_id` for a real application. |
| `version` | Optional version text (same rules as `name`). |
| `icon` | Optional path to an application icon file. It must exist. |
| `instance` | `"single"` (default): launching the same app again hands the launch to the running instance, and the second launch exits with status 0. `"multiple"`: every launch is a separate application. |
| `startup_timeout_ms` | 1000–300000. How long the host waits for the backend to start. |
| `shutdown_timeout_ms` | 100–120000. How long an orderly shutdown may take before the host stops the backend. |
| `max_windows` | 1–64. Maximum number of windows this application may open. |

Invalid options raise `ValueError` in both processes (see below), before anything starts.

**How it works.** `start()` runs in one of two roles:

1. **Launcher** — the process you started. There is no host connection yet. `start()` writes a private temporary manifest naming the exact interpreter (`sys.executable`) and command line it was started with (interpreter options and the `-m module` form are preserved; a script path is made absolute), and runs the packaged native host. While the application runs it relays your backend's `print()` and stderr output to the console as plain text. Host warnings and errors are summarized as lines such as `ui4me-host error: ...`; set `UI4ME_VERBOSE=1` to see every host log record. When the host exits, the manifest is deleted and `start()` raises `SystemExit` with the host's exit status. **In the launcher, `start()` never returns.**
2. **Backend** — the host starts the same command again with `UI4ME_ENDPOINT` and `UI4ME_TOKEN` in the environment. Now `start()` connects (exactly like `Application.from_environment()`), removes the connection variables from `os.environ` so that child processes you start do not inherit the secret, and returns the `Application`.

Consequences for your code:

- **Everything before `Application.start()` runs twice**, once in each process. Keep imports and constants at the top, put side effects (opening files, network calls, printing banners) after `start()`, and guard the entry point with `if __name__ == "__main__":`.
- The program must be a script file or `python -m package.module`. `python -c`, code piped through standard input and the interactive interpreter raise `StartupError`.
- The backend has no standard input, so `input()` cannot be used. `print()` output still reaches your terminal through the launcher.
- Ctrl+C or SIGTERM in the terminal stops the host and the backend. If the launcher itself is killed, the host is stopped too (Linux: parent-death signal; Windows: a kill-on-close job object).
- If an exception escapes a `with Application.start(...)` block in the backend, its traceback is printed before the connection closes, so it is not lost when the host stops the failed backend.

**Exit status** of `python app.py`: 0 after an orderly shutdown; nonzero when the backend failed (the host then keeps its own application-error window open until the user closes it); 128 + the signal number when interrupted.

**Single instance.** With `instance="single"`, a second launch of the same `app_id` brings the running instance's window forward and the host sends an `app.activated` notification to the running backend. Observe it with the low-level client:

```python
def on_activated(params):
    print("The app was launched again")

app.rpc.on_notification("app.activated", on_activated)
```

**Environment variables.**

| Variable | Effect |
| --- | --- |
| `UI4ME_HOST_BINARY` | Use this host executable instead of the packaged one (development builds). |
| `UI4ME_VERBOSE=1` | Relay every host log record, not only warnings and errors. |
| `UI4ME_TEST_MODE=1` | Testing aid: the launcher runs the host with `--headless`, which exercises the protocol, document engine and lifecycle without a desktop. No window is shown, so the app must end itself (for example with `app.request_quit()`). |

### `Application.from_environment()`

The advanced entry point for a backend that the host starts from an explicit manifest (`ui4me-host run --app app.toml`). It connects to the host named by `UI4ME_ENDPOINT`/`UI4ME_TOKEN` and does not launch anything. Without those variables it raises `StartupError` with guidance. Unlike `start()`, it leaves the variables in `os.environ`. See [host CLI](host-cli.md) for the manifest format. `ui4me-new` writes such a manifest for you.

## `Application`

`Application` is a context manager: leaving the `with` block calls `app.close()`.

| Call | Returns | Purpose |
| --- | --- | --- |
| `app.create_window(*, title="UI4ME", width=900, height=600, visible=True, decorations=True, min_width=None, min_height=None, max_width=None, max_height=None)` | `Window` | Create an empty native window. Sizes are logical pixels; minimum/maximum constraints are 1–8192. |
| `app.open_file(path, **window_options)` | `Window` | Create a window and load a local HTML file. Relative CSS, images and fonts resolve from the file's folder. |
| `app.open_html(html, *, asset_root=None, **window_options)` | `Window` | Create a window and load an HTML string. Pass `asset_root` (a folder) if the HTML references local files. |
| `app.ready()` | `None` | Tell the host the backend is ready. Requires at least one window. `run()` calls it for you. |
| `app.run()` | `None` | Announce readiness, then block until the host shuts the application down. Raises `DisconnectedError` if the host disappears without an orderly shutdown. |
| `app.request_quit()` | `None` | Ask the host to quit. The quit policy (below) may refuse. |
| `app.on_before_quit(callback)` | `None` | `callback()` returns `True` to allow quitting, `False` to veto. |
| `app.clipboard_text()` | `str` | Read plain text from the system clipboard. |
| `app.set_clipboard_text(text)` | `None` | Write plain text to the system clipboard. |
| `app.capabilities()` | `dict` | What the host implements and what is currently available on this desktop. |
| `app.desktop_preferences()` | `dict` | Observed desktop preferences: `color_scheme` (`"dark"`, `"light"` or `None`), `reduced_motion`, `high_contrast` (`True`/`False`/`None` when unknown), `available`, `source`, `revision`, `scope`. |
| `app.on_desktop_preferences(callback)` | `None` | `callback(snapshot)` receives the same structure when the preferences change. |
| `app.open_external(target, *, window=None)` | `dict` | Ask the desktop to open a file path or URL with its default application. Result: `accepted`, `cancelled`, `parent_forwarded`, `presented`. |
| `app.reveal_file(path, *, window=None)` | `dict` | Show a file in the system file manager. |
| `app.close()` | `None` | Disconnect from the host and mark all windows and documents stale. |
| `app.rpc` | `RpcClient` | The low-level client (see [below](#low-level-client)). |

`open_file` and `open_html` accept the same keyword options as `create_window` (`title`, `width`, ...). If loading fails, the new window is closed and the error is raised.

Closing the last window is a request to quit: the quit policy runs, and the application ends if it allows. `app.request_quit()` from a callback is the normal way to end a headless test.

## `Window`

| Call | Returns | Purpose |
| --- | --- | --- |
| `window.id` | `str` | Opaque host window ID. |
| `window.document` | `Document` or `None` | The current document. |
| `window.closed` | `bool` | `True` after the window closed. |
| `window.load_file(path, *, asset_root=None)` | `Document` | Replace the document with a local HTML file. |
| `window.load_html(html, *, asset_root=None)` | `Document` | Replace the document with an HTML string. |
| `window.set(**properties)` | `dict` | Request native window changes (see below). |
| `window.state()` | `dict` | Window state, with separate `requested` and `observed` objects. |
| `window.request_attention(kind="informational")` | `dict` | Ask the desktop to draw attention to the window: `"informational"`, `"critical"`, or `None` to cancel. |
| `window.request_close()` | `None` | Ask to close the window. The close policy may refuse. |
| `window.on_before_close(callback)` | `None` | `callback()` returns `True` to allow closing this window, `False` to veto. Each window has its own policy. |
| `window.open_files(*, multiple=False, title=None, directory=None, filters=(), wait=True)` | `dict` or future | Native open-file dialog. |
| `window.save_file(*, file_name=None, title=None, directory=None, filters=(), wait=True)` | `dict` or future | Native save dialog. |
| `window.choose_folder(*, title=None, directory=None, wait=True)` | `dict` or future | Native folder picker. |

**`window.set(...)` properties:** `title`, `width`, `height`, `visible`, `resizable`, `decorations`, `minimized`, `maximized`, `fullscreen`, `focus`, `x`, `y`, `min_width`, `min_height`, `max_width`, `max_height`. Unknown names are rejected. A request is not proof that the desktop applied it: read `window.state()["observed"]`. On Wayland, a global window position cannot be set or read (`observed["position"]` is `None`) and focus requests are up to the compositor; Windows supports both. See [platforms](../guides/platforms.md).

**File dialogs.** Each filter is `{"name": "Text files", "extensions": ["txt", "md"]}`; extensions have no leading dot (`"*"` matches everything). With `wait=True` (the default) the call blocks, with no time limit, until the user chooses, and returns `{"cancelled": bool, "paths": [str, ...]}`. With `wait=False` it returns a `concurrent.futures.Future` immediately; `future.result()` gives the same dictionary, and `future.cancel()` asks the host to close the native dialog.

**Frameless windows.** With `decorations=False`, mark a title-bar area in HTML with `data-ui4me-drag` and resize edges with `data-ui4me-resize="n"`, `"s"`, `"e"`, `"w"`, `"ne"`, `"nw"`, `"se"` or `"sw"`. The host handles these synchronously during the real pointer press (a Python callback would be too late to start a native move). Buttons, inputs, links and anything inside `data-ui4me-no-drag` are never drag regions.

## `Document`

A `Document` is one loaded page in one window. Element arguments named `element` are **HTML `id` values**, not CSS selectors. Methods that say *`element` or `Node`* also accept a [`Node`](#node).

### Generations and stale documents

Every successful `load_file`, `load_html` or `unload` creates a new document generation. The previous `Document` object, its `Node` objects, its event subscriptions and its live bitmaps are retired. Using them raises `StaleDocumentError` instead of changing the new page, even if the new HTML reuses the same IDs. After loading, take `window.document` (or the returned value) and register callbacks again. A failed load leaves the previous document in place.

**Loading rejects active content.** `load_html`, `load_file`, `open_html` and `open_file` raise `RpcError` (code -32602) for HTML that contains `<script>`, `<iframe>`, `<object>` or `<embed>`, any `on…` event-handler attribute or `srcdoc`, `contenteditable` (other than `false`), `<input type="color">` or date/time input types, or duplicate or invalid `id` values. Keep behavior in Python and use `<input>`/`<textarea>` for editing.

| Call | Purpose |
| --- | --- |
| `doc.id`, `doc.window` | Host document ID and its window. |
| `doc.unload()` | Remove the document from the window (the window stays open, empty). |
| `doc.command(op, **fields)` | Send any typed document operation. See [protocol](protocol.md). Not a general DOM API. |
| `doc.diagnostics()` | Bounded engine diagnostics: CSS warnings, resource errors, `css_profile_version`. |

### Content and attributes

| Call | Purpose |
| --- | --- |
| `doc.get_text(element)` / `doc.set_text(element, text)` | Read/replace the text content. `set_text` never interprets HTML, so it is safe for user input. |
| `doc.set_attribute(element, name, value)` / `doc.remove_attribute(element, name)` | Change an attribute. |
| `doc.add_class(element, name)` / `doc.remove_class(element, name)` | Change one class token. |
| `doc.replace_html(element, html)` | Replace the element's children with parsed HTML. **Trusted markup only.** |
| `doc.insert_html(element, html, *, position="beforeend")` | Insert parsed HTML at `"beforebegin"`, `"afterbegin"`, `"beforeend"` or `"afterend"`. **Trusted markup only.** |
| `doc.remove(element)` | Remove the element and its subtree. |
| `doc.batch(operations)` | Apply several changes as one atomic transaction (see below). |
| `doc.query(selector, *, offset=0, limit=1024)` | CSS selector query. Returns `{"ids", "total", "matched_nodes", "offset", "limit", "truncated"}`; only elements with an `id` are listed in `ids`. `limit` is 1–4096. |

To put user text inside generated HTML, escape it first with `html.escape()`.

**Batches.** `doc.batch([...])` validates every entry before applying any of them; if one fails, nothing changes. At most 1024 entries. Each entry is a dictionary with an `"op"` and the same fields as the single command:

| `op` | Fields |
| --- | --- |
| `set_text`, `append_text` | `element`, `text` |
| `set_attribute` | `element`, `name`, `value` |
| `add_class`, `remove_class` | `element`, `name` |
| `insert_html` | `element`, `position`, `html` |
| `replace_children_html` | `element`, `html` (what `replace_html()` sends) |
| `remove_element` | `element` |
| `move_element` | `element`, `target` (another element ID), `position` |
| `trim_children` | `element`, `keep`, optional `from` (`"start"`, the default, removes the oldest children; `"end"` the newest) |

`remove_attribute` (fields `element`, `name`) is also accepted in a batch. Form values, checked state, focus, stylesheets, dialogs and clipboard are not batch operations; call their methods separately.

```python
document.batch([
    {"op": "set_text", "element": "status", "text": "Saved"},
    {"op": "add_class", "element": "status", "name": "success"},
])
```

### Form controls

| Call | Purpose |
| --- | --- |
| `doc.get_value(element)` / `doc.set_value(element, value)` | Value of an `<input>`, `<textarea>` or `<select>`. `set_value` is a programmatic change, not simulated typing. |
| `doc.get_checked(element)` / `doc.set_checked(element, checked)` | Checkbox/radio state. |
| `doc.options(element)` | `<select>` options: `[{"value", "label", "selected", "disabled"}, ...]`. |
| `doc.set_selected_values(element, values)` | Select options by value. |
| `doc.files(element)` | Local paths chosen through an `<input type="file">` (its picker is native). |
| `doc.form_data(form, *, submitter=None)` | `{"entries": [...], "submitter": ...}`. Entries keep document order and repeated names: text entries are `{"name", "kind": "text", "value"}`, file entries `{"name", "kind": "file", "path", "filename"}`. |
| `doc.request_submit(form, *, submitter=None)` | Validate and submit like a user would. Returns `{"submitted", "default_prevented"}`. An invalid form is not submitted. |
| `doc.reset_form(form)` | Reset controls to their defaults. Returns `{"reset", "default_prevented"}`. |
| `doc.validity(element)` | `{"will_validate", "valid", "flags", "message"}` without emitting events. |
| `doc.check_validity(element)` | `{"valid", "invalid": [ids], "count"}`; emits `invalid` events, does not submit. |
| `doc.set_custom_validity(element, message="")` | Set a custom error message; an empty message clears it. |

`form`, `submitter` and the `element` of `validity`-style calls may be IDs; `form_data`, `request_submit`, `reset_form` and `set_custom_validity` also accept a `Node`.

### Focus, selection and editing

| Call | Purpose |
| --- | --- |
| `doc.focus(element)` | Move keyboard focus (`element` or `Node`). |
| `doc.blur(element=None)` | Remove focus; without an argument, from whatever has focus. |
| `doc.focused_element()` | `{"node", "element"}` of the focused element, or `None`. |
| `doc.element_at_point(x, y)` | `{"node", "element"}` at viewport CSS-pixel coordinates, or `None`. |
| `doc.control_selection(element)` | `{"start", "end", "direction", "length", "node"}` of a text control. |
| `doc.set_control_selection(element, start, end, *, direction="none")` | Select a range; `direction` is `"none"`, `"forward"` or `"backward"`. |
| `doc.replace_control_range(element, replacement, *, start=None, end=None, selection_mode="preserve")` | Replace text through the native editor (keeps undo/redo and emits `input`). Without `start`/`end` the current selection is replaced. `selection_mode`: `"preserve"`, `"select"`, `"start"`, `"end"`. Result includes `changed`. |
| `doc.edit(command, element=None)` | Run `"undo"`, `"redo"`, `"cut"`, `"copy"`, `"paste"`, `"select_all"` or `"delete"` on the given or focused control. Returns `{"accepted", "clipboard_pending"}`. |

**Selection offsets are UTF-16 code units, not Python string indices.** In `"A😀B"` the emoji occupies offsets 1–3 and `B` starts at 3; offset 2 is rejected because it splits the emoji. For ASCII and most European text the two agree. Convert with `len(text[:i].encode("utf-16-le")) // 2`.

Clipboard edit commands complete asynchronously: `accepted` does not mean the operating system finished the transfer. A later failure arrives as an [`error` event](#events).

### Styles and appearance

| Call | Purpose |
| --- | --- |
| `doc.add_stylesheet(css)` | Add an application stylesheet. Returns `{"sheet": int, "diagnostics": [...]}`; keep `sheet`. |
| `doc.replace_stylesheet(sheet, css)` | Replace that stylesheet's CSS. |
| `doc.remove_stylesheet(sheet)` | Remove it. |
| `doc.stylesheets()` | All stylesheets: `[{"kind", "sheet", "node", "element", "href", "media", "length"}, ...]`. |
| `doc.computed_style(properties, element=None)` | `{"schema": 1, "properties": {name: {"status", "value", ...}}}`; `status` is `"supported"`, `"unsupported"` or `"invalid"`. |
| `doc.environment()` | Effective `color_scheme`/`reduced_motion`, where each value came from, scale and high contrast. |
| `doc.set_environment(*, color_scheme=None, reduced_motion=None)` | Override for this document: `color_scheme` `"light"`, `"dark"` or `"system"`; `reduced_motion` `"reduce"`, `"no-preference"` or `"system"`. `"system"` removes the override. |

The `prefers-color-scheme` and `prefers-reduced-motion` media queries follow the effective environment. See [HTML/CSS scope](../concepts/html-css-scope.md) for supported CSS.

### Scrolling, dialogs and popovers

| Call | Purpose |
| --- | --- |
| `doc.scroll(element=None)` | Scroll state `{"x", "y", "min_x", "min_y", "max_x", "max_y", "width", "height", "scrollable", "animating"}` of an element or the viewport. |
| `doc.scroll_to(*, x=None, y=None, element=None, behavior="auto")` | Scroll to CSS-pixel coordinates; `behavior` is `"auto"`, `"instant"` or `"smooth"`. |
| `doc.scroll_into_view(element, *, block="nearest", inline="nearest")` | Bring an element into view; alignment `"start"`, `"center"`, `"end"` or `"nearest"`. |
| `doc.show_dialog(element, *, modal=True)` / `doc.close_dialog(element, return_value="")` | Open/close an HTML `<dialog>` in the page. |
| `doc.show_popover(element)` / `doc.hide_popover(element)` / `doc.toggle_popover(element)` | Control an element with the `popover` attribute. `toggle_popover` returns the new open state. |

HTML `<dialog>` elements are part of the page. Native operating-system dialogs are the `Window` file pickers.

### Resource bundles and live bitmaps

| Call | Purpose |
| --- | --- |
| `doc.register_resources(name, entries)` | Atomically register or replace a bundle of in-memory files, addressed as `ui4me-res://name/path`. Each value is `bytes` or `(bytes, "mime/type")`. |
| `doc.unregister_resources(name)` | Remove the bundle. |
| `doc.resource_stats()` | Cache and bundle counters. |
| `doc.purge_unused_resources(url=None)` | Release decoded images not currently in use. |
| `doc.create_bitmap(bitmap_id, width, height, *, slots=2)` | Create a [`LiveBitmap`](#livebitmap) for `<img src="ui4me-bitmap://bitmap_id">`. |

Resource paths are relative keys with `/` separators (no `..`, no absolute paths). One registration may hold at most 256 entries and 1 MiB of data; a document may hold up to 16 MiB in 64 bundles. A rejected registration leaves the previous bundle working.

### Node lookup

| Call | Purpose |
| --- | --- |
| `doc.node(element=None, *, reference=None)` | A `Node` for an element ID, the document node (no arguments), or an existing reference string. |
| `doc.query_nodes(selector, *, offset=0, limit=1024)` | `{"nodes": [{"node", "element"}, ...], "total", "offset"}`; includes elements without an `id`. |
| `doc.node_batch(operations)` | Atomic batch addressed by node reference (see [Node](#node)). |

## Events

```text
unsubscribe = doc.on(event_type, callback, *, element=None)
```

`callback(event)` receives a dictionary. With `element="save"` the callback runs for events whose target is `save` **or** that bubble through `save` (delegation), so one handler on a list container sees clicks on its items. `doc.on` returns a function; call it to unsubscribe. A document allows at most 256 subscriptions.

Common event types: `click`, `dblclick`, `auxclick`, `contextmenu`, `input`, `change`, `focus`, `blur`, `focusin`, `focusout`, `keydown`, `keyup`, pointer events (`pointerdown`, `pointerup`, `pointermove`, ...), `wheel`, `scroll`, `submit`, `reset`, `invalid`, `close` (HTML dialogs), popover toggles, composition events, CSS transition/animation events, `drop`, and the UI4ME events `link_activated` and `environment_changed`. The [HTML/CSS scope](../concepts/html-css-scope.md#events-delivered-to-the-backend) page lists them all. Text editing emits one `input` per edit and one `change` when the value is committed (for example when the field loses focus).

Every event has:

| Field | Meaning |
| --- | --- |
| `type` | Event type. |
| `element` | ID of the nearest element with an `id`, starting from the target (`""` if none). |
| `path` | IDs of identified elements the event passes through, innermost first. |
| `node` | Opaque node reference of the actual target. |
| `shift`, `ctrl`, `alt`, `meta` | Modifier keys. |
| `cancelable` | Whether the event has a default action. |
| `window_id`, `document_id` | Added by the Python client. |

Depending on the type and target: `value`, `checked`, `selection`, `files` (controls); `key`, `code`, `repeat`, `is_composing` (keyboard); `x`, `y`, `button`, `buttons`, `pointer_id`, `pointer_type`, `pressure` (pointer, viewport CSS pixels); `delta_x`, `delta_y` (wheel); `data`, `input_type` (input); `submitter` (submit); `validity` (invalid); `return_value` (dialog close); `default_prevented`.

**Events arrive after the native action.** By the time a `click` or `keydown` callback runs, the host has already performed the default action (the character is typed, the form is submitted, the link is followed). A callback cannot cancel it. To suppress a default action, register a rule in advance:

```text
doc.prevent_default(element, event, *, prevent=True, key=None, shift=None, ctrl=None, alt=None, meta=None)
```

`key` and the modifier flags narrow the rule (for example `doc.prevent_default("editor", "keydown", key="s", ctrl=True)`); `prevent=False` removes it. Only default actions the engine supports can be prevented (such as keyboard shortcuts, `submit`, `contextmenu` and dialog `cancel`). A document holds at most 256 rules.

**Errors.** `doc.on("error", callback)` receives asynchronous failures for this document (for example a failed clipboard transfer) as `{"type": "error", "message", "fatal"}`. The message is also printed to stderr. `fatal=True` means the host unloaded the document after a rendering failure; the window stays open, and you may load a replacement from the callback.

**Links never navigate.** Activating an `<a href>` sends a `link_activated` event with `href` and the resolved `url`; decide in Python what to do, for example `app.open_external(event["url"])` for a web link or `window.load_file(...)` for another local page.

**Drag and drop from other applications.** Mark a receiving element with `data-ui4me-drop="files"`, `"text"` or `"files text"`; `"none"` blocks a subtree. A completed drop arrives as a `drop` event with `x`, `y`, `external: True` and either `paths` (a list of local file paths) or `text`.

## `Node`

A `Node` is an opaque reference to one node in one document generation, including elements without an `id` and text nodes. Get one from `doc.node(...)`, `doc.query_nodes(...)` (wrap the `node` string with `doc.node(reference=...)`), or an event's `node` field. Never build reference strings yourself.

| Call | Purpose |
| --- | --- |
| `node.id`, `node.document` | Reference string and owning document. |
| `node.inspect()` | Type, name, `id`, attributes or text, parent and child count. |
| `node.children(*, offset=0, limit=256)` | `{"children": [{"node", "type", "name", "id"}, ...], "total"}`; `limit` 1–4096. |
| `node.set_text(text)` / `node.append_text(text)` | Replace / append text. |
| `node.set_attribute(name, value)` / `node.remove_attribute(name)` | Attributes. |
| `node.add_class(name)` / `node.remove_class(name)` | Classes. |
| `node.insert_html(html, *, position="beforeend")` / `node.replace_html(html)` | Trusted markup. |
| `node.trim_children(keep, *, from_start=True)` | Keep `keep` children; remove the oldest (`from_start=True`) or the newest. |
| `node.move_to(target, *, position="beforeend")` | Move this node relative to another `Node` of the same document. |
| `node.remove()` | Remove the node. |
| `node.focus()` / `node.blur()` | Focus. |

`doc.node_batch(operations)` is the atomic batch for nodes: the same operations as `doc.batch`, with `node` instead of `element` and `target_node` instead of `target`. A node keeps its identity when moved but not after removal.

## `LiveBitmap`

For pixels that change continuously (previews, plots, video-like output), use a live bitmap instead of re-encoding images. The pixels go through a shared memory mapping, not through JSON.

```html
<img src="ui4me-bitmap://preview" width="320" height="200" alt="Preview">
```

```python
bitmap = document.create_bitmap("preview", 320, 200, slots=2)
frame = bytes([30, 120, 220, 255]) * (320 * 200)
bitmap.present(frame)
```

| Member | Purpose |
| --- | --- |
| `bitmap.present(pixels)` | Copy one complete frame and return after the host has taken it. |
| `bitmap.try_present(pixels)` | Same, but returns `None` immediately if another `present` is still in progress (drop that frame). |
| `bitmap.close()` | Release the bitmap. Also used by `with document.create_bitmap(...) as bitmap:`. |
| `bitmap.id`, `bitmap.width`, `bitmap.height`, `bitmap.stride`, `bitmap.slots`, `bitmap.slot_bytes` | Geometry. |

A frame is straight-alpha RGBA8, row by row, exactly `width * height * 4` bytes (`bytes`, `bytearray` or `memoryview`); do not modify the buffer during the call. `bitmap_id` is 1–64 ASCII letters, digits, `_` or `-`; each side is 1–4096 pixels; `slots` is 2 or 3. A document can have 8 bitmaps using up to 64 MiB together. Frames of the same size update the image without re-running layout. A bitmap belongs to its document: replacing or unloading the document closes it.

## Errors

| Exception | Raised when |
| --- | --- |
| `StartupError` (`RuntimeError`) | `Application.start()` cannot run: interactive interpreter, `python -c` or stdin; host binary missing; only one of `UI4ME_ENDPOINT`/`UI4ME_TOKEN` set; host failed to launch. Also `from_environment()` without a host connection. |
| `StaleDocumentError` (`RuntimeError`) | A retired `Document`, `Node` or closed `Window` was used. |
| `RpcError` | The host rejected a request. Has `code`, `message`, `data`. |
| `DisconnectedError` (`ConnectionError`) | The connection to the host is gone. |
| `ProtocolError` | The host sent a malformed message. |
| `ValueError` / `TypeError` | Invalid arguments detected locally. |

Common `RpcError.code` values: `-32602` invalid parameters, `-32001` unsupported on this platform, `-32002` unavailable right now, `-32003` denied, `-32004` closed or stale, `-32005` busy or a limit reached, `-32006` cancelled, `-32008` invalid state (for example shutting down). Platform-dependent features such as Wayland window positioning fail with an explicit error instead of pretending to work, so catch `RpcError` around them.

An exception in an event callback is printed (`Notification handler failed: ...`) and does not stop the application. An exception in a close or quit policy callback counts as a veto: the window stays open or the application keeps running.

## Threading and callbacks

- **Event callbacks run one at a time, in order,** on a single event thread of your backend (not the host's UI thread). A callback may call any document method and wait for the reply.
- A long-running callback delays later events. Do slow work in a `threading.Thread` and send small updates back with document methods.
- **Close and quit policy callbacks** (`on_before_close`, `on_before_quit`) and the shutdown request run on separate worker threads, so they work even while an event callback is blocked, for example waiting on a file dialog. They may run concurrently with an event callback: protect shared state with a lock and keep the decision quick.
- Document and window methods are thread-safe; you can call them from your own threads.
- An event is a snapshot. If the page changed after the event was queued, its `element` ID may no longer exist; do not treat it as proof that the element is still there.

### Low-level client

`app.rpc` is an `RpcClient`. Useful members: `on_notification(method, callback)` (one callback per method; do not replace `document.event`, `document.error` or `window.event`, which the `Application` uses), `on_request(method, callback)`, `call(method, params=None, *, timeout=...)`, `call_async(method, params=None)`, `notify(method, params=None)`, `closed`, `shutdown_requested` (a `threading.Event`), and `dropped_notifications` (incremented if your backend falls so far behind that the bounded queue overflows). Default timeout is 15 seconds, none for dialogs. The wire protocol is described in [protocol](protocol.md); normal applications do not need it.

## Limits

| Limit | Value |
| --- | --- |
| One IPC message | 2 MiB after JSON encoding |
| HTML source per load | 1 MiB |
| Text per `set_text`/`set_value`/`append_text` | 16 KiB |
| Nodes / nesting depth | 65,536 / 256 |
| Stylesheets added at runtime | 64, each up to 1 MiB |
| Default-action rules | 256 per document |
| Live documents per host | 16 |
| Batch / node batch | 1024 operations |
| `query`, `query_nodes`, `node.children` page | 1–4096 entries |
| Subscriptions per document (Python) | 256 |
| Outstanding requests (Python) | 128 |
| Resource registration | 256 entries, 1 MiB per call; 16 MiB, 64 bundles per document |
| Live bitmaps | 8 per document, 64 MiB total, 4096 px per side |
| File dialog result | 1024 paths |

A result that would exceed the message limit is an error, never silently truncated; use paging (`offset`/`limit`) instead.

## Command-line tools in the package

| Command | Purpose |
| --- | --- |
| `ui4me-new DIR [--name NAME] [--python PATH]` | Create a starter project: `app.py` (calls `Application.start(name=APP_NAME, app_id=APP_ID)`), `index.html`, `style.css`, `pyproject.toml` (depends on `ui4me-core>=1.0.1`, so `uv run app.py` works in the folder) and an optional `app.toml` manifest with the same app ID that pins the interpreter (`--python`, default: the current one). The directory must be new or empty. |
| `ui4me-host run --app app.toml` | Run the native host with an explicit manifest. |
| `ui4me-host doctor` | Check the installed runtime and desktop session. |
| `ui4me-host --version` | Host version. |
| `ui4me-skill install\|status\|uninstall [--agents all\|claude,codex,...] [--scope user\|project] [--project PATH] [--dry-run] [--force]` | Install the bundled coding-agent skill. See [agent skills](../guides/agent-skills.md). |

`ui4me_core.host_binary()` returns the path of the native host that `start()` and `ui4me-host` use (`UI4ME_HOST_BINARY` if set, otherwise the packaged binary, otherwise `ui4me-host` on `PATH`).

Source: [application.py](https://github.com/ui4me/ui4me-core/blob/main/python/ui4me_core/application.py), [\_bootstrap.py](https://github.com/ui4me/ui4me-core/blob/main/python/ui4me_core/_bootstrap.py), [client.py](https://github.com/ui4me/ui4me-core/blob/main/python/ui4me_core/client.py), [live_bitmap.py](https://github.com/ui4me/ui4me-core/blob/main/python/ui4me_core/live_bitmap.py).
