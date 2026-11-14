# Python recipes

Ready-to-adapt recipes for common desktop-app tasks, written for the `ui4me_core` Python package. Each one is a complete program: save it as `app.py` and run `python app.py` (or `uv run app.py`), on Linux or Windows. Every recipe works with Python 3.10 to 3.14. The HTML is inline to keep the examples short; in a real project put it in `index.html` and `style.css` and use `app.open_file(...)` (recipe 6). The same recipes for PowerShell are in [PowerShell recipes](patterns-powershell.md).

A few rules apply to every recipe:

- Behavior lives in Python callbacks registered with `document.on(...)`. There is no JavaScript: HTML containing `<script>` or `onclick=`-style attributes is rejected when it loads.
- Code before `Application.start()` runs twice (once in the launcher, once in the backend). Keep only imports and constants such as the HTML strings at module level, and put everything else inside `main()` after `start()`.
- Callbacks run on a background thread of your program, one at a time, *after* the native action already happened. Document methods are thread-safe, so your own threads may call them too.
- A callback is a function nested in `main()`. It can *read* `main()`'s variables (`document`, `window`, a dictionary), but to *assign* one, declare it `nonlocal name` first; a plain `name = ...` only creates a local variable that disappears when the callback returns. Changing an entry of a shared dictionary (`state["dirty"] = True`) needs no declaration.
- Use named `def` callbacks rather than lambdas: they show up by name in tracebacks and can hold several statements.
- Save `.py` files as UTF-8 (Python's default source encoding); no byte-order mark is needed.

Every call used here is described in the [Python API reference](../reference/python-api.md).

## Contents

1. [Respond to a button](#1-respond-to-a-button)
2. [Read a form](#2-read-a-form)
3. [Handle many items with one callback](#3-handle-many-items-with-one-callback)
4. [Update several elements at once](#4-update-several-elements-at-once)
5. [Switch screens by replacing the document](#5-switch-screens-by-replacing-the-document)
6. [Load HTML and CSS files](#6-load-html-and-css-files)
7. [Change styles at runtime](#7-change-styles-at-runtime)
8. [Follow the desktop's light/dark preference](#8-follow-the-desktops-lightdark-preference)
9. [Open and save files with native dialogs](#9-open-and-save-files-with-native-dialogs)
10. [Copy and paste](#10-copy-and-paste)
11. [Ask before closing](#11-ask-before-closing)
12. [Run slow work without freezing events](#12-run-slow-work-without-freezing-events)
13. [Stream pixels with a live bitmap](#13-stream-pixels-with-a-live-bitmap)
14. [Work with elements that have no id](#14-work-with-elements-that-have-no-id)
15. [Keyboard shortcuts](#15-keyboard-shortcuts)

## 1. Respond to a button

```python
from ui4me_core import Application

HTML = """
<button id="save">Save</button>
<p id="status" role="status">Not saved</p>
"""


def main():
    with Application.start(name="Button demo") as app:
        window = app.open_html(HTML, title="Button demo", width=400, height=200)
        document = window.document

        def handle_save(event):
            document.set_text("status", "Saved")

        document.on("click", handle_save, element="save")
        app.run()


if __name__ == "__main__":
    main()
```

`element="save"` is the button's `id`. `set_text` always inserts plain text, so it is safe for anything the user typed. `document.on` returns a function; keep it if you want to remove the callback later by calling it.

## 2. Read a form

Prevent the form's native submission in advance, then handle `submit` in Python. Registering `prevent_default` inside the callback would be too late, because events arrive after the native action.

```python
from ui4me_core import Application

HTML = """
<form id="signup">
  <label for="person">Name</label>
  <input id="person" name="person" type="text" required>
  <label><input id="news" name="news" type="checkbox"> Send me news</label>
  <button id="send" type="submit">Sign up</button>
</form>
<p id="result" role="status">Waiting</p>
"""


def main():
    with Application.start(name="Form demo") as app:
        window = app.open_html(HTML, title="Form demo", width=420, height=260)
        document = window.document
        document.prevent_default("signup", "submit")

        def handle_submit(event):
            name = document.get_value("person").strip()
            wants_news = document.get_checked("news")
            document.set_text("result", f"Welcome, {name}! News: {'yes' if wants_news else 'no'}")
            document.set_value("person", "")

        def show_typing(event):
            document.set_text("result", "Typing: " + event["value"])

        document.on("submit", handle_submit, element="signup")
        document.on("input", show_typing, element="person")
        app.run()


if __name__ == "__main__":
    main()
```

The `required` attribute is checked natively: an empty name does not submit. `document.form_data("signup")["entries"]` returns every entry in order (each with `name` and `value`), if you prefer reading the form in one call. `document.request_submit("signup")` requests a submission from Python, with the same validation and `submit` event.

For live validation, a non-empty message marks the field invalid and `""` clears it:

```python
document.set_custom_validity("person", "That name is taken")
```

Text-control selection (`document.control_selection(...)`, `document.set_control_selection(...)`) uses **UTF-16 offsets**, not Python string indices: an emoji counts as 2 offsets but 1 Python character. Convert with `len(text[:i].encode("utf-16-le")) // 2`.

## 3. Handle many items with one callback

A callback registered with `element="todo-list"` also receives events from everything inside the list, because events bubble. `event["element"]` is the nearest element with an `id`.

```python
from html import escape

from ui4me_core import Application

HTML = """
<input id="new-task" type="text" placeholder="New task">
<button id="add">Add</button>
<ul id="todo-list"></ul>
"""


def main():
    with Application.start(name="Tasks") as app:
        window = app.open_html(HTML, title="Tasks", width=420, height=400)
        document = window.document
        counter = 0

        def add_task(event):
            nonlocal counter
            title = document.get_value("new-task").strip()
            if not title:
                return
            counter += 1
            # Escape user text before putting it into HTML.
            item = f'<li id="task-{counter}">{escape(title)}</li>'
            document.insert_html("todo-list", item)
            document.set_value("new-task", "")

        def remove_task(event):
            if event["element"].startswith("task-"):
                document.remove(event["element"])

        document.on("click", add_task, element="add")
        document.on("click", remove_task, element="todo-list")
        app.run()


if __name__ == "__main__":
    main()
```

`nonlocal counter` is needed because the callback assigns to a variable of `main()`; without it, `counter += 1` would fail with `UnboundLocalError`. Only `insert_html` and `replace_html` (and the `insert_html` and `replace_children_html` batch operations) parse HTML. Treat their input as code: build it from your own templates and pass user text through `html.escape()`.

## 4. Update several elements at once

`document.batch(...)` applies a group of changes as one transaction: either all succeed or none do, and the window repaints once.

```python
from ui4me_core import Application

HTML = """
<h2 id="title">Order</h2>
<p id="total">0.00</p>
<p id="state" class="pending">Pending</p>
<ul id="log"></ul>
<button id="pay">Pay</button>
"""


def main():
    with Application.start(name="Batch demo") as app:
        window = app.open_html(HTML, title="Batch demo", width=400, height=320)
        document = window.document

        def pay(event):
            document.batch([
                {"op": "set_text", "element": "total", "text": "42.00"},
                {"op": "remove_class", "element": "state", "name": "pending"},
                {"op": "add_class", "element": "state", "name": "paid"},
                {"op": "set_text", "element": "state", "text": "Paid"},
                {"op": "append_text", "element": "title", "text": " (complete)"},
                {"op": "insert_html", "element": "log", "position": "beforeend", "html": "<li>Paid</li>"},
                {"op": "trim_children", "element": "log", "keep": 50, "from": "start"},
                {"op": "set_attribute", "element": "pay", "name": "disabled", "value": ""},
            ])

        document.on("click", pay, element="pay")
        app.run()


if __name__ == "__main__":
    main()
```

Batch operations: `set_text`, `append_text`, `set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`, `replace_children_html`, `remove_element`, `move_element` and `trim_children`, at most 1024 per batch. Form values (`set_value`, `set_checked`), focus and stylesheets are separate calls. `trim_children` keeps a growing log bounded: `"from": "start"` removes the oldest entries.

## 5. Switch screens by replacing the document

Loading new HTML into a window replaces the whole page and returns a new `Document`. The old document, its nodes and its callbacks are retired; using them raises `StaleDocumentError`. Register callbacks again on the new document.

```python
from ui4me_core import Application

LOGIN = """
<h1>Sign in</h1>
<input id="user" type="text" value="ada">
<button id="login">Continue</button>
"""

HOME = """
<h1 id="welcome">Home</h1>
<button id="logout">Sign out</button>
"""


def main():
    with Application.start(name="Screens") as app:
        window = app.open_html(LOGIN, title="Screens", width=420, height=240)

        def show_login():
            document = window.load_html(LOGIN)
            document.on("click", handle_login, element="login")

        def show_home(user):
            document = window.load_html(HOME)
            document.set_text("welcome", "Welcome, " + user)
            document.on("click", handle_logout, element="logout")

        def handle_login(event):
            user = window.document.get_value("user")
            show_home(user)

        def handle_logout(event):
            show_login()

        window.document.on("click", handle_login, element="login")
        app.run()


if __name__ == "__main__":
    main()
```

Read `window.document` when you need the current page, instead of keeping an old `Document` variable around. The nested functions above refer to each other and to `window`; Python closures keep those names alive after `main()`'s setup code has run, so nothing extra is needed. For small changes, prefer updating the existing page; reload only to switch whole screens. If a load fails (for example the HTML is over 1 MiB), the previous page stays.

## 6. Load HTML and CSS files

```text
my-app/
  app.py
  index.html
  style.css
  images/logo.png
```

```python
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Files demo") as app:
        window = app.open_file(HERE / "index.html", title="Files demo")
        app.run()


if __name__ == "__main__":
    main()
```

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <img src="images/logo.png" alt="Logo">
  <h1>My desktop app</h1>
</body>
</html>
```

Relative URLs resolve from the HTML file's folder and must stay inside it. Use local files only; network URLs are not loaded. Bundling fonts with `@font-face` makes typography identical on every machine. Build paths from `HERE` (the script's folder): relative paths resolve against the current working directory, which is wherever the user started the script. For HTML built in Python, pass the folder explicitly: `app.open_html(html, asset_root=HERE)`. `ui4me-new my-app` creates this layout for you.

## 7. Change styles at runtime

Toggle classes for state, and add a stylesheet for values computed in Python.

```python
from ui4me_core import Application

HTML = """
<style>
  .card { padding: 16px; border-radius: 8px; background: #f2f4f7; color: #1d2633; }
  .card.highlight { outline: 3px solid #2f80ed; }
</style>
<div id="card" class="card">A card</div>
<button id="toggle">Highlight</button>
<button id="warm">Warm background</button>
"""


def main():
    with Application.start(name="Styles") as app:
        window = app.open_html(HTML, title="Styles", width=420, height=240)
        document = window.document
        added = document.add_stylesheet(".card { font-size: 18px; }")
        sheet = added["sheet"]
        highlighted = False

        def toggle(event):
            nonlocal highlighted
            highlighted = not highlighted
            if highlighted:
                document.add_class("card", "highlight")
            else:
                document.remove_class("card", "highlight")

        def make_warm(event):
            document.replace_stylesheet(sheet, ".card { font-size: 18px; background: #ffe8cc; }")

        document.on("click", toggle, element="toggle")
        document.on("click", make_warm, element="warm")
        app.run()


if __name__ == "__main__":
    main()
```

If a property seems ignored, check `document.computed_style(["outline-color"], "card")` (each property reports `supported`, `unsupported` or `invalid`) and `document.diagnostics()`. See [HTML/CSS scope](../concepts/html-css-scope.md).

## 8. Follow the desktop's light/dark preference

The engine's `prefers-color-scheme` media query follows the desktop setting automatically, so CSS alone usually suffices:

```css
:root { color-scheme: light dark; --bg: #ffffff; --text: #1d2633; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #0d1727; --text: #ecf4ff; }
}
body { background: var(--bg); color: var(--text); }
```

To offer an in-app choice, override it per document from Python:

```python
from ui4me_core import Application

HTML = """
<style>
  body { background: #ffffff; color: #1d2633; }
  @media (prefers-color-scheme: dark) { body { background: #0d1727; color: #ecf4ff; } }
</style>
<select id="theme">
  <option value="system">System</option>
  <option value="light">Light</option>
  <option value="dark">Dark</option>
</select>
<p id="desktop">Desktop preference: unknown</p>
"""


def main():
    with Application.start(name="Theme") as app:
        window = app.open_html(HTML, title="Theme", width=400, height=200)
        document = window.document

        def choose_theme(event):
            document.set_environment(color_scheme=event["value"])

        def show_preferences(snapshot):
            scheme = snapshot.get("color_scheme") or "unknown"
            document.set_text("desktop", "Desktop preference: " + scheme)

        document.on("change", choose_theme, element="theme")
        app.on_desktop_preferences(show_preferences)
        show_preferences(app.desktop_preferences())
        app.run()


if __name__ == "__main__":
    main()
```

`on_desktop_preferences` receives the same dictionary that `app.desktop_preferences()` returns (`color_scheme`, `reduced_motion`, `high_contrast`, `available`, `source`). Desktop values can be `None` when the platform does not report them (for example in a minimal Linux session); design for that case. `document.environment()` shows what the document actually uses and where each value came from.

## 9. Open and save files with native dialogs

File pickers are native operating-system dialogs owned by the window. With the default `wait=True` the call blocks until the user decides and returns `{"cancelled": ..., "paths": [...]}`. Blocking inside an event callback is fine: close and quit still work while a dialog is open.

```python
from pathlib import Path

from ui4me_core import Application

HTML = """
<button id="open">Open…</button>
<button id="save">Save as…</button>
<textarea id="editor" rows="10" cols="60"></textarea>
<p id="status" role="status">No file</p>
"""
TEXT_FILES = [{"name": "Text files", "extensions": ["txt", "md"]}]


def main():
    with Application.start(name="Editor") as app:
        window = app.open_html(HTML, title="Editor", width=640, height=420)
        document = window.document

        def open_file(event):
            result = window.open_files(title="Open a text file", filters=TEXT_FILES)
            if result["cancelled"] or not result["paths"]:
                return
            path = Path(result["paths"][0])
            document.set_value("editor", path.read_text(encoding="utf-8"))
            document.set_text("status", "Opened " + path.name)

        def save_file(event):
            result = window.save_file(file_name="notes.txt", filters=TEXT_FILES)
            if result["cancelled"] or not result["paths"]:
                return
            path = Path(result["paths"][0])
            path.write_text(document.get_value("editor"), encoding="utf-8")
            document.set_text("status", "Saved " + path.name)

        document.on("click", open_file, element="open")
        document.on("click", save_file, element="save")
        app.run()


if __name__ == "__main__":
    main()
```

The script reads and writes with an explicit `encoding="utf-8"` because Python's default file encoding depends on the platform (the ANSI code page on Windows). The dialog returns absolute paths.

Other options: `window.open_files(multiple=True)`, `window.choose_folder()`, and `directory=` for the starting folder. With `wait=False` you get a `concurrent.futures.Future`: `future.result()` returns the result and `future.cancel()` closes the dialog. An `<input type="file">` in the page also opens the native picker; read its paths with `document.files("input-id")`. On Linux the dialog comes from the desktop portal, which decides its own appearance.

## 10. Copy and paste

```python
from ui4me_core import Application, RpcError

HTML = """
<input id="field" type="text" value="Copy me">
<button id="copy">Copy</button>
<button id="paste">Paste</button>
<p id="status" role="status"></p>
"""


def main():
    with Application.start(name="Clipboard") as app:
        window = app.open_html(HTML, title="Clipboard", width=420, height=200)
        document = window.document

        def copy_value(event):
            try:
                app.set_clipboard_text(document.get_value("field"))
                document.set_text("status", "Copied")
            except RpcError as error:
                document.set_text("status", "Copy failed: " + error.message)

        def paste_value(event):
            try:
                document.set_value("field", app.clipboard_text())
                document.set_text("status", "Pasted")
            except RpcError as error:
                document.set_text("status", "Paste failed: " + error.message)

        def show_error(event):
            document.set_text("status", "Error: " + event["message"])

        document.on("click", copy_value, element="copy")
        document.on("click", paste_value, element="paste")
        document.on("error", show_error)
        app.run()


if __name__ == "__main__":
    main()
```

Ctrl+C/Ctrl+V inside text fields already work natively. `document.edit("copy", "field")` runs the same native editing command from Python; it completes asynchronously, and a failure arrives as an `error` event (the third callback above). A clipboard call that the host refuses raises `RpcError`; without the `try`, the exception would only be printed (`Notification handler failed: ...`) and the status would not change. On Wayland the app must have received keyboard or pointer input before it may set the clipboard.

## 11. Ask before closing

A before-close callback returns `True` to let the window close or `False` to keep it open. Closing the last window asks to quit the application, which runs the before-quit callback.

```python
import threading

from ui4me_core import Application

HTML = """
<textarea id="notes" rows="6" cols="40"></textarea>
<p id="status" role="status">Saved</p>
<button id="save">Save</button>
<button id="quit">Quit</button>
"""


def main():
    with Application.start(name="Unsaved changes") as app:
        window = app.open_html(HTML, title="Unsaved changes", width=440, height=300)
        document = window.document
        lock = threading.Lock()
        state = {"dirty": False}

        def mark_dirty(event):
            with lock:
                state["dirty"] = True
            document.set_text("status", "Unsaved changes")

        def save(event):
            with lock:
                state["dirty"] = False
            document.set_text("status", "Saved")

        def quit_app(event):
            app.request_quit()

        def allow_close():
            with lock:
                dirty = state["dirty"]
            if dirty:
                document.set_text("status", "Save first, or your changes will be lost")
            return not dirty

        document.on("input", mark_dirty, element="notes")
        document.on("click", save, element="save")
        document.on("click", quit_app, element="quit")
        window.on_before_close(allow_close)
        app.on_before_quit(allow_close)
        app.run()


if __name__ == "__main__":
    main()
```

Unlike PowerShell, Python runs policy callbacks on a different thread from event callbacks, possibly at the same time, hence the lock. Only the return value decides: `True` allows, anything false (including a missing `return`, which gives `None`) vetoes. Answer quickly; do not open dialogs or do slow work inside a policy. An exception in a policy callback counts as "no".

## 12. Run slow work without freezing events

Event callbacks run one at a time. A callback that takes ten seconds delays every later event. Start a **thread** and send small updates from it; document methods are safe to call from any thread. (PowerShell cannot do this, because its UI4ME commands must stay on the script's runspace; its recipe uses a job and a pump loop instead.)

```python
import threading
import time

from ui4me_core import Application, StaleDocumentError

HTML = """
<button id="start">Start</button>
<progress id="bar" max="100" value="0"></progress>
<p id="status" role="status">Idle</p>
"""


def main():
    with Application.start(name="Worker") as app:
        window = app.open_html(HTML, title="Worker", width=400, height=200)
        document = window.document
        running = threading.Event()

        def work():
            # The thread may call document methods; it reports progress directly.
            try:
                for step in range(1, 101):
                    time.sleep(0.05)  # stand-in for real work
                    document.set_attribute("bar", "value", str(step))
                document.set_text("status", "Done")
            except StaleDocumentError:
                return  # The window closed or the page changed; stop quietly.
            except Exception as error:
                document.set_text("status", f"Stopped: {error}")
            finally:
                running.clear()
            document.remove_attribute("start", "disabled")

        def start(event):
            if running.is_set():
                return  # already running
            running.set()
            document.set_attribute("start", "disabled", "")
            document.set_text("status", "Working...")
            threading.Thread(target=work, daemon=True).start()

        document.on("click", start, element="start")
        app.run()


if __name__ == "__main__":
    main()
```

`daemon=True` lets the program end while a thread is still working; catching `StaleDocumentError` stops it quietly when the window closes. Several threads may run at once, but keep shared state behind a `threading.Lock` (recipe 11). For CPU-heavy work that should use another core, `concurrent.futures.ProcessPoolExecutor` runs it in another process, and its `Future` callbacks can update the page.

## 13. Stream pixels with a live bitmap

For pictures that change many times per second, write raw RGBA pixels into a live bitmap. The pixels travel through shared memory, not JSON, and same-sized frames do not re-run layout. Python produces the frames on its own thread while `app.run()` handles events.

![Live bitmap example](../images/live-bitmap.png)

```python
import math
import threading

from ui4me_core import Application

WIDTH, HEIGHT = 256, 160
# The width and height attributes match WIDTH and HEIGHT.
HTML = f"""
<img id="preview" src="ui4me-bitmap://preview" width="{WIDTH}" height="{HEIGHT}" alt="Animated preview">
"""


def make_band():
    """One band of colours, twice the image width, so that any row can be sliced out of it."""
    band = bytearray(WIDTH * 2 * 4)
    for x in range(WIDTH * 2):
        t = (x % WIDTH) / WIDTH * 2 * math.pi
        band[x * 4] = round(127 + 127 * math.sin(t))
        band[x * 4 + 1] = round(127 + 127 * math.sin(t + 2.1))
        band[x * 4 + 2] = round(127 + 127 * math.sin(t + 4.2))
        band[x * 4 + 3] = 255
    return bytes(band)


def main():
    with Application.start(name="Live bitmap") as app:
        window = app.open_html(HTML, title="Live bitmap", width=320, height=240)
        document = window.document
        bitmap = document.create_bitmap("preview", WIDTH, HEIGHT)
        band = make_band()  # per-pixel Python loops are too slow per frame
        stop = threading.Event()

        def produce():
            frame = bytearray(WIDTH * HEIGHT * 4)
            row_bytes = WIDTH * 4
            tick = 0
            while not stop.wait(1 / 30):  # about 30 frames per second
                tick = (tick + 4) % WIDTH
                for y in range(HEIGHT):
                    start = ((tick + y) % WIDTH) * 4
                    frame[y * row_bytes:(y + 1) * row_bytes] = band[start:start + row_bytes]
                try:
                    bitmap.try_present(frame)  # None means "busy, frame skipped"
                except Exception:
                    break  # The document or window went away.

        producer = threading.Thread(target=produce, daemon=True)
        producer.start()
        try:
            app.run()
        finally:
            stop.set()
            producer.join(timeout=2)
            bitmap.close()


if __name__ == "__main__":
    main()
```

A frame is exactly `width * height * 4` bytes of straight-alpha RGBA, row by row (`bytes`, `bytearray` or `memoryview`). `present` waits until the host has taken the frame, which paces a fast producer; `try_present` returns `None` immediately instead of waiting when another present of the same bitmap is still in progress, so a producer thread skips that frame. (PowerShell has one producer loop, so its `Send-UI4MELiveBitmap -Try` only matters for callbacks.) A bitmap belongs to its document and is closed automatically if the page is replaced. Limits: 4096 pixels per side, 8 bitmaps and 64 MiB per document.

## 14. Work with elements that have no id

`query_nodes` finds elements by CSS selector, including ones without an `id`, and returns opaque references. Wrap them with `document.node(reference=...)`.

```python
from ui4me_core import Application

HTML = """
<style>.marked { font-weight: bold; }</style>
<ul id="list">
  <li>Apples</li>
  <li>Pears</li>
  <li>Plums</li>
</ul>
<button id="mark">Mark every other item</button>
<button id="first">Move last item first</button>
"""


def main():
    with Application.start(name="Nodes") as app:
        window = app.open_html(HTML, title="Nodes", width=400, height=260)
        document = window.document

        def mark(event):
            result = document.query_nodes("#list > li:nth-child(odd)")
            operations = []
            for entry in result["nodes"]:
                operations.append({"op": "add_class", "node": entry["node"], "name": "marked"})
            if operations:
                document.node_batch(operations)

        def move_last_first(event):
            items = document.query_nodes("#list > li")["nodes"]
            if len(items) < 2:
                return
            last = document.node(reference=items[-1]["node"])
            first = document.node(reference=items[0]["node"])
            last.move_to(first, position="beforebegin")

        document.on("click", mark, element="mark")
        document.on("click", move_last_first, element="first")
        app.run()


if __name__ == "__main__":
    main()
```

Node references belong to one document generation; after a reload, look them up again. `node_batch` takes the same operations as `batch`, with `node` instead of `element` (and `target_node` instead of `target` for moves). `document.query(...)` runs the same selector but returns only the `ids` of matches that have one. For a fixed set of elements you control, giving them `id`s and using `batch` is simpler.

## 15. Keyboard shortcuts

```python
from ui4me_core import Application

HTML = """
<textarea id="editor" rows="8" cols="50"></textarea>
<p id="status" role="status">Press Ctrl+S to save</p>
"""


def main():
    with Application.start(name="Shortcuts") as app:
        window = app.open_html(HTML, title="Shortcuts", width=480, height=300)
        document = window.document
        # Stop Ctrl+S from typing into the text area; register before input.
        document.prevent_default("editor", "keydown", key="s", ctrl=True)

        def handle_key(event):
            if event.get("ctrl") and event.get("key", "").lower() == "s":
                document.set_text("status", "Saved")

        document.on("keydown", handle_key, element="editor")
        document.focus("editor")
        app.run()


if __name__ == "__main__":
    main()
```

`.lower()` makes `"S"` (Ctrl+Shift+S, or Caps Lock) match too; compare `event["key"] == "s"` to tell them apart. The event also carries `shift`, `alt` and `meta`.

## Testing a recipe without a desktop

`UI4ME_TEST_MODE=1` runs the host headless: the document engine and lifecycle work, no window is shown, and the program must end itself. Request the quit with a timer just before `app.run()`. `document.request_submit` makes the host send a real `submit` event without any input device, which exercises a callback end to end:

```python
import threading
import time

from ui4me_core import Application


def main():
    with Application.start(name="Smoke test") as app:
        window = app.open_html('<form id="form"><input id="msg" value="Hello"></form><p id="out"></p>',
                               title="Smoke test")
        document = window.document
        if document.get_value("msg") != "Hello":
            raise RuntimeError("unexpected value")

        def copy_message(event):
            document.set_text("out", document.get_value("msg"))

        document.prevent_default("form", "submit")
        document.on("submit", copy_message, element="form")
        document.request_submit("form")
        deadline = time.monotonic() + 5
        while document.get_text("out") != "Hello":
            if time.monotonic() > deadline:
                raise RuntimeError("the submit callback did not run")
            time.sleep(0.05)

        threading.Timer(0.5, app.request_quit).start()
        app.run()


if __name__ == "__main__":
    main()
```

```sh
UI4ME_TEST_MODE=1 python smoke.py; echo "exit code $?"
```

```powershell
$env:UI4ME_TEST_MODE = '1'; python smoke.py; "exit code $LASTEXITCODE"
```

An exception in the script ends the backend, and the launcher exits with a nonzero code, so the smoke test fails in CI. Exceptions inside event callbacks do not end the app; they are printed as `Notification handler failed: ...`, so check the result (as the loop above does). For clicks and key presses there is no headless input: move the logic into a function and call it from the test as well as from the callback. Headless mode cannot check real input, IME, clipboard, native dialogs or how the window looks (clipboard and file-dialog calls raise `RpcError` with code `-32001`); try those on a real desktop. See [platforms](platforms.md) and [troubleshooting](troubleshooting.md).
