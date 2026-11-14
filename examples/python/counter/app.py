"""Counter: click callbacks, atomic DOM batches, native clipboard, CSS from disk.

Run it directly:
  python app.py          (or: uv run app.py)
Or through the manifest:  ui4me-host run --app app.toml
"""
from datetime import datetime
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Python Counter", app_id="org.ui4me.PythonCounter") as app:
        window = app.open_file(HERE / "index.html", title="UI4ME Python Counter", width=760, height=520)
        document = window.document
        counter = 0

        def increase(event):
            nonlocal counter
            counter += 1
            document.batch([
                {"op": "set_text", "element": "count", "text": str(counter)},
                {"op": "set_text", "element": "status",
                 "text": "Updated by Python at " + datetime.now().strftime("%H:%M:%S")},
            ])

        def copy_value(event):
            app.set_clipboard_text(f"Count = {counter}")
            document.set_text("status", "Copied to the system clipboard.")

        document.on("click", increase, element="increase")
        document.on("click", copy_value, element="copy")
        app.run()


if __name__ == "__main__":
    main()
