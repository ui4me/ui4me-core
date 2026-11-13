"""Windows completion suite part 2: C06 drops (Explorer, WordPad, .NET drag source), C11 open/reveal."""
import sys, threading, time
import wl
import cw as c
from wl import P, key, ex, uia
import c_suite_lib as L

C = c.C
only = set(sys.argv[1:])

if not only or "C06" in only:
    P("## C06")
    L.restart()
    ex(r'''
import os
d = r"F:\ui4me\lab\dnd"
open(os.path.join(d, "source.txt"), "w", encoding="utf-8-sig").write("drag this text: Příliš žluťoučký")
w.spawn(["explorer.exe", d])
w.spawn([r"C:\Program Files\Windows NT\Accessories\wordpad.exe", os.path.join(d, "source.txt")])''')
    time.sleep(5)
    ex(f'''
for t, r in (("dnd", (10, 40, 560, 420)), ("source - WordPad", (10, 500, 560, 400))):
    x = w.window(t); w.move(x["hwnd"], *r)
x = w.window("{C}"); w.move(x["hwnd"], 580, 20, 1016, 960)''')
    time.sleep(1.5)

    def zone(z, top=False, cap=C, win="main"):
        r = c.rect(z, win); w_ = wl.win(cap)
        return w_["cx"] + r["x"] + 40, w_["cy"] + r["y"] + (12 if top else r["height"] / 2)

    def src_file():
        it = uia('''
e = uia.window(title="dnd")
v = uia.find(e, name="Items View")
r = uia.find(v, name="žluťoučký 日本", control="ListItem").CurrentBoundingRectangle
vr = v.CurrentBoundingRectangle
result = [max(r.left, vr.left) + 40, (r.top + r.bottom) / 2]''')
        return it

    def src_text():
        wl.activate("source - WordPad"); key("ctrl+a"); time.sleep(0.3)
        d = uia('''
e = uia.window(title="source - WordPad")
rects = list(uia.pattern(uia.find(e, control="Document"), "Text").DocumentRange.GetBoundingRectangles())
result = rects[:4]''')
        return d[0] + min(60, d[2] / 2), d[1] + d[3] / 2

    def drag(src, dst):
        sx, sy = src
        wl.move(sx, sy); time.sleep(0.2); wl.xdo("mousedown", 1); time.sleep(0.6)
        for i in range(1, 31):
            wl.move(sx + (dst[0] - sx) * i / 30, sy + (dst[1] - sy) * i / 30); time.sleep(0.03)
        time.sleep(0.4); wl.xdo("mouseup", 1); time.sleep(0.2)

    def summ(t):
        o = []
        for e in c.doc_events(t, {"drop", "drag_feedback"}):
            if e["type"] == "drop":
                o.append(("DROP", e.get("element"), str(e.get("paths") or e.get("text"))[:70]))
            elif e.get("element"):
                o.append(("fb", e.get("element"), e.get("accepted"), e.get("kind")))
        return o
    c.op("scroll_into_view", element="s-c06", block="start"); time.sleep(1)
    for z, top in (("dz-files", True), ("dz-none", False), ("dz-text", False), ("dz-both", True), ("dz-inner-text", False), ("dz-disabled", False), ("dz-inert", False), ("dz-plain", False)):
        wl.activate("dnd"); sf = src_file(); t = time.time(); drag(sf, zone(z, top)); time.sleep(0.8); P("FILE ->", z, summ(t), "| src", [round(v) for v in sf])
    for z, top in (("dz-text", False), ("dz-files", True), ("dz-inner-text", False), ("dz-both", True), ("dz-none", False), ("dz-plain", False)):
        s = src_text(); t = time.time(); drag(s, zone(z, top)); time.sleep(0.8); P("TEXT ->", z, summ(t))
    P("WordPad source text after text drags", repr(uia('''
e = uia.window(title="source - WordPad")
result = uia.pattern(uia.find(e, control="Document"), "Text").DocumentRange.GetText(-1)''')))

    def net(mode, z, top=False, action=None, cap=C, win="main", settle=2.5):
        ex(rf'''import subprocess; subprocess.Popen(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-STA", "-File", r"F:\ui4me\lab\dragsrc.ps1", "-Mode", "{mode}"], creationflags=0x08000000)''')
        try:
            x = wl.win(f"DragSrc {mode}", 30); ex(f"w.move({x['hwnd']}, 100, 300, 300, 200)"); time.sleep(0.8)
            x = wl.win(f"DragSrc {mode}"); t = time.time()
            drag((x["cx"] + 150, x["cy"] + 80), zone(z, top, cap, win))
            a = action() if action else None; time.sleep(settle)
            return {"action": a, "drops": [(e.get("element"), len(e.get("paths") or []), (e.get("paths") or [e.get("text")])[:1]) for e in c.doc_events(t, {"drop"})],
                    "errors": [e["data"].get("message") for e in c.events(t, "document.error")]}
        finally:
            for x in wl.wins(f"DragSrc {mode}"):
                ex(f"w.post({x['hwnd']}, 0x10)")
            time.sleep(0.5)
    P("mixed -> dz-text", net("mixed", "dz-text")); P("mixed -> dz-files", net("mixed", "dz-files", True))
    P("https -> dz-text", net("https", "dz-text")); P("https -> dz-files", net("https", "dz-files", True))
    P("1.1 MB text -> dz-text", net("bigtext", "dz-text"))
    P("1024 files", net("files1024", "dz-files", True)); P("1025 files", net("files1025", "dz-files", True))
    P("odd paths (missing, \\\\?\\ prefix, directory)", net("oddpaths", "dz-files", True))
    P("delayed control", net("delayed", "dz-text", settle=5))
    P("delayed + receiver removed/recreated", net("delayed", "dz-text", action=lambda: c.op("node_batch", ops=[{"op": "remove_element", "node": c.op("node_of", element="dz-text")["result"]["node"]}]).get("ok") and c.op("insert_html", element="s-c06", position="beforeend", html='<div id="dz-text" class="zone" data-ui4me-drop="text">recreated</div>').get("ok"), settle=5))
    P("delayed + policy -> none", net("delayed", "dz-text", action=lambda: c.op("set_attribute", element="dz-text", name="data-ui4me-drop", value="none").get("ok"), settle=5))
    c.op("set_attribute", element="dz-text", name="data-ui4me-drop", value="text")
    P("delayed + document replaced", net("delayed", "dz-text", action=lambda: c.req({"kind": "load", "page": "fixture.html"}).get("ok"), settle=5))
    c.req({"kind": "open", "name": "w2", "page": "fixture.html", "title": "UI4ME C-lab 2", "width": 900, "height": 700}); x2 = wl.win("C-lab 2", 20)
    ex(f"w.move({x2['hwnd']}, 580, 20, 1000, 900)"); time.sleep(1)
    c.op("scroll_into_view", window="w2", element="s-c06", block="start"); time.sleep(1)
    P("delayed + target window closed", net("delayed", "dz-text", action=lambda: ex(f"w.post({x2['hwnd']}, 0x10)") or "closed", cap="C-lab 2", win="w2", settle=5), "| windows", [x["title"] for x in wl.wins("C-lab")])
    ex('''
for t in ("source - WordPad", "dnd"):
    for x in w.windows(t): w.post(x["hwnd"], 0x10)''')

