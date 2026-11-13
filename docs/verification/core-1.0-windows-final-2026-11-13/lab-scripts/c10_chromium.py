import json, os, sys
from pathlib import Path
from playwright.sync_api import sync_playwright
ids = ["fb-min", "fb-max", "bal", "bal-end", "bw", "bw-end", "sel", "rz", "sh", "liga-off", "liga-on", "png", "png-end"]
out = {}
url = Path(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-core1-clab\c10.html")).as_uri()
with sync_playwright() as p:
    b = p.chromium.launch(); pg = b.new_page()
    for w in (360, 640, 900):
        pg.set_viewport_size({"width": w, "height": 900}); pg.goto(url); pg.wait_for_timeout(500)
        out[w] = {i: pg.evaluate("id=>{const r=document.getElementById(id).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}}", i) for i in ids}
        pg.screenshot(path=sys.argv[1] + f"/chromium-{w}.png")
    out["version"] = b.version; b.close()
print(json.dumps(out))
