"""Windows smoke for the engine thread move (F-01): clipboard, native file dialog, UIA tree, quit."""
import threading, time
from ui4me_core import Application
html = """<h1>Smoke</h1><button id=ok>Save changes</button><input id=name aria-label="User name" value="x">
<a id=lnk href="https://example.invalid/">Help link</a><p id=s>ready</p>"""
def main():
    with Application.start(name="Win smoke", app_id="org.ui4me.stab.WinSmoke", instance="multiple") as app:
        w = app.open_html(html, title="winsmoke", width=600, height=400)
        def body():
            try:
                time.sleep(1.0)
                app.set_clipboard_text("ui4me-smoke-\u00e9\u4e2d")
                print("CLIPBOARD", app.clipboard_text() == "ui4me-smoke-\u00e9\u4e2d", flush=True)
                print("DIALOG_OPENING", flush=True)
                result = w.open_files(title="UI4ME smoke open", wait=True)
                print("DIALOG_RESULT", result, flush=True)
                time.sleep(4.0)  # UIA tree dump happens now
                print("SMOKE_PASS", flush=True)
            except Exception as e:
                print("SMOKE_FAIL", type(e).__name__, e, flush=True)
            finally:
                try: app.request_quit()
                except Exception: pass
        threading.Thread(target=body, daemon=True).start()
        app.run()
if __name__ == "__main__":
    main()
