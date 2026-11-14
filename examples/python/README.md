# Python examples

Small, complete HTML/CSS/Python apps for UI4ME Core. Each example is a folder with its own `app.py`, `index.html` and `style.css`, and imports the installed `ui4me_core` package (`from ui4me_core import Application`). The same apps for PowerShell are in [`powershell/examples`](../../powershell/examples/README.md).

| Example | Shows | Run it with |
| --- | --- | --- |
| [`hello`](hello/app.py) | **Start here.** One window, a text field and a button; `Application.start()`, `app.open_file()`, a click callback, `document.get_value()`, `document.set_text()`. | `python examples/python/hello/app.py` |
| [`notes`](notes/app.py) | HTML and CSS files, a form, a list built from Python (`replace_html` with escaped text), a native save dialog. | `python examples/python/notes/app.py` |
| [`counter`](counter/app.py) | Click callbacks, atomic DOM batches (`document.batch()`), the system clipboard, a stylesheet from disk. | `python examples/python/counter/app.py` |
| [`frameless`](frameless/app.py) | A window without system decorations: HTML title bar with native drag, eight resize edges, minimize/maximize/close buttons. | `python examples/python/frameless/app.py` |
| [`live-bitmap`](live-bitmap/app.py) | Raw RGBA8 frames shared with the host through `ui4me-bitmap://`, assembled from precomputed rows by a producer thread and paced to 60 frames/s. | `python examples/python/live-bitmap/app.py` |
| [`showcase`](showcase/app.py) | A multi-page tour of the supported HTML and CSS: cards, forms with validation, flex and grid layouts, typography, a dialog, a popover, toasts and a light/dark theme. See the [showcase guide](../../docs/guides/showcase.md). | `python examples/python/showcase/app.py` |

Install the package first:

```bash
python -m pip install ui4me-core     # in a virtual environment; or: uv add ui4me-core
```

## The direct-start style (all examples)

Every `app.py` is a normal script. Run it with `python app.py` (or `uv run app.py` in a uv project), on Linux or Windows. `Application.start()` starts the native host, which runs the script again as its backend; code above that call runs twice, so the examples keep all real work inside `main()`, below it. This is the recommended style for new apps (it is what `ui4me-new` generates). See [Your first Python app](../../docs/guides/quickstart-python.md).

## The manifest style (`counter`, `frameless`, `live-bitmap`)

These examples also have an `app.toml` manifest that runs the same `app.py`:

```bash
ui4me-host run --app examples/python/counter/app.toml
```

The host starts `app.py` with its private connection, and `Application.start()` then only connects. This style suits launchers and deployments that start the host explicitly. The manifests run the backend with `python3`; on Windows, or to use a virtual environment, change `program` to that interpreter (for example `"{app_dir}/.venv/bin/python"` or `"python"`). See [`ui4me-host` and app.toml](../../docs/reference/host-cli.md).

## From a source checkout

Without the PyPI package, set `UI4ME_HOST_BINARY` to a native host you built and put the package source on `PYTHONPATH`:

```bash
export UI4ME_HOST_BINARY="$PWD/target/release/ui4me-host"   # ui4me-host.exe on Windows
export PYTHONPATH="$PWD/python"
python examples/python/hello/app.py
"$UI4ME_HOST_BINARY" run --app examples/python/counter/app.toml
```

```powershell
$env:UI4ME_HOST_BINARY = (Resolve-Path ./target/release/ui4me-host.exe).Path
$env:PYTHONPATH = (Resolve-Path ./python).Path
python examples/python/hello/app.py
```

## More Python apps (Python only)

These larger apps have no PowerShell counterpart.

[`renderer_demo.py`](renderer_demo.py) with [`ui/`](ui/index.html) is the larger reference app shown in the README screenshot. It has a sidebar layout, forms, a native file picker, an HTML dialog and popover, a second window, local fonts, PNG and SVG images, theming and drag-and-drop. It is also the app driven by the native acceptance tests, so it uses the explicit manifest launch and `Application.from_environment()`:

```bash
python tools/run_renderer.py              # from the repository root
```

`run_renderer.py` writes a temporary manifest that names the active Python interpreter and starts the host from your build (`target/debug` or `target/release`) or from `PATH`. To copy the app somewhere permanent, with the client package beside it and an `app.toml` you can launch with `ui4me-host run --app`, use `python tools/install.py --example-dir DIR`.

[`live_bitmap_demo.py`](live_bitmap_demo.py) with [`ui/live_bitmap.html`](ui/live_bitmap.html) combines a frameless window and a live bitmap; it is the app in the [live bitmap screenshot](../../docs/images/live-bitmap.png). Run it with `python examples/python/live_bitmap_demo.py` or through `live-bitmap.toml`.

## Test fixtures

These are used by the test tools and are not meant as starting points:

| File | Used by |
| --- | --- |
| `demo.py`, `app.toml` | Protocol and lifecycle probe (`tools/run_example.py`) |
| `faults.py` | Failure injection: crashes, hangs, floods, orphaned children (`tools/run_faults.py`) |
| `completion_checks.py` | Extra checks inside the renderer acceptance journey |
| `live-bitmap.toml`, `renderer-app.toml` | Manifests for launching `live_bitmap_demo.py` and `renderer_demo.py` through `ui4me-host run --app` |

From a source checkout, the runners put `python/` on `PYTHONPATH` so the examples import the package from the checkout.
