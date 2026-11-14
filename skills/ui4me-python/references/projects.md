# Projects, running and troubleshooting

## Layout

```text
my-app/
  app.py          # Application.start(...) inside main()
  index.html      # structure; ids for everything Python touches
  style.css       # linked from index.html
  assets/         # images, fonts (local only)
  pyproject.toml  # dependencies = ["ui4me-core>=1.0.1"]
  app.toml        # optional manifest for ui4me-host run --app
```

`ui4me-new DIR [--name NAME] [--python PATH]` creates exactly this (the directory must be new or empty). Its `app.py` uses `Application.start(name=APP_NAME, app_id=APP_ID)`; `app.toml` has the same app ID and pins the interpreter given by `--python` (default: the one running `ui4me-new`).

Resolve files from the script, not the working directory: `HERE = Path(__file__).resolve().parent`.

## Running

| Command | When |
| --- | --- |
| `python app.py` | Inside an activated virtual environment with `ui4me-core` installed. |
| `uv run app.py` | In a folder with the generated `pyproject.toml` (uv creates the environment). |
| `python -m package.app` | Package layouts; the `-m` form is preserved for the backend. |
| `ui4me-host run --app app.toml` | Explicit manifest launch (packaging, desktop launchers, custom timeouts/env). |

Useful environment variables: `UI4ME_VERBOSE=1` (all host logs), `UI4ME_TEST_MODE=1` (headless host for tests; no window, app must quit itself), `UI4ME_HOST_BINARY` (development host build).

## Structuring code

- Keep module top level side-effect free; everything runs twice before `Application.start()`.
- Register all callbacks before `app.run()`; `run()` blocks.
- Group per-screen setup in functions (`show_home(window)`), each loading HTML and registering its callbacks on the returned document.
- Keep application state in Python objects; the page only displays it. Guard state shared with policy callbacks or worker threads with a `threading.Lock`.
- Background work: `threading.Thread(daemon=True)`; catch `StaleDocumentError` to stop when the window or page goes away.
- For headless smoke tests, add `threading.Timer(1.0, app.request_quit).start()` behind an `os.environ.get("UI4ME_TEST_MODE") == "1"` check.

## Common mistakes

| Symptom | Cause / fix |
| --- | --- |
| Something happens twice at startup (two log lines, file written twice) | It runs before `Application.start()`. Move it after. |
| `StartupError: ... must run from a script file` | Ran via `python -c`, stdin or a REPL. Save to a file. |
| `StartupError: No UI4ME host connection` | `Application.from_environment()` used with `python app.py`. Use `Application.start()`. |
| `StartupError: ... native host is missing` | Platform wheel not installed (unsupported OS/arch) or wrong environment. Check `ui4me-host doctor`. |
| `RpcError -32602` when loading HTML | The page has `<script>`, `on…=` attributes, `contenteditable`, a color/date input or duplicate `id`s. Remove them. |
| `RpcError: Text exceeds 16 KiB` | Split long text, or render it as a list of elements. |
| Clicking a link does nothing | Links never navigate; handle `link_activated`. |
| `StaleDocumentError` | Using a `Document`/`Node` after `load_html`/`load_file` or after its window closed. Use the returned document / `window.document`. |
| Button does nothing | Wrong `element` id, callback registered on an old document, or JS/`onclick` used. |
| Form clears/reloads on submit | Missing `document.prevent_default("form-id", "submit")` before input. |
| Relative image/CSS missing with `open_html` | Pass `asset_root=HERE`. Network URLs never load. |
| `RpcError -32001` on `window.set(x=..., y=...)` | Wayland cannot position windows; make it optional. |
| UI freezes for later events | A callback is doing slow work; move it to a thread. |
| Selection lands in the wrong place with emoji | Offsets are UTF-16 units, not `str` indices. |
| `input()` hangs/fails | The backend has no stdin; use a form in the window. |
| Nothing visible over SSH | The host needs the user's desktop session (Wayland on Linux; an interactive session on Windows). |

## Packaging and distribution

The wheel contains the native host for its platform; it does not bundle your app or Python. Ship your project with its dependency list (`pyproject.toml`) and run it with `python app.py`/`uv run app.py`, or install a desktop launcher that runs `ui4me-host run --app /path/to/app.toml` with a manifest that names the right interpreter (regenerate it if the virtual environment moves). Use a fixed reverse-DNS `app_id` so single-instance behavior and per-app settings stay stable.

## Agent skill installation

`ui4me-skill install --agents all` (or `claude,codex,opencode,copilot,cursor,gemini`) copies this skill into each agent's per-user folder; `--scope project --project PATH` installs into a repository; `status`, `uninstall`, `--dry-run`, `--force` are available. It never touches unrelated skills or agent settings.

More: https://github.com/ui4me/ui4me-core/blob/main/docs/guides/troubleshooting.md and https://github.com/ui4me/ui4me-core/blob/main/docs/reference/host-cli.md
