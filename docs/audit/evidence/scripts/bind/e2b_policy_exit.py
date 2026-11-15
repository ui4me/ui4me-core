"""sys.exit() inside a close policy callback."""
import sys, time, threading
from ui4me_core import Application
app = Application.start(name="e2b")
win = app.open_html("<p id=a>x</p>", width=300, height=200)
calls = []
def before_close():
    calls.append(time.time())
    print("before_close -> sys.exit()", flush=True)
    sys.exit(0)
win.on_before_close(before_close)
app.on_window_event(lambda p: print("window.event", p.get("type"), flush=True))
app.ready()
win.request_close()
time.sleep(1.0)
print("diag phase:", app.rpc.call("host.diagnostics")["phase"], "threads", len([t for t in threading.enumerate() if t.name.startswith("ui4me-callback")]), flush=True)
win.request_close()
time.sleep(1.0)
print("calls", len(calls), "closed?", win.closed, flush=True)
try:
    app.request_quit()
    print("request_quit accepted", flush=True)
except Exception as e:
    print("request_quit failed:", e, flush=True)
time.sleep(1.0)
print("shutdown requested?", app.rpc.shutdown_requested.is_set(), flush=True)
app.rpc.call("app.force_quit")
time.sleep(1)
