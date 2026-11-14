# Your first Python app

This guide builds a small native window with HTML, CSS and Python. You need the
`ui4me-core` package ([install it](install-python.md)); Rust, a browser and JavaScript are
not needed. The same guide for PowerShell is [Your first PowerShell app](quickstart-powershell.md).

## 1. A window in one file

Create a folder with a file named `app.py`:

```python
from ui4me_core import Application

PAGE = """
<main style="font-family: sans-serif; padding: 24px">
  <h1>Hello from Python</h1>
  <button id="hello">Say hello</button>
  <p id="message" role="status">Click the button.</p>
</main>
"""


def main():
    with Application.start(name="Hello UI4ME") as app:
        window = app.open_html(PAGE, title="Hello UI4ME", width=520, height=320)
        document = window.document

        def say_hello(event):
            document.set_text("message", "Hello from native UI4ME!")

        document.on("click", say_hello, element="hello")
        app.run()


if __name__ == "__main__":
    main()
```

Run it like any other script:

```bash
python app.py           # Linux or Windows
uv run app.py           # inside a uv project
```

A window opens; press **Say hello** and the message changes. Close the window to end the
script.

What each line does:

| Line | Purpose |
| --- | --- |
| `Application.start()` | Starts the native window host and connects your script to it. Returns the application object. |
| `app.open_html()` | Creates a native window (size in CSS pixels), loads HTML into it and returns the window; `window.document` is the loaded document. |
| `document.on()` | Runs a function when the element with `id="hello"` is clicked. |
| `document.set_text()` | Changes the text of the element with `id="message"`. |
| `app.run()` | Tells the host that startup is finished, then runs your callbacks until the user closes the app. |
| `with ... as app:` | Releases the connection when the block ends. |

## 2. Why the top of the script runs twice

`Application.start()` starts the Rust host, which then runs **the same script a second
time** as its supervised backend, with a private connection. In the first run (the
launcher), `start()` waits until the app ends and then exits with the host's exit status
(0 after a normal close), so nothing after it runs there. In the second run it returns the
connected application, and your code below builds the window.

So:

- Put imports and constants above `Application.start()`, and keep the
  `if __name__ == "__main__":` guard.
- Put everything with side effects (reading or writing files, logging, network calls,
  starting threads) inside `main()`, after the call.
- Script arguments are passed on: `python app.py --path data.csv` gives the backend the same
  `sys.argv`, so `argparse` works.
- `print()` output and tracebacks from your script appear in the terminal you started it
  from. Set `UI4ME_VERBOSE=1` before starting to see the host's own log as well.
- Ctrl+C in that terminal stops the app.
- The program must be a file or a module: `python app.py`, `uv run python app.py` and
  `python -m mypackage.app` work; `python -c` and the interactive interpreter do not,
  because the host needs something it can run again.

Useful options:

```python
Application.start(
    name="Invoice Tool",              # window/app name (default: the script name)
    app_id="com.example.InvoiceTool", # stable reverse-DNS identity (default: derived from the script path)
    instance="single",                # a second launch reaches the running app; "multiple" allows several
)
```

## 3. Use HTML and CSS files

Real apps keep the interface in files next to the script. Create `index.html`:

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Greeter</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<main>
  <h1>Greeter</h1>
  <label for="name">Your name</label>
  <input id="name" type="text">
  <button id="greet">Say hello</button>
  <p id="message" aria-live="polite">Type a name and press the button.</p>
</main>
</body>
</html>
```

`style.css`:

```css
body { margin: 0; padding: 28px; font-family: system-ui, sans-serif; }
main { max-width: 480px; display: flex; flex-direction: column; gap: 8px; }
button { padding: 8px 16px; border: 0; border-radius: 8px; background: #3268d8; color: #fff; }
```

And `app.py`:

```python
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Greeter", app_id="org.example.Greeter") as app:
        window = app.open_file(HERE / "index.html", title="Greeter", width=560, height=360)
        document = window.document

        def greet(event):
            name = document.get_value("name").strip() or "world"
            document.set_text("message", f"Hello, {name}!")

        document.on("click", greet, element="greet")
        app.run()


if __name__ == "__main__":
    main()
```

`app.open_file()` resolves `style.css`, images and fonts relative to the HTML file. Use
`HERE` (the script's folder) so the script works from any current directory. Elements are
found by their HTML `id`, so give an `id` to everything your script reads or changes. For
HTML strings, pass `asset_root=HERE` to `open_html()` if they reference local files.

`document.set_text()` never interprets HTML, which makes it safe for anything a user typed.
There is no JavaScript: `<script>` and `onclick="..."` do nothing. Handle events in Python
with `document.on()` instead.

`app_id` gives the app a stable identity (reverse-DNS style). By default only one copy of an
app runs: starting it again while it is open hands the launch to the running copy. Pass
`instance="multiple"` to `Application.start()` to allow several.

## 4. Or generate a project

```bash
ui4me-new my-app --name "My App"
python my-app/app.py        # or, inside the folder: uv run app.py
```

This writes `app.py`, `index.html`, `style.css`, a `pyproject.toml` (so `uv run app.py`
installs `ui4me-core` automatically) and an optional `app.toml`. Daily development just
runs `app.py`.

## 5. Launching the host explicitly (advanced)

Installers, desktop shortcuts and process managers can start the host from a checked-in
manifest such as the generated `app.toml`:

```bash
ui4me-host run --app my-app/app.toml
```

A backend started this way can still call `Application.start()`, which then just connects.
Older code that uses `Application.from_environment()` keeps working with a manifest launch.
`ui4me-new --python PATH` pins the interpreter the manifest names. See
[`ui4me-host` and app.toml](../reference/host-cli.md).

## Common mistakes

- **Work above `Application.start()` happens twice.** Move it into `main()`, after the call.
- **Nothing happens on click.** Check that the `element=` id exists in the HTML, and that
  `app.run()` is reached.
- **A callback cannot change a variable of `main()`.** Declare it `nonlocal` in the callback
  (see the `hello` example), or keep state in a dictionary or object.
- **`input()` hangs or fails.** The backend has no standard input; ask with a form control.

## Next steps

- [Python recipes](patterns.md): forms, lists, batch updates, screens, styles, file dialogs, close guards, background threads and live images.
- [Python API reference](../reference/python-api.md): every call.
- [Application lifecycle](../concepts/lifecycle.md) and [HTML/CSS scope](../concepts/html-css-scope.md).
- [Troubleshooting](troubleshooting.md).
- Examples in the repository: [`examples/python`](https://github.com/ui4me/ui4me-core/tree/main/examples/python).
