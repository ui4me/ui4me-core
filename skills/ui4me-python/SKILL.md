---
name: ui4me-python
description: Build native Linux and Windows desktop GUI applications in Python with UI4ME Core, using local HTML and CSS for the interface and Python callbacks for behavior (no browser, no JavaScript, no web server). Use when creating or modifying a ui4me_core app - Application.start(), windows, documents, events, forms, batch updates, styling, native file dialogs, clipboard, close/quit policy, live RGBA bitmaps - or when debugging one.
---

# Build a desktop app with UI4ME Core (Python)

UI4ME Core is a native Rust host that owns the windows, parses and paints HTML/CSS, handles input/IME/editing and runs the event loop. The application's Python code runs as a **separate supervised process** connected over authenticated local IPC and changes the page through `ui4me_core` calls. There is **no browser, no JavaScript, no WebKit/Chromium/Qt, no HTTP**. Do not write `<script>`, `onclick=`, `fetch`, a web server, Electron/Flask/React code, or browser DOM APIs. HTML containing `<script>`, `<iframe>`, any `on…=` attribute, `contenteditable`, `<input type="color">`/date/time inputs, or duplicate `id`s **fails to load** (`RpcError` -32602).

## Start a project

```sh
python -m pip install ui4me-core     # in a virtual environment; or: uv add ui4me-core
ui4me-new my-app --name "My App"     # app.py, index.html, style.css, pyproject.toml, app.toml
cd my-app
python app.py                        # or: uv run app.py
```

The minimal program:

```python
from ui4me_core import Application


def main():
    with Application.start(name="My App", app_id="org.example.MyApp") as app:
        window = app.open_html("<h1>Hello UI4ME</h1>")
        app.run()


if __name__ == "__main__":
    main()
```

Run with `python app.py`, `uv run app.py`, `uv run python app.py` or `python -m package.app`. Wheels exist for Linux x86-64 (glibc 2.34+, Wayland desktop) and Windows x86-64; Python 3.10+.

## How `Application.start()` works (read this first)

