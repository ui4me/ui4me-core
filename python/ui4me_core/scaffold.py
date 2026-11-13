"""Create a complete UI4ME HTML/CSS/Python app without requiring Rust to scaffold."""
from __future__ import annotations

import argparse
import hashlib
import html as html_escape
import json
import re
import shlex
import sys
import unicodedata
from pathlib import Path


def app_slug(value: str) -> str:
    text = value.strip()
    # Fold accents (Café -> cafe) so most Latin names keep a readable ID.
    folded = unicodedata.normalize('NFKD', text).encode('ascii', 'ignore').decode('ascii')
    slug = re.sub(r'[^a-zA-Z0-9_-]', '-', folded).strip('-_').lower()
    slug = re.sub(r'[-_]+', '-', slug)
    if not slug and any(ch.isalpha() for ch in text):
        # Names written only in non-Latin scripts get a stable ASCII ID.
        slug = 'app-' + hashlib.sha256(text.encode('utf-8')).hexdigest()[:10]
    if not slug or not slug[0].isalpha():
        raise ValueError('App name needs a letter and must form a valid application ID')
    return slug


def toml_string(text: str) -> str:
    """A JSON string literal is a valid TOML basic string with escaped backslashes."""
    return json.dumps(text, ensure_ascii=False)


def templates(name: str, python: Path | str) -> dict[str, str]:
    slug = app_slug(name)
    name = name.strip()
    interpreter = str(python)
    if not interpreter or '\x00' in interpreter:
        raise ValueError('Invalid Python interpreter')
    manifest = f'''[app]
id = "org.ui4me.examples.{slug.replace('-', '_')}"
name = {toml_string(name)}
version = "1.0.0"

[backend]
program = {toml_string(interpreter)}
args = ["{{app_dir}}/app.py"]
cwd = "{{app_dir}}"

[host]
startup_timeout_ms = 30000
shutdown_timeout_ms = 5000
max_windows = 4
instance = "single"
'''
    code = f'''"""A native HTML/CSS window controlled by Python.

Start with: ui4me-host run --app app.toml
Do not launch app.py directly; the native Rust host starts the Python backend.
"""
from pathlib import Path
from ui4me_core import Application

HERE = Path(__file__).resolve().parent
APP_NAME = {name!r}


def main() -> None:
    with Application.from_environment() as app:
        window = app.open_file(HERE / "index.html", title=APP_NAME, width=860, height=560)
        document = window.document

        def add_item(event: dict) -> None:
            title = document.get_value("item-name").strip() or "Untitled"
            # Form values are not batch operations; set them directly.
            document.set_text("status", "Added: " + title)
            document.set_value("item-name", "")

        document.on("click", add_item, element="add-item")
        document.on("error", lambda event: print("Document error:", event.get("message")))
        app.run()


if __name__ == "__main__":
    main()
'''
    html = f'''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html_escape.escape(name)}</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <main class="app">
    <p class="eyebrow">Native desktop · HTML · CSS · Python</p>
    <h1>{html_escape.escape(name)}</h1>
    <p class="lead">It's running.</p>
    <p>Built from local files, with the native Rust runtime.</p>
    <section class="card">
      <label for="item-name">Name</label>
      <input id="item-name" type="text" value="A new item">
      <button id="add-item" type="button">Add item</button>
      <p id="status" role="status" aria-live="polite">Ready.</p>
    </section>
  </main>
</body>
</html>
'''
    css = '''* { box-sizing: border-box; }
html { font-family: sans-serif; color-scheme: dark; background: #0d1727; color: #ecf4ff; }
body { margin: 0; }
.app { max-width: 700px; margin: 60px auto; padding: 28px; }
h1 { font-size: 36px; margin: 8px 0 12px; }
.eyebrow { color: #79b8ff; font-weight: 600; }
.card { margin-top: 30px; padding: 26px; background: #1b2b42; border: 1px solid #3a4e67; border-radius: 12px; }
label { display: block; margin-bottom: 8px; }
input { background: #142136; border: 1px solid #55718d; border-radius: 6px; padding: 10px; color: #fff; width: 100%; }
button { margin-top: 14px; background: #79b8ff; color: #06192e; border: none; padding: 11px 20px; border-radius: 7px; cursor: pointer; }
#status { color: #afd6ff; margin-top: 18px; }
'''
    return {'app.toml': manifest, 'app.py': code, 'index.html': html, 'style.css': css}


def create_project(destination: Path, *, name: str | None = None, python: Path | str = sys.executable) -> list[Path]:
    destination = destination.expanduser().absolute()
    files = templates(name or destination.name, python)
    if destination.is_symlink() or (destination.exists() and any(destination.iterdir())):
        raise FileExistsError(f'Refusing to overwrite existing project directory: {destination}')
    destination.mkdir(parents=True, exist_ok=True)
    created=[]
    for filename, content in files.items():
        path = destination / filename
        with path.open('x', encoding='utf-8', newline='\n') as handle:
            handle.write(content)
        created.append(path)
    return created


def shell_argument(path: Path | str, *, windows: bool = sys.platform == 'win32') -> str:
    """Quote a path for copy/paste into PowerShell (Windows) or a POSIX shell."""
    text = str(path)
    if windows:
        if re.fullmatch(r'[A-Za-z0-9_.:\\/-]+', text):
            return text
        return "'" + text.replace("'", "''") + "'"
    return shlex.quote(text)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='New or empty project directory')
    parser.add_argument('--name', help='Display name (defaults to directory name)')
    parser.add_argument('--python', default=sys.executable, help='Interpreter used as the managed backend, normally your active virtual environment')
    args=parser.parse_args(argv)
    try:
        paths=create_project(args.directory, name=args.name, python=args.python)
    except (OSError, ValueError) as error:
        parser.exit(2, f'ERROR: {error}\n')
    print('Created UI4ME app:')
    for file in paths:
        print(' ', file)
    print(f'Run: ui4me-host run --app {shell_argument(paths[0])}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
