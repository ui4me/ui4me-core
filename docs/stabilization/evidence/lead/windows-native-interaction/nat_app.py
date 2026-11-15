"""Native interaction probe app (F-08, F-17, F-17b). Logs document events as JSON lines to NAT_LOG."""
import json, os, threading
from ui4me_core import Application
LOG = os.environ["NAT_LOG"]
def log(obj):
    with open(LOG, "a", encoding="utf-8") as f: f.write(json.dumps(obj, ensure_ascii=False) + "\n")
app = Application.start(name="natprobe", app_id="org.ui4me.test.NatProbe" + os.environ.get("NAT_TAG", "x"), instance="multiple")
html = """<style>body{font:16px sans-serif;margin:8px} button,a,input{margin:4px}</style>
<p><button id="pr1">Press here</button> <button id="pr2">Release here</button> <button id="ib" style="padding:4px 40px 4px 4px"><span id="icon" style="display:inline-block;width:24px;height:16px;background:#c33"></span> Icon button</button></p>
<input id="name" value="Привет мир" aria-label="Name field"> <a id="lnk" href="https://example.invalid/x">Help link</a>
<div style="height:2400px;background:linear-gradient(#eef,#fee)">filler</div>
<button id="far">Far button</button>"""
w = app.open_html(html, title="natprobe", width=640, height=420)
d = w.document
for t in ("click", "link_activated", "keydown", "focusin"):
    d.on(t, lambda e, t=t: log({"t": t, "el": e.get("element"), "key": e.get("key"), "code": e.get("code"),
                                "ctrl": e.get("ctrl_key", e.get("ctrlKey")), "href": e.get("href")}))
w.on("closed", lambda e: log({"t": "closed"})) if hasattr(w, "on") else None
app.ready()
log({"t": "ready"})
app.run()
log({"t": "exit"})
