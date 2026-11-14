"""The smallest UI4ME app: one window, one button, one Python callback.

Run it:  python hello.py      (or: uv run hello.py)
"""
from ui4me_core import Application

PAGE = """
<main style="font-family: sans-serif; padding: 24px">
  <h1>Hello from Python</h1>
  <button id="hello">Say hello</button>
  <p id="message" role="status">Click the button.</p>
</main>
"""


def main():
    with Application.start(name="Hello UI4ME", app_id="org.ui4me.examples.Hello") as app:
        window = app.open_html(PAGE, title="Hello UI4ME", width=520, height=320)
        document = window.document
        clicks = 0

        def say_hello(event):
            nonlocal clicks
            clicks += 1
            document.set_text("message", f"Hello from native UI4ME! ({clicks})")

        document.on("click", say_hello, element="hello")
        app.run()


if __name__ == "__main__":
    main()
