# Python API reference — UI4ME Core 1.0.0

UI4ME's Python client lives in `ui4me_core` and communicates with a **separate native Rust host** using authenticated local IPC. Python 3.10+; no CPython extension is required. Start your backend through `ui4me-host run --app app.toml`, not as a standalone script.

## The first three objects

| Object | Purpose | How you get it |
| --- | --- | --- |
| `Application` | Backend session, native services, window collection and lifecycle | `Application.from_environment()` inside a host-launched process |
| `Window` | Native window; load and replace its document; set title/state; use native dialogs | `app.create_window(...)`, `app.open_file(...)`, `app.open_html(...)` |
| `Document` | Current HTML/CSS DOM, events, forms, styles, resources and live bitmap surfaces | `window.document`, `window.load_file(...)`, `window.load_html(...)` |

```python
from pathlib import Path
from ui4me_core import Application

with Application.from_environment() as app:
    window = app.open_file(Path(__file__).parent / 'ui/index.html', title='Example', width=960, height=640)
    doc = window.document
    doc.on('click', lambda event: doc.set_text('message', 'Saved'), element='save')
    app.run()
```

### Application

| Python method | Purpose |
| --- | --- |
| `Application.from_environment()` | Connects to host-provided token and endpoint; **only** inside a managed backend. |
| `create_window(title=...,width=...,height=...,decorations=...,min_width=...,max_width=...)` | Create native window (empty until document loaded). |
| `open_file(path, **window_options)` | Create window and load local HTML file. Relative CSS/images resolve beside HTML. |
| `open_html(html,asset_root=None,**window_options)` | Create window and parse supplied HTML. Supply `asset_root` for local fonts/images. |
| `ready()` / `run()` | Mark backend ready / wait for host-initiated shutdown. |
| `request_quit()` / `on_before_quit(callback)` | Request quit / approve or veto application quit. |
| `clipboard_text()` / `set_clipboard_text(value)` | Read/write native text clipboard. |
| `open_external(target,window=None)` / `reveal_file(path,window=None)` | Ask OS to open a non-executable target or reveal file. |
| `desktop_preferences()` / `on_desktop_preferences(callback)` | Read/observe native appearance and motion preferences. |
| `capabilities()` | Supported/available native capabilities; handle unsupported errors honestly. |

### Window

| Method | Purpose |
| --- | --- |
| `load_file(path,asset_root=None)` | Replace with a local HTML file and return its new `Document`. |
| `load_html(html,asset_root=None)` | Replace with generated HTML string, return its new `Document`. |
| `set(**properties)` / `state()` | Update requested window state / read requested and observed state. |
| `request_attention(kind='informational')` | Ask window manager to draw attention where supported. |
| `request_close()` / `on_before_close(callback)` | Close or asynchronously approve/veto close. |
| `open_files(multiple=False,title=None,directory=None,filters=(),wait=True)` | Native file chooser. |
| `save_file(file_name=None,title=None,directory=None,filters=(),wait=True)` | Native save chooser. |
| `choose_folder(title=None,directory=None,wait=True)` | Native folder chooser. |

`wait=False` on a dialog returns a transport future; `future.cancel()` requests cancellation of the native dialog. Do not assume every platform reports the cancellation of an external app chooser.

**Document identity:** after a successful `load_html`, `load_file` or `unload`, the previous `Document` and its `Node` references are stale. Do not reuse them or their event subscriptions. A failed load leaves the previous document intact.

### Document: common operations

| Method | Example | Notes |
| --- | --- | --- |
| `get_text` / `set_text` | `doc.set_text('status','Saved')` | Text, escaped for you; good for untrusted user data. |
| `get_value` / `set_value` | `doc.get_value('name')` | Form-control values. |
| `get_checked` / `set_checked` | `doc.set_checked('notify', True)` | Checkboxes/radios. |
| `add_class` / `remove_class` | `doc.add_class('panel','selected')` | CSS class update. |
| `set_attribute` / `remove_attribute` | `doc.set_attribute('image','src','icon.png')` | DOM attribute update. |
| `insert_html` / `replace_html` / `remove` | `doc.insert_html('items','<li>New</li>')` | Treat HTML strings as trusted markup; escape interpolated user text with `html.escape()`. |
| `batch(operations)` | `doc.batch([{'op':'set_text',...},...])` | One validated atomic DOM batch (max 1024). Allowed ops: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html` (what `replace_html()` sends), `remove_element`, `move_element` and `trim_children`. Form values, focus, stylesheets and dialogs are not batchable. |
| `query(selector,offset=0,limit=1024)` | `doc.query('button.primary')` | Paged selector query. |
| `query_nodes` / `node(reference=...)` | Work with anonymous nodes | Opaque generation-bound handles; do not fabricate references. |
| `focus` / `blur` / `focused_element` | `doc.focus('search')` | Focus/keyboard integration. |
| `scroll_into_view` / `scroll` / `scroll_to` | `doc.scroll_into_view('row-5')` | Scroll operations with documented platform/layout rules. |
| `on(event_type, callback, element=None)` | `doc.on('click', save, element='save')` | Returns an unsubscribe callable. |
| `prevent_default(element,event,prevent=True)` | `doc.prevent_default('save','click')` | Register **before** native input; callback cannot retroactively cancel the action. |
| `diagnostics()` | `doc.diagnostics()` | CSS/layout/asset diagnostics. |

### Events, error handling and close policies

```python
def on_change(event: dict) -> None:
    print('Changed element:', event.get('element'))

