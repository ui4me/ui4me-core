# Python examples

Install `ui4me-core` (see [Install for Python](../../docs/guides/install-python.md)), then run any of these like a normal script.

| Example | Shows | Run |
| --- | --- | --- |
| [`hello.py`](hello.py) | One window, one button, one callback | `python hello.py` |
| [`notes/`](notes/app.py) | HTML and CSS files, a form, a list built from Python, a native save dialog | `python notes/app.py` |
| [`live_bitmap_demo.py`](live_bitmap_demo.py) | A frameless window with an HTML title bar and a live RGBA bitmap fed from a Python thread | `python live_bitmap_demo.py` |

All three use `Application.start()`: the first run starts the native host, and the host runs the script again as the connected app. See [Your first Python app](../../docs/guides/quickstart-python.md).

## Desktop workspace (manifest launch)

[`renderer_demo.py`](renderer_demo.py) with [`ui/`](ui/index.html) is the larger reference app shown in the README screenshot. It has a sidebar layout, forms, a native file picker, an HTML dialog and popover, a second window, local fonts, PNG and SVG images, theming and drag-and-drop. It is also the app driven by the native acceptance tests, so it uses the explicit manifest launch and `Application.from_environment()`:

```bash
python tools/run_renderer.py              # from the repository root
```

`run_renderer.py` writes a temporary manifest that names the active Python interpreter and starts the host from your build (`target/debug` or `target/release`) or from `PATH`. To copy the app somewhere permanent, with the client package beside it and an `app.toml` you can launch with `ui4me-host run --app`, use `python tools/install.py --example-dir DIR`.

## Test fixtures

These are used by the test tools and are not meant as starting points:

| File | Used by |
| --- | --- |
| `demo.py`, `app.toml` | Protocol and lifecycle probe (`tools/run_example.py`) |
| `faults.py` | Failure injection: crashes, hangs, floods, orphaned children (`tools/run_faults.py`) |
| `completion_checks.py` | Extra checks inside the renderer acceptance journey |
| `live-bitmap.toml`, `renderer-app.toml` | Manifests for launching the examples through `ui4me-host run --app` |

From a source checkout, the runners put `python/` on `PYTHONPATH` so the examples import the package from the checkout.
