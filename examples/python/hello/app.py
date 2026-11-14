"""Hello: the smallest complete UI4ME app in Python.

Run it directly:
  python app.py          (Linux or Windows; or: uv run app.py)

Application.start() starts the native window host, which runs this script a
second time as its backend. Everything above that call runs twice, so keep
real work below it, inside main().
"""
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent


def main():
    with Application.start(name="Hello UI4ME", app_id="org.ui4me.PythonHello") as app:
        window = app.open_file(HERE / "index.html", title="Hello UI4ME", width=560, height=360)
        document = window.document
        clicks = 0

        def greet(event):
            nonlocal clicks
            clicks += 1
            name = document.get_value("name").strip() or "world"
            # Text is never parsed as HTML, so user input is safe here.
            document.set_text("message", f"Hello, {name}! (button pressed {clicks} time(s))")

        document.on("click", greet, element="greet")
        app.run()


if __name__ == "__main__":
    main()
