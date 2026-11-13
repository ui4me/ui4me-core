# First UI4ME application — 5 minutes

UI4ME is a native Rust host controlled by Python. A Python backend **cannot** create the window by itself: the host launches it with a private connection and owns the UI event loop.

## 1. Install

Linux x86-64 (glibc 2.34+):

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install ui4me-core
```

Windows PowerShell x86-64:

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install ui4me-core
```

The packaged host is installed with the wheel. No Rust compilation or Node/npm is needed. On an OS without a published wheel, do not assume `pip` can build a compatible native host.

## 2. Generate a full application

```bash
ui4me-new hello-ui --name "Hello UI4ME"
```

The generator writes `app.toml`, `app.py`, `index.html` and `style.css`. `app.toml` names the interpreter from your active venv, so keep that venv installed or regenerate/adjust the manifest if moving the application.

## 3. Run it

```bash
ui4me-host run --app hello-ui/app.toml
```

**Do not run** `python hello-ui/app.py` by itself. That file expects the native host to supply the authenticated local connection. `ui4me-host doctor` checks the host installation.

## 4. Edit the interface

`index.html` loads the local stylesheet and contains an `<input>`, a button and a status paragraph. In `app.py`, `doc.on("click",...)` registers a Python handler. Changing the input or pressing the button uses the native DOM/form/keyboard implementation, not JavaScript.

A useful pattern is:

```python
def save(event):
    value = doc.get_value('item-name')
    doc.batch([
        {'op': 'set_text', 'element': 'status', 'text': 'Saved ' + value},
        {'op': 'add_class', 'element': 'status', 'name': 'saved'},
    ])
    doc.set_value('item-name', '')  # form values are set outside batches

doc.on('click', save, element='add-item')
```

Call `doc.replace_html` or `doc.insert_html` for markup changes, and `doc.set_text` for untrusted user-entered strings. There is no JavaScript/HTML `onclick=` interpreter.

## 5. Load generated HTML instead of files

```python
from ui4me_core import Application

with Application.from_environment() as app:
    win = app.open_html('<button id="run">Run</button><p id="status">Waiting</p>', title='Generated HTML')
    doc = win.document
    doc.on('click', lambda _: doc.set_text('status', 'Complete'), element='run')
    app.run()
```

Put this inside an app backend launched through a manifest. For local fonts/images/stylesheets with string HTML, pass `asset_root=Path(__file__).resolve().parent`.

When replacing a whole page, `doc = window.load_html(new_html)` or `window.load_file(path)` returns a **new document generation**. Register its handlers again; old node/document objects reject mutations.

## 6. What next?

[Python API](../reference/python-api.md) · [Forms, resource bundles and bitmaps](patterns.md) · [Agent skill installation](agent-skills.md) · [Platform limitations](platforms.md).
