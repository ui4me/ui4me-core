# Local HTML, CSS and Python

From the repository root, build `ui4me-host`, then run:

```sh
python3 tools/run_renderer.py
```

Use `python` on Windows. The launcher writes a manifest with the active Python
interpreter's exact path. `renderer_demo.py` is the application; `ui/index.html`,
`ui/style.css` and their local assets are its interface. The app has no Python
package dependencies beyond the standard library. The Rust executable owns the
native event loop, document, editing, rendering and desktop services.

`application.py` is a small supplied source facade over `client.py`'s private
socket/named-pipe protocol. It exposes `Application`, `Window`, generation-bound
`Document` and opaque `Node` handles, named application helpers and `Document.command()` for the complete
selected protocol. It is not a second DOM model or a separately published SDK.

```python
from pathlib import Path
from application import Application

with Application.from_environment() as app:
    window = app.open_file(Path(__file__).with_name("ui") / "index.html",
                           title="My application", width=1000, height=700)
    document = window.document
    document.on("click", lambda event: document.set_text("status", "Saved"),
                element="add-row")
    document.on("error", lambda event: print(event["message"]))
    app.run()
```

Use IDs present in your HTML, or query anonymous nodes with `query_nodes` and
wrap their references with `document.node(reference=...)`. Application notifications are serialized in event
order on one backend worker; the independent transport reader resolves nested
RPC calls, and the separate host-request pool answers close/quit policy. Lifecycle
policy callbacks can run concurrently with application event callbacks; protect
shared state when registering custom policy.
Native editing/painting continues even if an application callback is slow.
Native close/quit and shutdown can also proceed while a notification callback is
waiting for a native picker. Client close does not wait for blocked application
callback code.
Register native default-action prevention before the input event, rather than
trying to cancel it after the notification reaches Python.

`Window.load_file()` / `load_html()` replace a document only after the new source
is admitted. An old `Document` is then stale and cannot mutate the replacement.
Current-document native failures are logged and emitted as `error`; fatal paint
failures retire that document while the native window stays available. A handler
can load a replacement into the window.

For a standalone installation outside the checkout:

```sh
python3 tools/install.py --example-dir /permanent/path/ui4me-renderer-reference
python3 tools/create_launcher.py install --app /permanent/path/ui4me-renderer-reference/app.toml
```

The installer copies the backend, facade, transport client, complete `ui/` assets,
font license and icon. It refuses to overwrite a nonempty destination. The
manifest pins the active interpreter, so keep that interpreter installed.

`demo.py` and `app.toml` retain the fixed v0.1 host probe. Launch that with
`tools/run_example.py`; install it separately with `tools/install.py --probe`.
`faults.py` retains deliberately misbehaving process fixtures. None of these
backends is meant to be run directly without the host-provided private endpoint
and authentication token.

See [PROTOCOL_DOCUMENTS.md](../../docs/PROTOCOL_DOCUMENTS.md) for the wire/API
contract and [RENDERER_ACCEPTANCE.md](../../docs/RENDERER_ACCEPTANCE.md) for actual
native qualification. `--headless` and `--native-test` renderer launcher modes
produce bounded semantic/smoke evidence, not a claim that every desktop scenario
has been tested.

## Core 1.0 helpers

`Document` adds focus/blur, hit-testing, UTF-16 control selection and replacement,
editing commands, ordered form data, reset/submit/custom validity, node mutation
transactions, stylesheet inspection, dialogs/popovers and resource bundles.
`Window` adds native size constraints, state/attention and file picker methods;
`Application` adds preferences, clipboard and parent-aware external services.
See [CORE_1.0_PROTOCOL](../../docs/CORE_1.0_PROTOCOL.md) for exact parameters and limits.

Selection offsets count UTF-16 units: in `A😀B`, select 1..3 for the emoji.
Python indices are different. `window.open_files(wait=False)` returns the actual
transport future so `cancel()` asks the native picker to close. Per-window
`on_before_close` callbacks are routed independently.

The automated renderer journey includes `completion_checks.py`. The normal
example is static while idle. Add the `demonstrate-animation` class to the body
explicitly to exercise the pulse animation, and remove it afterward. Incoming
drops require `data-ui4me-drop="files text"` (or either type) on a receiving
ancestor; the example body opts in explicitly.
