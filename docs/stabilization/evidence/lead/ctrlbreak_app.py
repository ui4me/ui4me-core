import os, sys, time
from ui4me_core import Application
MARK = os.environ.get("CB_MARK", "cb-mark.txt")
def note(s):
    with open(MARK, "a", encoding="utf-8") as f: f.write(f"{time.time():.3f} {os.getpid()} {s}\n")
with Application.start(name="Ctrl break", app_id="org.ui4me.stab.CtrlBreak", instance="multiple") as app:
    app.open_html("<p>idle</p>", title="cb")
    app.on_shutdown(lambda: note("BACKEND_SHUTDOWN_DELIVERED"))
    note("READY"); print("READY", flush=True)
    app.run()
note("RUN_RETURNED")
