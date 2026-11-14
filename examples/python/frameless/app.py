"""Frameless window: HTML title bar, native drag/resize regions, window controls.

Run it directly:
  python app.py          (or: uv run app.py)
Or through the manifest:  ui4me-host run --app app.toml
"""
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Python Frameless Window", app_id="org.ui4me.PythonFrameless") as app:
        window = app.open_file(HERE / "index.html", title="Python Custom Chrome", width=900, height=590,
                               decorations=False, min_width=480, min_height=320)
        document = window.document

        def minimize(event):
            window.set(minimized=True)

        def toggle_maximize(event):
            # window.state() separates requested from observed state; toggle
            # what the desktop actually did.
            observed = window.state().get("observed") or {}
            window.set(maximized=not observed.get("maximized"))

        def close(event):
            window.request_close()

        document.on("click", minimize, element="minimize")
        document.on("click", toggle_maximize, element="maximize")
        document.on("click", close, element="close")
        app.run()


if __name__ == "__main__":
    main()
