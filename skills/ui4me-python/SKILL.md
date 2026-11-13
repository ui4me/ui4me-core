---
name: ui4me-python
description: Build native Linux and Windows GUI applications with UI4ME Core using local HTML, CSS and Python. Use for creating windows, modifying the document, handling events and dialogs, styling desktop interfaces, and live bitmap previews. No browser or JavaScript is required.
---

# Build an application with UI4ME Core

**Use this skill when a user wants a Python desktop GUI built from HTML and CSS.** UI4ME is a native Rust host that creates desktop windows, parses HTML/CSS, edits and paints the document, and forwards application events to a separate Python backend. It does **not** run JavaScript, WebKit, Chromium, Qt, HTTP, or a browser. Do not create a web server, HTML JavaScript event handlers or `electron` project.

## Fast path: generate and run a project

```sh
python -m pip install ui4me-core
ui4me-new my-app --name "My App"
ui4me-host run --app my-app/app.toml
```

Windows PowerShell:

```powershell
py -m pip install ui4me-core
ui4me-new my-app --name "My App"
ui4me-host run --app my-app/app.toml
```

Use a **virtual environment** for the project. `ui4me-new` records that interpreter in `app.toml`; do not move the venv without updating `backend.program`. After installation, `ui4me-host --version` and `ui4me-host doctor` are useful diagnostics.

## Read these rules before writing code

1. **Always start through the host:** `ui4me-host run --app app.toml`, not `python app.py`. The native host creates the authenticated backend connection. In backend code, call `Application.from_environment()`.
2. **Load HTML from a file or directly from a string:** `app.open_file(path)` loads linked local CSS/assets; `app.open_html(html, asset_root=directory)` accepts generated HTML. You may also create a window first, then use `window.load_file()` or `window.load_html()` to replace its current document.
3. **Use Python for callbacks:** `doc.on("click", callback, element="button-id")`; use IDs for simple targets, `doc.query_nodes()`/opaque `Node` for anonymous nodes. Avoid `onclick=`, `<script>`, JavaScript, web fetches and browser APIs.
4. **Change the live document instead of reloading:** `doc.set_text`, `doc.set_value`, `doc.add_class`, `doc.replace_html`, `doc.insert_html`, or `doc.batch([...])` for an atomic group. Reload only when replacing the whole page; after reload the **old document and node objects are stale**.
5. **Event callbacks run asynchronously after native default actions.** To suppress an action, register `doc.prevent_default(element, event)` beforehand; calling it inside a callback cannot undo an action already performed.
6. **Native services come from the host:** use `window.open_files`, `window.save_file`, `window.choose_folder`, `app.clipboard_text`, `app.set_clipboard_text`, and `app.open_external`—not HTML browser pickers/JavaScript.
7. **Window lifecycle:** call `app.run()` to announce readiness and wait until shutdown; use `app.request_quit()`, `window.request_close()`, `window.on_before_close()` and `app.on_before_quit()` when appropriate. Clean up streams/bitmaps on unload/close.
8. **Styling:** use ordinary local CSS and the documented supported subset. Complex responsive layouts, CSS variables, fonts, flex/grid/tables, controls and common paint effects are supported, but not every browser feature. Check `doc.diagnostics()` when CSS behaves differently.
9. **Use bounded live bitmaps** for rapidly changing RGBA pixels: `<img src="ui4me-bitmap://preview">`, `doc.create_bitmap("preview", width, height)`, then `bitmap.present(frame)` or `bitmap.try_present(frame)`; close when finished. Do not JSON/base64 encode continuous frames.
10. **Linux and Windows differ in certain native window operations** (e.g., some Wayland focus/position requests). Check `app.capabilities()` and handle unsupported replies instead of assuming a request succeeded.

## Smallest file-based app

`app.py`:

```python
from pathlib import Path
from ui4me_core import Application

HERE = Path(__file__).resolve().parent
with Application.from_environment() as app:
    window = app.open_file(HERE / "index.html", title="My Native App", width=900, height=600)
    doc = window.document
    doc.on("click", lambda event: doc.set_text("message", "Clicked"), element="action")
    app.run()
```

`index.html`:

```html
<!doctype html><html lang="en"><head><meta charset="utf-8">
<link rel="stylesheet" href="style.css"></head><body>
<button id="action">Click me</button><p id="message" role="status">Ready</p>
</body></html>
```

`app.toml` names the application and backend interpreter, for example:

```toml
[app]
id = "org.example.MyNativeApp"
name = "My Native App"
version = "1.0.0"
[backend]
program = "/absolute/path/to/project-venv/bin/python"
args = ["{app_dir}/app.py"]
cwd = "{app_dir}"
[host]
instance = "single"
```

On Windows, `backend.program` is an absolute `...\\Scripts\\python.exe` path (properly escaped in TOML). Generate this manifest with `ui4me-new` instead of typing paths manually when possible.

## Generated HTML in Python (no HTML file)

```python
from ui4me_core import Application
with Application.from_environment() as app:
    window = app.open_html('<button id="go">Run</button><p id="out">Ready</p>', title="Generated UI")
    doc = window.document
    doc.on('click', lambda _: doc.set_text('out', 'Done'), element='go')
    app.run()
```

For images/fonts/local CSS use `asset_root=Path(__file__).resolve().parent`. To replace the entire document use `window.load_html(...)` or `window.load_file(...)` and **re-register callbacks on the new** `window.document`.

## Batch changes, forms and events

```python
doc.batch([
    {"op": "set_text", "element": "status", "text": "Saved"},
    {"op": "add_class", "element": "panel", "name": "active"},
    {"op": "insert_html", "element": "items", "position": "beforeend", "html": "<li>New</li>"},
])
name = doc.get_value('name')
doc.set_value('name', 'Updated')
doc.on('change', lambda event: print(event), element='name')
```

Batch operations are validated/committed as one document transaction (max 1024 entries). **Only these `op` names are accepted:** `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html` (what `replace_html()` sends), `remove_element`, `move_element` and `trim_children`. Form values (`set_value`, `set_checked`), focus, stylesheets and dialogs are *not* batch operations; call their methods separately. Escape untrusted data inserted as HTML with `html.escape()`, or use `set_text()` for user content.

## Further references (read when needed)

- [Python API cookbook](references/python-api.md): windows, DOM, events, forms, dialogs, fonts, resources, bitmaps.
- [Project and packaging](references/projects.md): manifests, venvs, local assets, generators, Windows/Wayland differences, common errors.
- [HTML/CSS support and boundaries](references/css-and-security.md): supported features, security and non-browser expectations.
- Repository documentation: `docs/guides/quickstart.md`, `docs/reference/python-api.md`, `docs/guides/platforms.md`.

If a method is not shown here or in the public docs, inspect the installed `ui4me_core.application` source; do not invent methods. For especially advanced document operations use the typed `Document.command()` only with names documented in `CORE_1.0_PROTOCOL.md` and `PROTOCOL_DOCUMENTS.md`.
