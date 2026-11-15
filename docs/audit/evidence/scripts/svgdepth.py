import sys, time, threading, base64
from ui4me_core import Application
D = int(sys.argv[1])
svg = '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">' + "<g>" * D + '<rect width="10" height="10" fill="red"/>' + "</g>" * D + "</svg>"
url = "data:image/svg+xml;base64," + base64.b64encode(svg.encode()).decode()
def main():
    with Application.start(name="SvgDepth", app_id="org.ui4me.audit.SvgDepth", instance="multiple") as app:
        w = app.open_html("<p id=s>start</p><div id=box></div>", title="svg", width=400, height=300)
        d = w.document
        def body():
            try:
                time.sleep(0.5)
                d.insert_html("box", f'<img id=im src="{url}" width=40 height=40>')
                time.sleep(2.0)
                print("TEXT", d.get_text("s"), flush=True)
                print("SVG_PASS", D, flush=True)
            except Exception as e:
                print("SVG_FAIL", D, type(e).__name__, str(e)[:200], flush=True)
            finally:
                try: app.request_quit()
                except Exception: pass
        threading.Thread(target=body, daemon=True).start()
        app.run()
if __name__ == "__main__": main()
