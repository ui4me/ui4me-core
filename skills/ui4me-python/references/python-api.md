# UI4ME Python API cookbook

Use `from ui4me_core import Application` from a backend launched by `ui4me-host run --app app.toml`.

## Windows and documents

```python
with Application.from_environment() as app:
    win = app.open_file(Path(__file__).parent / "ui/index.html", title="My app")
    other = app.open_html('<p id="info">Hello</p>', title="Generated window")
    win.load_html('<p id="info">Replacement</p>', asset_root=Path(__file__).parent)
    current = win.document  # old document and node objects are now stale
    current.set_text('info', 'Hello again')
    app.run()
```

`Application.create_window(title=...,width=...,height=...,visible=...,decorations=...,min_width=...,max_width=...)` creates an empty window. `window.load_file(path)` and `window.load_html(source,asset_root=...)` set/replace the document. `window.set(**properties)`, `window.state()`, `window.request_attention()`, and `window.request_close()` expose native operations. `app.request_quit()` asks to quit the application.

## Document operations

- Read/write: `get_text(id)`, `set_text(id,text)`, `get_value(id)`, `set_value(id,value)`, `get_checked(id)`, `set_checked(id,bool)`.
- HTML: `insert_html(id,fragment,position="beforeend")`, `replace_html(id,fragment)`, `remove(id)`.
- CSS/class: `add_class(id,name)`, `remove_class(id,name)`, `set_attribute(id,name,value)`, `remove_attribute(id,name)`, `add_stylesheet(css)`, `stylesheets()`, `replace_stylesheet(sheet_id,css)`, `remove_stylesheet(sheet_id)`, `computed_style(properties,id)`.
- Layout/navigation: `scroll_into_view(id)`, `scroll_to(x=...,y=...)`, `element_at_point(x,y)`, `focused_element()`, `focus(id)`, `blur(id)`.
- Forms/selection: `form_data(form_id)`, `request_submit(form_id)`, `reset_form(form_id)`, `set_custom_validity(id,message)`, `validity(id)`, `control_selection(id)`, `set_control_selection(id,start,end)`, `replace_control_range(id,replacement,start=...,end=...)`, `edit(command,id)`.
- Anonymous nodes: `doc.query_nodes('ul > li')` then `doc.node(reference=...)`; opaque references are bound to the document generation.
- `doc.batch([{"op":"set_text","element":"...","text":"..."},...])` applies a validated atomic group. Allowed ops: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html` (what `replace_html()` sends), `remove_element`, `move_element` and `trim_children`; fields match the single commands (`element`, `text`, `name`, `value`, `html`, `position`, `target`, `keep`, `from_start`). `set_value`/`set_checked` are not batchable.
- Diagnostics: `doc.diagnostics()`, `app.capabilities()`, `doc.resource_stats()`.

## Events and action cancellation

```python
unsub = doc.on('click', lambda ev: print(ev['element']), element='save')
doc.on('contextmenu', lambda ev: print('Right click', ev))
doc.on('auxclick', lambda ev: print('Other button', ev.get('button')))
# To cancel a supported action, register BEFORE native input:
doc.prevent_default('save', 'click')
# Later:
unsub()
```

Document and window events are delivered to the Python backend asynchronously. Event handlers may issue nested document RPCs. Register close/quit approval callbacks using `win.on_before_close(lambda: bool(...))` or `app.on_before_quit(lambda: bool(...))`. Return `False` to veto.

## Native desktop services

```python
paths = win.open_files(multiple=True)  # native picker; may be cancelled
save = win.save_file(file_name='result.txt')
folder = win.choose_folder()
app.set_clipboard_text('Hello')
print(app.clipboard_text())
app.open_external('https://example.org', window=win)
app.reveal_file(Path('/path/to/file.txt'), window=win)
```

`window.open_files(wait=False)` returns a transport future; calling `future.cancel()` asks the host to cancel the native dialog. The host may reject or report unavailable native capabilities. Do not treat a request as confirmed focus/window movement on Wayland.

## Live RGBA preview

```python
bitmap = doc.create_bitmap('preview', 320, 200, slots=2)
try:
    rgba = bytes([0, 90, 200, 255]) * (320 * 200)
    bitmap.try_present(rgba)  # None when every slot is occupied
finally:
    bitmap.close()
```

HTML contains `<img src="ui4me-bitmap://preview" width="320" height="200">`. Layout is unchanged for updates with the same dimensions. Respect the pixel and slot budgets; use the producer on a background thread if frames are continuously generated.

## Resource bundles

`doc.register_resources('icons', {'ok.png': png_bytes})` exposes a bounded `ui4me-res://icons/ok.png` resource. Same-size replacement can repaint without style/layout. Call `unregister_resources('icons')` or unload the document to release ownership.
