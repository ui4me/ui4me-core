"""Independent UI4ME consumer: uses only the installed ui4me_core wheel."""
import json, os, sys, time
from pathlib import Path
import ui4me_core
from ui4me_core import Application

HERE = Path(__file__).resolve().parent
report = {"python": sys.version, "executable": sys.executable, "ui4me_core": ui4me_core.__file__,
          "host": str(ui4me_core.host_binary()), "ppid": os.getppid()}
with Application.from_environment() as app:
    window = app.open_html("<!doctype html><title>Independent consumer</title><h1 id=h>Independent UI4ME consumer</h1>"
                           "<img id=b src='ui4me-bitmap://live' alt='live'>", title="Independent UI4ME consumer", width=500, height=360)
    doc = window.document
    app.ready()
    bm = doc.create_bitmap("live", 64, 64)
    frames = 0
    for i in range(120):
        r = bm.present(bytes(((i * 2) % 256, 64, 255 - (i * 2) % 256, 255)) * 64 * 64)
        frames += 1 if r.get("presented_to_engine") else 0
        time.sleep(1 / 60)
    report["frames_presented"] = frames
    report["window_state"] = window.state().get("observed")
    time.sleep(float(os.environ.get("CONSUMER_HOLD", "8")))
    bm.close()
    (HERE / "consumer-report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    app.request_quit()
    app.run()
