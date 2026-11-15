"""Which DOM events can headless commands generate? Count all document.event types."""
import collections, sys, time, threading
from ui4me_core import Application
app = Application.start(name="e1")
counts = collections.Counter()
orig = app._document_event
def spy(params):
    counts[params.get("event", {}).get("type")] += 1
    orig(params)
app.rpc.on_notification("document.event", spy)
win = app.open_html("""<html><body><form id=f><input id=a><input id=b><button id=s type=submit>S</button></form>
<dialog id=d>hi</dialog><div id=sc style="height:50px;overflow:auto"><div style="height:500px">x</div></div></body></html>""", width=300, height=200)
doc = win.document
doc.on("submit", lambda e: None)
t0 = time.time()
for i in range(50):
    doc.focus("a"); doc.focus("b")
doc.request_submit("f")
doc.show_dialog("d"); doc.close_dialog("d")
doc.scroll_to(element="sc", y=100)
doc.set_value("a", "hello")
time.sleep(0.5)
print("events:", dict(counts), "elapsed", round(time.time()-t0, 2), "dropped", app.rpc.dropped_notifications, flush=True)
app.request_quit()
app.run()
print("run returned", flush=True)
