"""sys.exit() inside an event callback: does the single event worker survive?"""
import sys, time, threading
from ui4me_core import Application
app = Application.start(name="e2")
win = app.open_html("<input id=a><input id=b>", width=300, height=200)
doc = win.document
seen = []
def on_focus(e):
    seen.append(e["element"])
    if e["element"] == "a":
        print("callback calling sys.exit(0)", flush=True)
        sys.exit(0)
doc.on("focus", on_focus)
app.ready()
doc.focus("a")
time.sleep(0.5)
doc.focus("b")
time.sleep(0.5)
print("seen after two focus events:", seen, flush=True)
print("threads:", sorted(t.name for t in threading.enumerate()), flush=True)
def quit_later():
    time.sleep(0.2); app.request_quit()
threading.Thread(target=quit_later, daemon=True).start()
t0 = time.time()
app.run()
print("run returned after", round(time.time() - t0, 2), "s", flush=True)