unsubscribe = doc.on('change', on_change, element='name')
doc.on('contextmenu', lambda event: print('Right click:', event))
doc.on('auxclick', lambda event: print('Middle/right:', event.get('button')))
doc.on('error', lambda event: print('Error:', event.get('message')))

window.on_before_close(lambda: True)  # False vetoes closing
app.on_before_quit(lambda: True)      # False vetoes quitting
# Later, unsubscribe() removes the subscription.
```

Python callbacks receive **notifications after native event processing**. To suppress a supported native default action, call `prevent_default` **before** the click/keydown. Callback handlers may perform document RPCs, but avoid long blocking operations on the application event worker; use background threads where appropriate and respect shutdown.

### Forms, editing and selection

`doc.form_data('form-id')` returns ordered entries (repeated field names are preserved); `doc.request_submit('form-id')`, `doc.reset_form('form-id')`, `doc.set_custom_validity('field',message)` and `doc.validity('field')` cover form behavior. `doc.options('select')`, `doc.set_selected_values('select',['x'])`, and `doc.files('input')` inspect native controls.

Text-control selection offsets are **UTF-16 units, not Python string indices**. For `A😀B`, the emoji occupies selection 1–3. Methods: `doc.control_selection('field')`, `doc.set_control_selection('field',1,3)`, `doc.replace_control_range(...)`, and `doc.edit('select_all','field')` (consult the typed command documentation for allowed edit command names).

### CSS, local assets and resource bundles

`doc.add_stylesheet(css)` returns a stylesheet identifier; `doc.replace_stylesheet(id,css)`, `doc.remove_stylesheet(id)`, `doc.stylesheets()`, and `doc.computed_style(['color','width'],'item')` are available. Ordinary local `<link rel="stylesheet">`, image and `@font-face` resources are supported within the declared profile.

For generated images/fonts that live in Python rather than on disk, register bounded binary sources:

```python
png_bytes = Path('icon.png').read_bytes()
doc.register_resources('icons', {'status.png': (png_bytes, 'image/png')})
# HTML references: <img src="ui4me-res://icons/status.png">
doc.unregister_resources('icons')
```

Per-call and app budgets apply; see [resource limits](../CORE_1.0_SPEC.md). For diagnostics use `doc.resource_stats()` and `doc.purge_unused_resources()`.

### Continuous pixel updates

```python
with doc.create_bitmap('preview', 320, 200, slots=2) as bitmap:
    frame = bytes([0, 120, 255, 255]) * (320 * 200)  # straight-alpha RGBA8
    bitmap.present(frame)
```

Use `<img src="ui4me-bitmap://preview" width="320" height="200">` in HTML. `bitmap.try_present(frame)` returns `None` when it cannot accept immediately. The frame length must be `width * height * 4` bytes; memory mapping is managed by the library. Each bitmap is document-local and closes automatically when the document is replaced or unloaded. See [live bitmap example](../../examples/python/live_bitmap_demo.py).

### Frameless custom titlebars

`app.create_window(decorations=False)` removes native titlebar decorations. Use `data-ui4me-drag` for a native drag region and `data-ui4me-resize="n|s|e|w|ne|nw|se|sw"` for resize edges. Put actual buttons/text fields in non-draggable elements. Python handles the buttons using `window.set(...)` or close operations. The native host must start drags directly from input; Python callbacks are too late to begin a compositor drag gesture.

## Lower-level commands

`Document.command(op, **fields)` exposes typed document commands without a second DOM state model. Consult [Core 1.0 protocol additions](../CORE_1.0_PROTOCOL.md) and [document protocol](../PROTOCOL_DOCUMENTS.md) for exact op names, input limits, errors, and responses. The private wire connection should normally be accessed through the Python façade rather than constructed manually.