`python app.py` is the **launcher**: `start()` runs the packaged native host, which starts the **same command again** as the backend with a private connection. In the launcher `start()` never returns (it exits with the host's status). In the backend it connects and returns the `Application`.

- **Code before `Application.start()` runs twice.** Put side effects (file writes, network, printing banners, heavy init) after it, inside `main()`, and always use `if __name__ == "__main__":`.
- It must be a script file or `python -m`. `python -c`, stdin and the REPL raise `ui4me_core.StartupError`.
- `print()` in the backend appears in the terminal. `input()` does not work (no stdin).
- Options: `name`, `app_id` (reverse-DNS; default derived from the script path, so set one for real apps), `version`, `icon`, `instance="single"|"multiple"`, `startup_timeout_ms`, `shutdown_timeout_ms`, `max_windows`. Invalid values raise `ValueError`.
- `app.run()` must come after at least one window exists; it blocks until shutdown. Closing the last window asks to quit (the before-quit policy runs).
- Exit status: 0 normal; nonzero if the backend failed (the host shows an error window); 128+signal on Ctrl+C.

## Core rules

1. **Load UI:** `app.open_file(HERE / "index.html", title=..., width=..., height=...)` (relative CSS/images/fonts resolve from that folder) or `app.open_html(html, asset_root=HERE)`. Use local files only; no network URLs.
2. **Behavior = Python callbacks:** `document.on("click", handler, element="save")`. `element` is an HTML **id**, not a selector, and also matches events bubbling from descendants (use `event["element"]` to see which identified element was hit). Use named `def` functions.
3. **Change the live page, don't reload it:** `set_text`, `get_value`/`set_value`, `get_checked`/`set_checked`, `add_class`/`remove_class`, `set_attribute`, `insert_html`, `replace_html`, `remove`, or `document.batch([...])` for an atomic group.
4. **Untrusted text:** `set_text`/`set_value` are always plain text. `insert_html`/`replace_html` parse HTML: wrap user data with `html.escape()`.
5. **Events arrive after the native action.** A callback cannot cancel it. To block a default (form submit, a key), call `document.prevent_default("form-id", "submit")` *before* the input happens, then handle the event in Python.
6. **Stale documents:** `window.load_html()`/`load_file()` returns a new `Document`; the old one, its nodes, callbacks and bitmaps are retired and raise `StaleDocumentError`. Re-register callbacks on the new document; read `window.document` for the current one.
7. **Threads:** event callbacks run one at a time on one background thread, in order. Slow work goes in a `threading.Thread`; document methods are thread-safe. `on_before_close`/`on_before_quit` callbacks run on other threads, possibly concurrently: protect shared state with a lock, return a bool quickly.
8. **Text offsets are UTF-16 units** in `control_selection`, `set_control_selection`, `replace_control_range` (an emoji counts as 2), not Python indices.
9. **Native services come from Python:** `window.open_files(...)`, `window.save_file(...)`, `window.choose_folder(...)` return `{"cancelled": bool, "paths": [...]}`; `app.clipboard_text()`/`app.set_clipboard_text()`; `app.open_external(path_or_url)`; `app.reveal_file(path)`.
10. **Links never navigate:** clicking `<a href>` sends a `link_activated` event with `href` and `url`; handle it (`app.open_external(event["url"])`, or load another local page). Switch screens with `window.load_file()`/`load_html()`.
11. **Limits:** 16 KiB per `set_text`/`set_value`, 1 MiB HTML per load, 1024 ops per batch, 2 MiB per IPC message. Page large lists (`trim_children`, `query(..., offset, limit)`).
12. **Platform differences are real:** on Wayland a window cannot be positioned and focus is up to the compositor. Requests may raise `RpcError` (code -32001 unsupported) or be ignored; check `window.state()["observed"]` and `app.capabilities()`.

## Typical app

`index.html`:

```html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><link rel="stylesheet" href="style.css"></head>
<body>
  <form id="add-form">
    <label for="task">Task</label>
    <input id="task" name="task" type="text" required>
    <button id="add" type="submit">Add</button>
  </form>
  <ul id="tasks"></ul>
  <p id="status" role="status">Ready</p>
</body>
</html>
```

`app.py`:

```python
from html import escape
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Tasks", app_id="org.example.Tasks") as app:
        window = app.open_file(HERE / "index.html", title="Tasks", width=520, height=420)
        document = window.document
        document.prevent_default("add-form", "submit")

        def add_task(event):
            title = document.get_value("task").strip()
            document.batch([
                {"op": "insert_html", "element": "tasks", "position": "beforeend",
                 "html": f"<li>{escape(title)}</li>"},
                {"op": "set_text", "element": "status", "text": "Added " + title},
            ])
            document.set_value("task", "")  # form values are not batch operations

        def report_error(event):
            print("Document error:", event["message"])

        document.on("submit", add_task, element="add-form")
        document.on("error", report_error)
        app.run()


if __name__ == "__main__":
    main()
```

Batch `op` names: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `remove_element`, `move_element` (`target`, `position`), `trim_children` (`keep`, `from`: `"start"`/`"end"`). Max 1024; all-or-nothing.

## Verify your work

- Syntax/import: `python -c "import ui4me_core; print(ui4me_core.__version__)"`.
- Runtime check: `ui4me-host doctor` and `ui4me-host --version`.
- Headless smoke test (no desktop needed): `UI4ME_TEST_MODE=1 python app.py`. No window appears and the app must end itself, so temporarily add `threading.Timer(1.0, app.request_quit).start()` before `app.run()`; exit status 0 means startup, document loading and shutdown worked. Assert on `document.get_text(...)`/`get_value(...)` inside the script.
- Set `UI4ME_VERBOSE=1` to see every host log line. `document.diagnostics()` and `document.computed_style([...], "id")` explain CSS that seems ignored.
- Real input, IME, clipboard, dialogs and appearance need a real desktop session (Wayland on Linux, an interactive session on Windows; not an SSH shell).

## Advanced: explicit host launch

For packaging, launchers or custom process policy, the host can start the backend from a manifest: `ui4me-host run --app app.toml` (the `app.toml` from `ui4me-new` pins the interpreter). In that backend `Application.start()` simply connects; the lower-level `Application.from_environment()` does the same without launcher behavior and raises `StartupError` when no host started the process. `UI4ME_HOST_BINARY=/path/to/ui4me-host` selects a development host build.

## References

- [references/python-api.md](references/python-api.md): every Application/Window/Document/Node/LiveBitmap call, event fields, errors, limits.
- [references/projects.md](references/projects.md): project layout, `ui4me-new`, uv/venv, manifests, packaging, common mistakes.
- [references/css-and-security.md](references/css-and-security.md): supported HTML/CSS, assets, security boundaries, platform differences.
- Online docs: [Python API](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/python-api.md), [patterns](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns.md), [HTML/CSS scope](https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md), [platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md).

If a method is not listed, read the installed `ui4me_core/application.py`; never invent methods or browser APIs.
