import sys, time, threading
from ui4me_core import Application
D = int(sys.argv[1])
html = "<div>" * D + "<span id=leaf>deep</span>" + "</div>" * D
def main():
    with Application.start(name="Depth", app_id="org.ui4me.audit.Depth", instance="multiple") as app:
        w = app.open_html("<p id=s>start</p>", title="depth", width=400, height=300)
        d = w.document
        def body():
            try:
                time.sleep(0.5)
                d = w.load_html(html)
                time.sleep(1.5)
                print("TEXT", d.get_text("leaf"), "RECT", d.command("get_rect", element="leaf") if True else "", flush=True)
                print("DEPTH_PASS", D, flush=True)
            except Exception as e:
                print("DEPTH_FAIL", D, type(e).__name__, str(e)[:200], flush=True)
            finally:
                try: app.request_quit()
                except Exception: pass
        threading.Thread(target=body, daemon=True).start()
        app.run()
if __name__ == "__main__": main()
