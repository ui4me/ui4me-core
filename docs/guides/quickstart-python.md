# Your first Python app

This guide assumes you have [installed `ui4me-core`](install-python.md).

## A window in one file

Create `app.py`:

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

Run it:

```bash
python app.py
uv run app.py           # inside a uv project
```

Close the window to quit. `print()` output from your callbacks appears in the terminal.

## How `Application.start()` works

UI4ME's native host owns the window and the event loop. `Application.start()` therefore does one of two things:

1. **When you run the script yourself**, there is no host yet. UI4ME writes a private temporary configuration, starts the bundled host and waits. The host then **runs your script a second time** as its supervised, connected app process. When the app ends, the first process exits with the same status (0 after a normal close). It never returns from `start()`.
2. **In that second run**, `start()` connects to the host and returns the `Application`.

So **everything above `Application.start()` runs twice.** Keep imports and constants there, but put side effects (writing files, network calls, starting threads) inside `main()` after `start()`, and keep the `if __name__ == "__main__":` guard.

Useful options:

```python
Application.start(
    name="Invoice Tool",              # window/app name (default: the script name)
    app_id="com.example.InvoiceTool", # stable reverse-DNS identity (default: derived from the script path)
    instance="single",                # a second launch reaches the running app; "multiple" allows several
)
```

Other ways to start the same script all work: `uv run python app.py`, `python -m mypackage.app` and arguments such as `python app.py --debug` (your `sys.argv` is identical in the app process). `python -c` and the interactive interpreter cannot be used, because the host needs a file or module to run again.

## HTML and CSS in separate files

Create `index.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Notes</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <main>
    <h1>Notes</h1>
    <input id="note" type="text" placeholder="Write something">
    <button id="save">Save</button>
    <p id="status" role="status">Nothing saved yet.</p>
  </main>
</body>
</html>
```

`style.css`:

```css
body { font-family: sans-serif; margin: 0; padding: 24px; background: #f4f6fb; color: #17233a; }
@media (prefers-color-scheme: dark) { body { background: #151a24; color: #e6ebf5; } }
button { padding: 8px 16px; border-radius: 6px; }
```

And `app.py`:

```python
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Notes") as app:
        window = app.open_file(HERE / "index.html", title="Notes", width=640, height=420)
        document = window.document

        def save(event):
            text = document.get_value("note").strip()
            document.set_text("status", f"Saved: {text}" if text else "Nothing to save.")

        document.on("click", save, element="save")
        app.run()


if __name__ == "__main__":
    main()
```

Relative links such as `style.css`, images and local fonts resolve against the HTML file. For HTML strings, pass `asset_root=HERE` to `open_html()` if they reference local files. `<script>` and inline `onclick=` handlers are not run; connect events in Python with `document.on()`.

## Create a project instead

```bash
ui4me-new my-first-app --name "My First App"
cd my-first-app
python app.py           # or: uv run app.py
```

The generator writes `app.py`, `index.html`, `style.css`, a `pyproject.toml` (so `uv run app.py` installs `ui4me-core` automatically) and an optional `app.toml`.

## Launching the host explicitly (advanced)

Installers, desktop shortcuts and process managers can start the host from a checked-in manifest:

```bash
ui4me-host run --app app.toml
```

A backend started this way can still call `Application.start()`, which then just connects. Older code that uses `Application.from_environment()` keeps working with a manifest launch. See [`ui4me-host` and app.toml](../reference/host-cli.md).

Next: [Python recipes](patterns.md), [Lifecycle and events](../concepts/lifecycle.md), [Python API](../reference/python-api.md).
