---
name: ui4me-python
description: Build native Linux and Windows desktop GUI applications in Python with UI4ME Core, using local HTML and CSS for the interface and Python callbacks for behavior (no browser, no JavaScript, no web server). Use when creating or modifying a ui4me_core app - Application.start(), windows, documents, events, forms, batch updates, styling, native file dialogs, clipboard, close/quit policy, custom title bars, live RGBA bitmaps - or when debugging one.
---

# Build a desktop app with UI4ME Core (Python)

**Use this skill when a user wants a desktop GUI written in Python with HTML and CSS.**
UI4ME Core is a native Rust host that creates the windows, lays out and paints HTML/CSS,
handles input, IME, editing and accessibility, runs the event loop, and sends events to a
**separate supervised Python process** over authenticated local IPC. It does **not** run
JavaScript, WebKit/Chromium/Qt, Electron or a web server. Never write `<script>`,
`onclick=`, `fetch`, HTTP servers, Flask/React code or browser DOM APIs for these apps.
HTML containing `<script>`, `<iframe>`, any `on...=` attribute, `contenteditable`,
`<input type="color">`/date/time inputs, or duplicate `id`s **fails to load**
(`RpcError` -32602).

Read [references/python-api.md](references/python-api.md) (every call) before writing
non-trivial code. Check a signature with `help(Document.set_text)` or the installed
`ui4me_core/application.py` rather than guessing.

## Fast path

```sh
python -m pip install ui4me-core     # in a virtual environment; or: uv add ui4me-core
ui4me-new my-app --name "My App"     # app.py, index.html, style.css, pyproject.toml, app.toml
python my-app/app.py                 # or, inside my-app: uv run app.py
```

The project has `app.py` (the logic), `index.html`, `style.css`, a `pyproject.toml` and an
optional `app.toml` for launchers that start the host explicitly (`ui4me-host run --app`).
Edit the files and run `app.py` again. Wheels exist for Linux x86-64 (glibc 2.34+, Wayland
desktop) and Windows x64; Python 3.10+.

## Minimal program

```python
from ui4me_core import Application


def main():
    with Application.start(name="My App", app_id="org.example.MyApp") as app:
        window = app.open_html("<h1>Hello UI4ME</h1>", title="My App", width=520, height=320)
        app.run()


if __name__ == "__main__":
    main()
```

Run with `python app.py`, `uv run app.py`, `uv run python app.py` or `python -m package.app`.

## How `Application.start()` works (read this first)

