# UI4ME application patterns

All examples run inside an application backend launched by the native host (`ui4me-host run --app app.toml`). There is no HTML JavaScript interpreter.

## Load a file and update it incrementally

```python
from pathlib import Path
from ui4me_core import Application

with Application.from_environment() as app:
    win = app.open_file(Path(__file__).parent/'ui/index.html', title='Tasks')
    doc = win.document

    def add_task(_event):
        name = doc.get_value('task-name') or 'Untitled'
        from html import escape
        doc.batch([
            {'op':'insert_html', 'element':'tasks', 'position':'beforeend',
             'html':f'<li>{escape(name)}</li>'},
            {'op':'set_text', 'element':'status', 'text':'Task added'},
        ])
        doc.set_value('task-name', '')  # form values are not batch operations

    doc.on('click', add_task, element='add-task')
    app.run()
```

Use `doc.set_text()` for plain user input. Only construct HTML fragments with correctly escaped dynamic content. For large append-only views, use batches and trim old rows instead of reloading the whole document for each change.

## Generate the entire page in code

```python
html = '<main><h1>Generated UI</h1><button id="refresh">Refresh</button><p id="result">Waiting</p></main>'
win = app.open_html(html, title='Generated', asset_root=Path(__file__).parent)
doc = win.document
doc.on('click', lambda _: doc.set_text('result','Updated'), element='refresh')
```

`asset_root` lets relative CSS/fonts/images resolve when the HTML is a Python string. Replacing the whole document via `win.load_html(...)` returns a **new** `Document`: capture and register callbacks on that new object.

## Styles and theme switching

```python
doc.add_class('app', 'dark')
doc.remove_class('app', 'dark')
sheet = doc.add_stylesheet('body { background: #0d1727; color: white; }')
doc.replace_stylesheet(sheet['sheet'], 'body { background: white; color: black; }')
```

The `add_stylesheet` return structure and allowed properties are documented in [Core protocol](../CORE_1.0_PROTOCOL.md). Runtime styles update through the engine; they do not require page reload or JavaScript.

## Native open/save and clipboard

```python
files = win.open_files(multiple=False)
if files.get('cancelled') is False:
    # The response has a list of selected local paths.
    paths = files.get('paths', [])
    doc.set_text('selected', Path(paths[0]).name if paths else 'None')

app.set_clipboard_text('Copied from UI4ME')
print(app.clipboard_text())
```

File choosers are native and window-parented where supported. For a nonblocking picker use `wait=False`; its future supports cancellation. Some desktop portals are responsible for their own parenting and cancellation limitations.

## Continuous visualizations

```python
from threading import Event, Thread
stop = Event()
bitmap = doc.create_bitmap('preview', 320, 200, slots=2)

def produce():
    while not stop.wait(1 / 24):
        rgba = bytes([20, 80, 160, 255]) * (320 * 200)
        try:
            bitmap.try_present(rgba)
        except Exception:
            break

thread = Thread(target=produce, daemon=True)
thread.start()
# On shutdown:
stop.set()
thread.join(timeout=2)
bitmap.close()
```

HTML: `<img src="ui4me-bitmap://preview" width="320" height="200">`. This uses bounded shared mappings; raw pixels do **not** pass through JSON-RPC. See the full example for lifecycle and frame generation.

## Frameless window controls

Create with `decorations=False`. Use HTML/CSS to style the titlebar and `data-ui4me-drag` on the noninteractive drag area. The native runtime begins moving the window during the real pointer gesture. Add buttons for close/minimize/restore through Python. Native resize regions use `data-ui4me-resize` with direction values.

## Application distribution

Use `ui4me-new` for a clean app and a virtual environment. The manifest must name the Python interpreter with `ui4me_core` installed, not an arbitrary global interpreter. For menu shortcuts, see the source repository tool `tools/create_launcher.py` and [platform guide](platforms.md). UI4ME's wheel includes the host executable; it does not bundle an entire application or Python itself.