if not only or "C11" in only:
    P("## C11")
    L.restart()
    ex(r'''
import os
d = r"F:\ui4me\lab\files\složka s mezerou"
os.makedirs(d, exist_ok=True)
open(os.path.join(d, "soubor č.txt"), "w", encoding="utf-8").write("external open")
open(r"F:\ui4me\lab\files\blob-neznámý.ui4mexyz", "wb").write(b"\0unknown")''')
    base = r"F:\ui4me\lab\files"

    def ext():
        return [(x["title"][:50], x["class"]) for x in wl.wins() if x["class"] in ("Notepad", "CabinetWClass", "Chrome_WidgetWin_1") or "Notepad" in x["title"]]
    before = ext()
    P("open file", c.call("desktop.open_external", target=base + r"\složka s mezerou\soubor č.txt", window="main")); time.sleep(4); P("  ", ext()); wl.wshot("final\\C11-open-file.png")
    P("open dir", c.call("desktop.open_external", target=base + r"\složka s mezerou", window="main")); time.sleep(4); P("  ", ext())
    P("open https", c.call("desktop.open_external", target="https://example.org/ui4me-c11", window="main")); time.sleep(5); P("  ", ext(), "| default browser processes", ex("import psutil; result = sorted(set(p.name() for p in psutil.process_iter(['name']) if (p.info['name'] or '').lower() in ('msedge.exe', 'chrome.exe')))"))
    ex("import subprocess\nfor n in ('msedge.exe', 'chrome.exe'): subprocess.run(['taskkill', '/IM', n, '/F'], capture_output=True)")
    for t in ("javascript:alert(1)", "ftp://example.org/x", "data:text/plain,hi", "file:///C:/Windows/System32/calc.exe", r"C:\Windows\System32\calc.exe", r"\\server\share\x"):
        r = c.call("desktop.open_external", target=t); P("deny/other", t, r.get("code"), (r.get("error") or "")[:90], r.get("result"))
    for p in (r"složka s mezerou\soubor č.txt", r"složka s mezerou"):
        P("reveal", p, c.call("desktop.reveal_file", path=base + "\\" + p, window="main").get("result")); time.sleep(3)
    P("  explorer windows", [(x["title"], x["class"]) for x in wl.wins() if x["class"] == "CabinetWClass"])
    sel = uia('''
a = uia.A(); out = []
root = a.GetRootElement()
arr = root.FindAll(uia.U.TreeScope_Children, a.CreatePropertyCondition(uia.U.UIA_ClassNamePropertyId, "CabinetWClass"))
for i in range(arr.Length):
    e = arr.GetElement(i)
    items = [d for d in uia.dump(e, depth=14, max_nodes=400) if d["type"] == "ListItem" and d.get("selected")]
    out.append((e.CurrentName, [d["name"] for d in items]))
result = out''')
    P("  reveal selection in Explorer", sel); wl.wshot("final\\C11-reveal.png")
    ex('''
for x in w.windows():
    if x["class"] in ("CabinetWClass", "Notepad") or ("example.org" in x["title"]):
        w.post(x["hwnd"], 0x10)''')
    time.sleep(2)
    target = base + r"\blob-neznámý.ui4mexyz"; res = {}

    def call(k, **kw):
        try:
            res[k] = c.call("desktop.open_external", target=target, **kw)
        except Exception as e:
            res[k] = repr(e)
    def chooser():
        return ex('''
import psutil
ow = [p.pid for p in psutil.process_iter(["name"]) if (p.info["name"] or "").lower() == "openwith.exe"]
result = [("OpenWith.exe", pid, [(x["title"], x["class"], x["owner"]) for x in w.windows(pid=pid)]) for pid in ow]''')
    th = threading.Thread(target=call, args=("esc",), kwargs={"window": "main"}); th.start(); time.sleep(4)
    P("chooser", chooser(), "| all new windows", [(x["title"][:40], x["class"]) for x in wl.wins() if x["title"] and "UI4ME" not in x["title"]][:6]); wl.wshot("final\\C11-chooser.png")
    key("Escape"); th.join(20); time.sleep(1); P("Escape", res.get("esc"), chooser())
    c.req({"kind": "open", "name": "w2", "page": "a.html", "title": "UI4ME C-lab 2", "width": 500, "height": 300}); x2 = wl.win("C-lab 2", 20); time.sleep(1)
    th = threading.Thread(target=call, args=("parent",), kwargs={"window": "w2"}); th.start(); time.sleep(4); P("chooser for w2", chooser())
    ex(f"w.post({x2['hwnd']}, 0x10)"); th.join(20); time.sleep(1); P("parent closed", res.get("parent"), chooser())
    key("Escape"); time.sleep(1)
    hp = wl.win(C)["pid"]; th = threading.Thread(target=call, args=("quit",), kwargs={"window": "main"}); th.start(); time.sleep(4); P("chooser pending", chooser())
    P("request_quit", c.call("app.request_quit")); th.join(20); time.sleep(2)
    P("pending result", res.get("quit"), "| chooser", chooser(), "| host alive", ex(f"import psutil; result = psutil.pid_exists({hp})"))
    key("Escape")
P("DONE")
