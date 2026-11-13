import json, time
import wl, cw as c
from wl import P, ex
import c_suite_lib as L
ids = ["fb-min", "fb-max", "bal", "bal-end", "bw", "bw-end", "sel", "rz", "sh", "liga-off", "liga-on", "png", "png-end"]
r = ex(r'''
import os
os.makedirs(r"F:\ui4me\lab\out\final\c10", exist_ok=True)
result = w.run([r"F:\ui4me\qa-venv\Scripts\python.exe", r"F:\ui4me\lab\c10_chromium.py", r"F:\ui4me\lab\out\final\c10"], env={"PLAYWRIGHT_BROWSERS_PATH": r"F:\ui4me\qa-browsers"}, timeout=300)''')
ch = json.loads(r["out"]); P("Chromium", ch["version"])
L.restart()
c.req({"kind": "load", "page": "c10.html"}); c.call("window.set", window="main", min_width=None, min_height=None, max_width=None, max_height=None)
ui = {}; worst = 0; worst_at = None
for wd in (360, 640, 900):
    c.call("window.set", window="main", width=wd, height=900); time.sleep(1.5)
    ui[wd] = {i: c.op("get_rect", element=i)["result"]["rect"] for i in ids}
    x = wl.win(c.C); ex(f"w.grab(({x['cx']}, {x['cy']}, {x['cx'] + wd}, {x['cy'] + min(900, x['ch'])})).save(r'F:\\ui4me\\lab\\out\\final\\c10\\ui4me-{wd}.png')")
    for i in ids:
        d = max(abs(ui[wd][i][k] - ch[str(wd)][i][k]) for k in ("x", "y", "width", "height"))
        if d > worst:
            worst, worst_at = d, (wd, i)
open("c10-ui4me.json", "w").write(json.dumps(ui, indent=1)); open("c10-chromium.json", "w").write(json.dumps(ch, indent=1))
P("max |geometry delta| over 13 markers x 3 widths:", round(worst, 2), "px at", worst_at)
cmp = ex(r'''
from PIL import Image, ImageChops
u = Image.open(r"F:\ui4me\lab\out\final\c10\ui4me-640.png").convert("RGB"); cc = Image.open(r"F:\ui4me\lab\out\final\c10\chromium-640.png").convert("RGB")
reg = lambda im, x, y, w, h: im.crop((x, y, x + w, y + h))
out = {}
for nm, b in {"png glyph": (8, 490, 32, 32), "inset shadow": (8, 318, 120, 50), "liga-off": (8, 371, 243, 55)}.items():
    out[nm] = [sum(1 for p in ImageChops.difference(reg(u, *b), reg(cc, *b)).getdata() if max(p) > 48), b[2] * b[3]]
out["PNG glyph red pixels"] = sum(1 for p in reg(u, 8, 490, 32, 32).getdata() if p[0] > 180 and p[1] < 80 and p[2] < 80)
out["liga default vs override differ"] = ImageChops.difference(reg(u, 8, 371, 243, 55), reg(u, 8, 427, 243, 55)).getbbox() is not None
result = out''')
P("pixel regions (differing>48 / total):", cmp)
c.call("window.set", window="main", width=1000, height=760, min_width=480, min_height=360, max_width=1400, max_height=1000)
P("DONE")