`python app.py` is the **launcher**: `start()` runs the packaged native host, which starts
the **same command again** as the backend with a private connection. In the launcher
`start()` never returns (it exits with the host's status). In the backend it connects and
returns the `Application`.

- **Code before `Application.start()` runs twice.** Put side effects (file writes, network,
  printing banners, heavy init) after it, inside `main()`, and always use
  `if __name__ == "__main__":`.
- It must be a script file or `python -m`. `python -c`, stdin and the REPL raise
  `ui4me_core.StartupError`.
- Arguments pass through: `python app.py --debug` gives the backend the same `sys.argv`.
- `print()` in the backend appears in the terminal. `input()` does not work (no stdin).
- Options: `name`, `app_id` (reverse-DNS; default derived from the script path, so set one
  for real apps), `version`, `icon`, `instance="single"|"multiple"`, `startup_timeout_ms`,
  `shutdown_timeout_ms`, `max_windows`. Invalid values raise `ValueError`.
- Exit status: 0 normal; nonzero if the backend failed (the host shows an error window);
  128+signal on Ctrl+C.

## Rules

1. **Start with `Application.start()`** inside `main()` of a `.py` file, as a `with`
   block, and run the file directly (`python app.py`).
2. **Everything above `Application.start()` runs twice** (launcher and backend). Keep only
   imports and constants at module level; put side effects after the call.
3. App shape: `Application.start()` -> `app.open_file()`/`app.open_html()` (or
   `app.create_window()` + `window.load_file()`) -> register callbacks with
   `document.on(...)` -> `app.run()` (announces readiness, blocks until shutdown).
   Leaving the `with` block closes the connection.
4. Address elements by HTML **id** (`element="save"` is an id, not a selector). Put an
   `id` on everything the script reads or changes. `element=` also matches events bubbling
   from descendants; `event["element"]` is the nearest identified element.
5. `set_text`/`set_value` for any text that comes from users, files or the network; they
   are never parsed as HTML. Use `insert_html`/`replace_html` only with trusted or
   `html.escape()`d markup.
6. Loading another document (`window.load_html()`/`load_file()`) makes the old `Document`,
   its nodes, bitmaps and subscriptions stale (`StaleDocumentError`). Use the returned
   document, or `window.document`, and register callbacks again.
7. Event callbacks run one at a time, in host order, on one background thread of your
   program. Keep them short; slow work goes in a `threading.Thread` (document methods are
   thread-safe). `on_before_close`/`on_before_quit` run on other threads, possibly at the
   same time: protect shared state with a `threading.Lock`.
8. Callback state: callbacks are closures. To rebind a variable of `main()` from a
   callback declare it `nonlocal` (`nonlocal clicks; clicks += 1`), or keep state in a
   dict/object and mutate it. (PowerShell counterpart: `$script:` and `.GetNewClosure()`.)
9. Events arrive after the native action. A callback cannot cancel it: call
   `document.prevent_default("form-id", "submit")` *before* the input happens.
10. Close/quit policies (`window.on_before_close`, `app.on_before_quit`) **return** a bool:
    `True` allows, `False` vetoes. An exception counts as a veto.
11. Use named `def` callbacks, not lambdas: they show up in tracebacks and can hold several
    statements. (PowerShell counterpart: discard unwanted output with `$null = ...`.)
12. Use only local files and the `ui4me-res://` / `ui4me-bitmap://` schemes for assets.
    Remote `http(s)` stylesheets and images are not fetched.
13. Load files relative to the script: `HERE = Path(__file__).resolve().parent`, then
    `app.open_file(HERE / "index.html")` or `app.open_html(html, asset_root=HERE)`.
14. Text offsets in `control_selection`, `set_control_selection` and
    `replace_control_range` are **UTF-16 units** (an emoji counts as 2), not `str` indices.
15. Source encoding: `.py` files are UTF-8 by default, so non-ASCII text needs no special
    handling. (PowerShell counterpart: save non-ASCII `.ps1` files as UTF-8 with BOM.)

## Typical app

`index.html` (next to `app.py`):

```html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Tasks</title><link rel="stylesheet" href="style.css"></head>
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

Run: `python app.py`. The `required` attribute is validated natively, so an empty task is
never submitted.

## Common tasks

These lines run inside `main()`, where `app`, `window` and `document` exist.

```python
# Several DOM changes atomically
document.batch([
    {"op": "set_text", "element": "count", "text": "3"},
    {"op": "add_class", "element": "count", "name": "changed"},
    {"op": "set_attribute", "element": "save", "name": "disabled", "value": ""},
])

# Lists from data: build escaped HTML, then replace the container's children
items = "".join(f"<li>{escape(row['name'])}</li>" for row in rows)
document.replace_html("list", items)

# Generated page with relative assets
document = window.load_html(html, asset_root=HERE)

# Forms
def remember_name(event):
    state["name"] = event["value"]

def handle_submit(event):
    entries = document.form_data("form")["entries"]

document.on("input", remember_name, element="name")
document.prevent_default("form", "submit")
document.on("submit", handle_submit, element="form")

# Checkboxes and selects
on = document.get_checked("agree")
document.set_selected_values("size", ["large"])

# Right and middle click
def show_menu(event):
    print("context menu at", event["x"], event["y"])

def middle_click(event):
    if event["button"] == 1:
        print("middle button")

document.on("contextmenu", show_menu, element="item")
document.on("auxclick", middle_click, element="item")

# Native dialogs (wait for the user; return cancelled and paths)
r = window.open_files(filters=[{"name": "CSV", "extensions": ["csv"]}])
if not r["cancelled"] and r["paths"]:
    path = r["paths"][0]
s = window.save_file(file_name="report.txt")

# Clipboard, external links
app.set_clipboard_text("hello")
app.open_external("https://example.com")

# Window state
window.set(title="Edited", maximized=True)
window_state = window.state()                  # window_state["observed"]["maximized"]

# Unsaved-changes guard
def allow_close():
    return not state["dirty"]

window.on_before_close(allow_close)

# Second launch of a single-instance app
def on_activated(params):
    print("launched again")

app.rpc.on_notification("app.activated", on_activated)

# Anything else in the document protocol
document.command("scroll_into_view", element="row-40")
```

## Custom title bar (frameless window)

```python
window = app.open_file(HERE / "index.html", title="Tool", decorations=False,
                       min_width=480, min_height=320)
```

```html
<header class="titlebar" data-ui4me-drag>
  <span>Tool</span>
  <nav data-ui4me-no-drag><button id="min">_</button><button id="max">[ ]</button><button id="close">X</button></nav>
</header>
<div class="edge e" data-ui4me-resize="e"></div>  <!-- also n s w ne nw se sw -->
```

```python
def minimize(event):
    window.set(minimized=True)

def toggle_maximized(event):
    observed = window.state().get("observed") or {}
    window.set(maximized=not observed.get("maximized"))

def close(event):
    window.request_close()

document.on("click", minimize, element="min")
document.on("click", toggle_maximized, element="max")
document.on("click", close, element="close")
```

The host performs drag and resize natively; do not try to move the window from a callback.

## Live pixel previews

```html
<img id="view" src="ui4me-bitmap://view" width="320" height="200" alt="Preview">
```

```python
bitmap = document.create_bitmap("view", 320, 200)
frame = bytes(320 * 200 * 4)                   # RGBA, straight alpha, tight rows
bitmap.present(frame)                          # returns when the host took it
```

For animation, produce frames in a `threading.Thread` loop
(`while not stop.wait(1 / 30): ...; bitmap.try_present(frame)`), set `stop` and close the
bitmap after `app.run()` returns. `try_present` returns `None` instead of waiting when a
present is still in progress (that frame is dropped). Build rows with slicing
(`frame[a:b] = row`), not per-pixel loops.

## Errors and debugging

```python
from ui4me_core import RpcError

try:
    document.set_text("missing", "x")
except RpcError as error:
    print(error.code, error.message)
```

- `StaleDocumentError`: an old document/node/bitmap was used. `DisconnectedError`: the host
  went away.
- Exceptions raised in event callbacks are printed (`Notification handler failed: ...`)
  and do not stop the app. An uncaught exception in `main()` ends the app with a nonzero
  exit code and the host shows an error window.
- Output and tracebacks of the backend appear in the terminal that ran `app.py`. Set
  `UI4ME_VERBOSE=1` to also see the host log.
- `document.diagnostics()` and `document.computed_style(["prop"], "id")` explain CSS that
  seems ignored. `document.on("error", callback)` reports asynchronous document errors.

## Verify your work

- Import and version: `python -c "import ui4me_core; print(ui4me_core.__version__)"`.
- Runtime check: `ui4me-host --version` and `ui4me-host doctor`.
- Headless smoke test (no desktop needed): `UI4ME_TEST_MODE=1 python app.py`. No window
  appears and the app must end itself, so temporarily add
  `threading.Timer(1.0, app.request_quit).start()` before `app.run()`; exit status 0 means
  startup, document loading and shutdown worked. Assert on `document.get_text(...)` /
  `get_value(...)` inside the script.
- Real input, IME, clipboard, dialogs and appearance need a real desktop session (Wayland
  on Linux, an interactive session on Windows; not an SSH shell).

## Platform facts

- Linux needs Python 3.10+ on an x86-64, glibc 2.34+ Wayland desktop; Windows x64 needs
  Python 3.10+. macOS, X11-only sessions and ARM64 are not supported.
- Wayland: the app must receive input before it can set the clipboard; `focus=True`,
  positioning (`x`, `y`) and un-minimizing are compositor-controlled and raise `RpcError`
  -32001. Check `window.state()["observed"]` and `app.capabilities()`.
- Executables, scripts and shortcuts are refused as `app.open_external()` targets.
- Links never navigate: `<a href>` sends a `link_activated` event with `href` and `url`.

## Advanced: explicit host launch

For packaging, launchers or custom process policy, the host can start the backend from a
manifest: `ui4me-host run --app app.toml` (the `app.toml` from `ui4me-new` pins the
interpreter). In that backend `Application.start()` simply connects; the lower-level
`Application.from_environment()` does the same without launcher behavior and raises
`StartupError` when no host started the process. `UI4ME_HOST_BINARY=/path/to/ui4me-host`
selects a development host build.

## Do not

- Do not use JavaScript, a webview, Qt, Tkinter, Electron, Node, PowerShell or a web
  server.
- Do not set or hardcode `UI4ME_ENDPOINT`/`UI4ME_TOKEN`, and do not run a
  `from_environment()` backend by hand; use `Application.start()`.
- Do not put side effects above `Application.start()`, and do not expect code after it to
  run in the launcher.
- Do not interpolate untrusted text into HTML.
- Do not reuse documents, nodes or bitmaps after loading another document.
- Do not report a dialog, clipboard or external-open request as successful without checking
  its result.

## References

- [references/python-api.md](references/python-api.md): every Application/Window/Document/Node/LiveBitmap call, event fields, errors, limits.
- [references/projects.md](references/projects.md): project layout, `ui4me-new`, uv/venv, manifests, packaging, common mistakes.
- [references/css-and-security.md](references/css-and-security.md): supported HTML/CSS, assets, security boundaries, platform differences.
- Online docs: [Python API](https://github.com/ui4me/ui4me-core/blob/main/docs/reference/python-api.md), [recipes](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/patterns.md), [HTML/CSS scope](https://github.com/ui4me/ui4me-core/blob/main/docs/concepts/html-css-scope.md), [platforms](https://github.com/ui4me/ui4me-core/blob/main/docs/guides/platforms.md).
- Complete example apps (hello, notes, counter, frameless, live bitmap): https://github.com/ui4me/ui4me-core/tree/main/examples/python

If a method is not listed, read the installed `ui4me_core/application.py`; never invent methods or browser APIs.
