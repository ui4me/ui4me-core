# UI4ME Python API (compact reference)

`from ui4me_core import Application, Window, Document, Node, LiveBitmap, StaleDocumentError, StartupError, RpcError, DisconnectedError, ProtocolError, host_binary`

Element arguments named `element` are HTML `id` values. Arguments marked *T* accept an id or a `Node`.

## Application

| Call | Notes |
| --- | --- |
| `Application.start(*, name=None, app_id=None, version=None, icon=None, instance="single", startup_timeout_ms=30000, shutdown_timeout_ms=5000, max_windows=16)` | Normal entry point. Launcher role: runs host, never returns (`SystemExit`). Backend role: returns the connected app. Use as `with ... as app:`. |
| `Application.from_environment()` | Connect only (manifest-launched backend). `StartupError` without `UI4ME_ENDPOINT`/`UI4ME_TOKEN`. |
| `app.create_window(*, title="UI4ME", width=900, height=600, visible=True, decorations=True, min_width=None, min_height=None, max_width=None, max_height=None)` | Empty window. |
| `app.open_file(path, **window_options)` | Window + local HTML file. |
| `app.open_html(html, *, asset_root=None, **window_options)` | Window + HTML string; `asset_root` folder for relative assets. |
| `app.run()` | Announce ready (needs ≥1 window) and block until shutdown. |
| `app.ready()` | Announce ready only. |
| `app.request_quit()` | Quit request (policy may veto). |
| `app.on_before_quit(callback)` | `callback() -> bool`. |
| `app.clipboard_text()` / `app.set_clipboard_text(text)` | Plain-text clipboard. |
| `app.capabilities()` | Implemented/available host features. |
| `app.desktop_preferences()` / `app.on_desktop_preferences(callback)` | `color_scheme`, `reduced_motion`, `high_contrast` (may be `None`), `available`, `source`, `revision`. |
| `app.open_external(target, *, window=None)` / `app.reveal_file(path, *, window=None)` | Desktop open / show in file manager. |
| `app.close()` | Disconnect (done by `with`). |
| `app.rpc.on_notification("app.activated", callback)` | A second launch of a single-instance app. |

Closing the last window asks to quit.

## Window

| Call | Notes |
| --- | --- |
| `window.document`, `window.id`, `window.closed` | Current document (or `None`). |
| `window.load_file(path, *, asset_root=None)` / `window.load_html(html, *, asset_root=None)` | Replace the page; returns the **new** `Document`. |
| `window.set(**props)` | `title, width, height, visible, resizable, decorations, minimized, maximized, fullscreen, focus, x, y, min_width, min_height, max_width, max_height`. |
| `window.state()` | `{"requested": {...}, "observed": {...}}`; trust `observed`. |
| `window.request_attention(kind="informational")` | `"critical"`, or `None` to cancel. |
| `window.request_close()` / `window.on_before_close(callback)` | Per-window close policy, `callback() -> bool`. |
| `window.open_files(*, multiple=False, title=None, directory=None, filters=(), wait=True)` | → `{"cancelled", "paths"}`. |
| `window.save_file(*, file_name=None, title=None, directory=None, filters=(), wait=True)` | → `{"cancelled", "paths"}`. |
| `window.choose_folder(*, title=None, directory=None, wait=True)` | → `{"cancelled", "paths"}`. |

Filters: `[{"name": "Images", "extensions": ["png", "jpg"]}]` (no dots). `wait=False` returns a `concurrent.futures.Future`; `future.cancel()` closes the dialog. `wait=True` has no timeout.

Frameless: `decorations=False` plus `data-ui4me-drag` (title bar), `data-ui4me-resize="n|s|e|w|ne|nw|se|sw"`, `data-ui4me-no-drag`.

## Document

| Area | Calls |
| --- | --- |
| Events | `on(event_type, callback, *, element=None) -> unsubscribe`, `prevent_default(element, event, *, prevent=True, key=None, shift=None, ctrl=None, alt=None, meta=None)` |
| Text/attrs | `get_text/set_text(element[, text])`, `set_attribute(element, name, value)`, `remove_attribute(element, name)`, `add_class/remove_class(element, name)` |
| Markup | `insert_html(element, html, *, position="beforeend")` (`beforebegin`, `afterbegin`, `beforeend`, `afterend`), `replace_html(element, html)`, `remove(element)`, `batch(ops)` |
| Query | `query(selector, *, offset=0, limit=1024)` → ids only; `query_nodes(selector, ...)` → `{"nodes": [{"node", "element"}], "total", "offset"}`; `node(element=None, *, reference=None)`; `node_batch(ops)` |
| Forms | `get_value/set_value`, `get_checked/set_checked`, `options(element)`, `set_selected_values(element, values)`, `files(element)`, `form_data(form, *, submitter=None)`, `request_submit(form, *, submitter=None)`, `reset_form(form)`, `validity(element)`, `check_validity(element)`, `set_custom_validity(T, message="")` |
| Focus/editing | `focus(T)`, `blur(T=None)`, `focused_element()`, `element_at_point(x, y)`, `control_selection(T)`, `set_control_selection(T, start, end, *, direction="none")`, `replace_control_range(T, replacement, *, start=None, end=None, selection_mode="preserve")`, `edit(command, T=None)` with `undo/redo/cut/copy/paste/select_all/delete` |
| Styles | `add_stylesheet(css)` → `{"sheet": int, ...}`, `replace_stylesheet(sheet, css)`, `remove_stylesheet(sheet)`, `stylesheets()`, `computed_style(properties, T=None)`, `environment()`, `set_environment(*, color_scheme=None, reduced_motion=None)` (`"system"` clears) |
| Scroll/overlays | `scroll(element=None)`, `scroll_to(*, x=None, y=None, element=None, behavior="auto")`, `scroll_into_view(element, *, block="nearest", inline="nearest")`, `show_dialog(element, *, modal=True)`, `close_dialog(element, return_value="")`, `show_popover/hide_popover/toggle_popover(element)` |
| Resources | `register_resources(name, {path: bytes or (bytes, mime)})` → `ui4me-res://name/path`; `unregister_resources(name)`, `resource_stats()`, `purge_unused_resources(url=None)` |
| Bitmaps | `create_bitmap(bitmap_id, width, height, *, slots=2)` → `LiveBitmap` for `<img src="ui4me-bitmap://bitmap_id">` |
| Other | `unload()`, `diagnostics()`, `command(op, **fields)` (typed host operation, not a DOM API) |

