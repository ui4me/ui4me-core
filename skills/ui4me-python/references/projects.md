# Project setup and troubleshooting

## Recommended workflow

1. Create a virtual environment. Install `ui4me-core` from PyPI or the appropriate wheel.
2. Run `ui4me-new folder --name "My App"` using that interpreter. It creates `app.toml`, `app.py`, `index.html`, and `style.css`.
3. Start **the host**: `ui4me-host run --app folder/app.toml`. The host starts the Python backend.
4. Edit the HTML/CSS; use `doc.batch` or element methods to change UI data. Reload a document to use new file contents when developing. Re-register events after reload.
5. Register a platform launcher when shipping; see repository's `tools/create_launcher.py`. Distribute both the app and its Python environment/requirements.

## Avoid typical mistakes

- `python app.py` fails because there is no host endpoint/token. Use the host command.
- A backend manifest points to a deleted virtualenv. Update `backend.program` with the active `sys.executable`/`ui4me-new --python ...`.
- A callback uses a stale document after `load_html`. Capture/re-register handlers on the new document.
- You generated HTML containing `<script>` or `onclick=`; neither runs. Use Python event subscriptions.
- You pass an unsafe unescaped user string into `replace_html`; use `html.escape()` or `set_text()`.
- Relative images/CSS fail after `open_html`; pass `asset_root=Path(__file__).resolve().parent` or use a file-based document.
- A CSS feature is ignored; check `doc.diagnostics()` and compare the supported desktop profile before rewriting the entire app.
- GUI tests only use SSH on Windows; run in the actual interactive desktop/RDP session. Linux needs a real Wayland/KDE session for native tests.

## Compatibility

Binary wheels currently cover Linux x86-64 glibc 2.34+ and Windows x86-64. No Rust compile step is needed when installing a wheel. Python 3.10+ is supported. macOS, X11 and ARM64 are outside the qualified release. No JS, WebKit or Chromium is bundled.

## Packaging skill and agent use

`python -m ui4me_core.skills install --agents all` installs into each supported per-user skill directory. Agents require support for the SKILL.md format; there is no operating-system-wide universal skills directory. Use `status`, `uninstall`, `--scope project --project PATH` and `--dry-run` as needed. It never edits agent settings or other skills.
