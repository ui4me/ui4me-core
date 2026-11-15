"""Try to provoke issue #1 (host outbound queue overflow) on Linux with a style-change burst."""
import collections, sys, time, threading
from ui4me_core import Application
N = int(sys.argv[1])
app = Application.start(name="e13")
counts = collections.Counter()
orig = app._document_event
def spy(params):
    counts[params.get("event", {}).get("type")] += 1
    orig(params)
app.rpc.on_notification("document.event", spy)
items = "".join(f'<span class=t id=i{i}>{i}</span>' for i in range(N))
css = ".t{color:black;transition:color 50ms linear} @media (prefers-color-scheme: dark){.t{color:white}}"
win = app.open_html(f"<style>{css}</style><div id=root>{items}</div>", width=400, height=300)
doc = win.document
app.ready()
for scheme in ("dark", "light", "dark"):
    try:
        doc.set_environment(color_scheme=scheme)
    except Exception as e:
        print("set_environment failed:", type(e).__name__, e, flush=True); break
    time.sleep(1.0)
print("events:", dict(counts), "dropped(client):", app.rpc.dropped_notifications, "closed:", app.rpc.closed, flush=True)
if not app.rpc.closed:
    app.request_quit(); app.run()