**Batch ops** (`batch`, max 1024, atomic): `set_text`/`append_text` (`element`, `text`), `set_attribute` (`element`, `name`, `value`), `remove_attribute` (`element`, `name`), `add_class`/`remove_class` (`element`, `name`), `insert_html` (`element`, `position`, `html`), `replace_children_html` (`element`, `html`), `remove_element` (`element`), `move_element` (`element`, `target`, `position`), `trim_children` (`element`, `keep`, `from`="start"|"end"). `node_batch` uses `node` instead of `element` and `target_node` instead of `target`. Not batchable: values, checked state, focus, stylesheets, dialogs.

**UTF-16:** selection offsets count UTF-16 code units. `"A😀B"`: emoji is 1–3; offset 2 is rejected. Convert: `len(s[:i].encode("utf-16-le")) // 2`.

## Events

`callback(event)` gets a dict: `type`, `element` (nearest identified target id or `""`), `path` (identified ancestors, innermost first), `node`, `shift`/`ctrl`/`alt`/`meta`, `cancelable`, `window_id`, `document_id`; plus by type `value`, `checked`, `selection`, `files`, `key`, `code`, `repeat`, `x`, `y`, `button`, `buttons`, `pointer_type`, `delta_x`, `delta_y`, `data`, `input_type`, `submitter`, `validity`, `return_value`, `default_prevented`.

Types include `click`, `dblclick`, `auxclick`, `contextmenu`, `input` (each edit), `change` (on commit), `focus`, `blur`, `focusin`, `focusout`, `keydown`, `keyup`, pointer events, `wheel`, `scroll`, `submit`, `reset`, `invalid`, `close`, composition, transition/animation events, `drop` (on `data-ui4me-drop="files"|"text"` elements; `paths` or `text`), `link_activated` (`href`, `url`; links never navigate), `environment_changed`, and `error` (`message`, `fatal`).

Events are delivered after the native default action. Only rules registered in advance with `prevent_default` suppress it.

## Node

`inspect()`, `children(*, offset=0, limit=256)`, `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html(html, *, position="beforeend")`, `replace_html`, `trim_children(keep, *, from_start=True)`, `move_to(target_node, *, position="beforeend")`, `remove()`, `focus()`, `blur()`. References are opaque and bound to one document generation; never construct them.

## LiveBitmap

`present(pixels)` (waits until the host took the frame), `try_present(pixels)` (returns `None` if busy), `close()`, context manager. Frame = straight-alpha RGBA8, exactly `width*height*4` bytes. Limits: id 1–64 of `[A-Za-z0-9_-]`, 1–4096 px per side, `slots` 2 or 3, 8 bitmaps / 64 MiB per document. Produce frames on a thread; stop it before `close()`. Closed automatically when its document is replaced.

## Errors and threading

- `StartupError`: REPL/`-c`/stdin, missing host binary, half-set `UI4ME_ENDPOINT`/`UI4ME_TOKEN`, launch failure, `from_environment()` without a host.
- `StaleDocumentError`: retired document/node or closed window.
- `RpcError(code, message, data)`: -32602 invalid params, -32001 unsupported, -32002 unavailable, -32004 closed/stale, -32005 busy/limit, -32008 invalid state.
- `DisconnectedError`: host gone (`app.run()` raises it on an unexpected disconnect).
- Event callbacks: one ordered background thread; an exception is printed and ignored. Policy callbacks: separate threads, may overlap event callbacks; an exception counts as a veto.

## Limits

Loading fails (`RpcError` -32602) for `<script>`, `<iframe>`, `on…=` attributes, `contenteditable`, color/date inputs and duplicate ids.

2 MiB per IPC message, 1 MiB HTML per load, 16 KiB text per `set_text`/`set_value`, 256 default-action rules, 16 documents, 1024 batch ops, 4096 entries per query page, 256 subscriptions per document, 128 outstanding requests, 256 entries / 1 MiB per resource registration (16 MiB per document).

Full reference: https://github.com/ui4me/ui4me-core/blob/main/docs/reference/python-api.md
