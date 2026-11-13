"""Independent consumer: uses only the installed ui4me_core package (no repo files)."""
import json, os, sys, time
from pathlib import Path
import ui4me_core
from ui4me_core import Application

OUT = Path(os.environ.get("CONSUMER_REPORT", "/tmp/consumer-report.json"))
HTML = """<!doctype html><html><body style="margin:0;font:16px sans-serif;background:#eef">
<h1 id="t">Consumer</h1><img id="v" src="ui4me-bitmap://frame"><p id="s">waiting</p></body></html>"""

def main():
    report = {"ui4me_core": ui4me_core.__version__, "module": ui4me_core.__file__,
              "host_binary": str(ui4me_core.host_binary()), "python": sys.version.split()[0]}
    with Application.from_environment() as app:
        w = app.open_html(HTML, title="UI4ME wheel consumer", width=420, height=300, decorations=False)
        d = w.document
        bm = d.create_bitmap("frame", 64, 32)
        for i in range(120):
            bm.present(bytes([i % 256, 128, 255 - i % 256, 255]) * (64 * 32))
        d.set_text("s", "presented %d frames" % bm._sequence)
        report.update(frames=bm._sequence, status=d.get_text("s"),
                      state=w.state()["observed"], capabilities=sorted(k for k, v in app.capabilities().items() if v is True)[:20])
        bm.close()
        report["result"] = "PASS"
        OUT.write_text(json.dumps(report, indent=1))
        import threading
        threading.Timer(2.0, app.request_quit).start()
        app.run()

if __name__ == "__main__":
    main()
