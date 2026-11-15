import threading
import time

from ui4me_core import Application, StaleDocumentError

HTML = """
<button id="start">Start</button>
<progress id="bar" max="100" value="0"></progress>
<p id="status" role="status">Idle</p>
"""


def main():
    with Application.start(name="Worker") as app:
        window = app.open_html(HTML, title="Worker", width=400, height=200)
        document = window.document
        running = threading.Event()

        def work():
            # The thread may call document methods; it reports progress directly.
            try:
                for step in range(1, 101):
                    time.sleep(0.05)  # stand-in for real work
                    document.set_attribute("bar", "value", str(step))
                document.set_text("status", "Done")
            except StaleDocumentError:
                return  # The window closed or the page changed; stop quietly.
            except Exception as error:
                document.set_text("status", f"Stopped: {error}")
            finally:
                running.clear()
            document.remove_attribute("start", "disabled")

        def start(event):
            if running.is_set():
                return  # already running
            running.set()
            document.set_attribute("start", "disabled", "")
            document.set_text("status", "Working...")
            threading.Thread(target=work, daemon=True).start()

        document.on("click", start, element="start")
        app.ready()
        start(None)  # simulate the click
        threading.Timer(1.0, window.request_close).start()  # user closes the window mid-work
        app.run()


if __name__ == "__main__":
    main()
