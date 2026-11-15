"""A slow event callback plus an event burst: are lifecycle events (window closed) dropped?"""
import sys, time, threading
from ui4me_core import Application, StaleDocumentError, RpcError
app = Application.start(name="e6", max_windows=4)
w1 = app.open_html("<input id=a><input id=b>", width=300, height=200)
w2 = app.open_html("<p id=p>second</p>", width=300, height=200)
doc = w1.document
slow_once = threading.Event()
def on_focus(e):
    if not slow_once.is_set():
        slow_once.set()
        time.sleep(3)
doc.on("focus", on_focus)
closed_seen = []
app.on_window_event(lambda p: p.get("type") == "closed" and closed_seen.append(p["window_id"]))
app.ready()
for i in range(40):
    doc.focus("a"); doc.focus("b")
w2.request_close()   # default policy allows; not last window
time.sleep(0.3)
print("dropped so far:", app.rpc.dropped_notifications, flush=True)
time.sleep(4)
print("closed events seen by listener:", closed_seen, flush=True)
print("w2.closed =", w2.closed, "| host windows:", app.rpc.call("host.diagnostics")["windows"], flush=True)
try:
    w2.document.set_text("p", "after close")
except StaleDocumentError as e:
    print("StaleDocumentError", e, flush=True)
except RpcError as e:
    print("RpcError instead of StaleDocumentError:", e, flush=True)
print("app._windows keys:", list(app._windows), flush=True)
app.request_quit()
app.run()
