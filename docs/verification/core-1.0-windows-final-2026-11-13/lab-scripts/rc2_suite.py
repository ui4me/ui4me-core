"""Windows RC2 final-binary suite on the installed C-lab: frameless chrome, buttons, live bitmaps,
bundles, fonts, veto, overload, teardown. Real RDP input; Win32/psutil observation."""
import json, sys, time
import wl
import cw as c
from wl import P, key, ex
import c_suite_lib as L

T = "UI4ME RC2 lab"
only = set(sys.argv[1:])
STRESS = open("stress_win.py").read()


def H(t):
    print("\n### " + t, flush=True)


def lab(code):
    return c.code(code)


def win():
    return wl.win(T)


def place(x=300, y=100, w=900, h=640):
    ex(f"w.move({win()['hwnd']}, {x}, {y}, {w}, {h})"); time.sleep(0.6)


def g():
    x = win(); return tuple(x["rect"])


def events(t, kind="event"):
    return [e for e in wl.ex(f"result = w.lab_events({t!r}, {kind!r})")]


def temp_files():
    return ex(r'''
import os, glob
result = sorted(os.path.basename(p) for p in glob.glob(os.path.join(os.environ["TEMP"], "ui4me-bitmap-*")))''')


def proc():
    return ex(f"result = w.mem({hp})")


L.restart()
lab(r'''
import json as _json
from pathlib import Path as _P
lw = app.open_file(HERE / "lab.html", title="UI4ME RC2 lab", width=900, height=640, decorations=False, min_width=320, min_height=240)
windows["rc2"] = lw
doc = lw.document
def _wire(d):
    for ev in ("pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"):
        d.on(ev, lambda e, ev=ev: log("event", {"type": ev, "e": e}))
    d.on("click", lambda _: lw.set(minimized=True), element="min")
    d.on("click", lambda _: lw.set(maximized=not bool((lw.state().get("observed") or {}).get("maximized"))), element="max")
    d.on("click", lambda _: lw.request_close(), element="cls")
_wire(doc)
state["veto"] = False
def _before_close():
    log("before_close", {"veto": state["veto"]})
    return not state["veto"]
lw.on_before_close(_before_close)
state["doc"] = doc; state["lw"] = lw; state["wire"] = _wire
''')
x = wl.win(T, 20)
hp = x["pid"]
bp = ex(f"import psutil; result = [p.pid for p in psutil.Process({hp}).children() if p.name().lower().startswith('python')][0]")
P("host", hp, ex(f"result = w.info({x['hwnd']})['class']"), "sha256", ex(f"import psutil; result = w.sha256(psutil.Process({hp}).exe())"), "backend", bp)
lab("doc = state['doc']\n" + STRESS)
c.code("doc = state['doc']; import zlib\ndoc.register_resources('lab', {'pic.png': (state['png'](200, 100, (255, 0, 0, 255)), 'image/png')})")
P("frameless styles", hex(x["style"]), "caption bit", bool(x["style"] & 0x00C00000) == 0x00C00000)

if not only or "chrome" in only:
    H("8 native resize directions")
    D = 60
    cases = {"n": (lambda X, Y, W, Hh: (X + W / 2, Y + 2), (0, -D)), "s": (lambda X, Y, W, Hh: (X + W / 2, Y + Hh - 3), (0, D)),
             "e": (lambda X, Y, W, Hh: (X + W - 3, Y + Hh / 2), (D, 0)), "w": (lambda X, Y, W, Hh: (X + 2, Y + Hh / 2), (-D, 0)),
             "ne": (lambda X, Y, W, Hh: (X + W - 4, Y + 3), (D, -D)), "nw": (lambda X, Y, W, Hh: (X + 3, Y + 3), (-D, -D)),
             "se": (lambda X, Y, W, Hh: (X + W - 4, Y + Hh - 4), (D, D)), "sw": (lambda X, Y, W, Hh: (X + 3, Y + Hh - 4), (-D, D))}
    allok = True
    for name, (pos, (dx, dy)) in cases.items():
        place(); X, Y, W, Hh = g(); px, py = pos(X, Y, W, Hh)
        wl.drag(px, py, px + dx, py + dy, steps=25, dt=0.02); time.sleep(0.6); X2, Y2, W2, H2 = g()
        ew = W + (dx if "e" in name else -dx if "w" in name else 0); eh = Hh + (dy if "s" in name else -dy if "n" in name else 0)
        exx = X + (dx if "w" in name else 0); ey = Y + (dy if "n" in name else 0)
        ok = max(abs(W2 - ew), abs(H2 - eh), abs(X2 - exx), abs(Y2 - ey)) <= 2; allok &= ok
        P(f"{name:3} {(X, Y, W, Hh)} -> {(X2, Y2, W2, H2)} expected {(exx, ey, ew, eh)} {'PASS' if ok else 'FAIL'}")
    P("ALL 8 DIRECTIONS", "PASS" if allok else "FAIL")

    H("title drag, double-click, buttons, right/middle, title input")
    place(); X, Y, W, Hh = g()
    wl.drag(X + 150, Y + 20, X - 50, Y + 170, steps=30); time.sleep(0.6); X2, Y2, _, _ = g(); P("title drag moved", (X2 - X, Y2 - Y))
    wl.dblclick(X2 + 150, Y2 + 20); time.sleep(1); P("dblclick -> maximized", win()["zoomed"], g())
    X3, Y3, _, _ = g(); wl.dblclick(X3 + 150, Y3 + 20); time.sleep(1); P("dblclick -> maximized", win()["zoomed"], g())
    place(); X, Y, W, Hh = g()
    pad = c.screen("pad", cap=T, window="rc2")
    for b in ("left", "right", "middle"):
        t = time.time(); wl.click(*pad, b); time.sleep(0.5)
        P(b, [(e["data"]["type"], e["data"]["e"].get("button")) for e in events(t)])
    for b in ("right", "middle"):
        w0 = g(); wl.drag(X + 150, Y + 20, X + 350, Y + 120, button=b, steps=20); time.sleep(0.5); w1 = g()
        P(f"{b}-drag on title moved window:", w0[:2] != w1[:2]); key("Escape")
    place(); w0 = g()
    tin = c.screen("tin", 0.2, 0.5, cap=T, window="rc2")
    wl.drag(tin[0], tin[1], tin[0] + 80, tin[1] + 60, steps=20); time.sleep(0.5); w1 = g()
    P("drag inside title input moved window:", w0[:2] != w1[:2], "| selection", c.op("get_control_selection", window="rc2", element="tin").get("result"))
    for b in ("max", "min"):
        place(); w0 = g(); t = time.time(); bx = c.screen(b, cap=T, window="rc2")
        wl.drag(bx[0], bx[1], bx[0] + 5, bx[1] + 3, steps=5); time.sleep(1.0)
        st = ex(f"result = w.info({win(0)['hwnd'] if False else x['hwnd']})")
        P(f"press on '{b}' button: window moved {st['rect'][:2] != list(w0[:2])} | max {st['zoomed']} min {st['iconic']} | clicks", [(e['data']['type'], e['data']['e'].get('element')) for e in events(t) if e['data']['type'] == 'click'])
        ex(f"w.show({x['hwnd']}, 9)"); time.sleep(0.8)
    wl.wshot("final\\RC2-frameless.png")

if not only or "bitmap" in only:
    H("live bitmap: 1000+ frames x 3 sizes, newest frame, memory, transport files")
    place(100, 20, 1450, 960)

    def snap(tag):
        m = proc(); b = ex(f"result = w.mem({bp})")
        d = {"tag": tag, "host_private_kib": m["private"] // 1024, "host_handles": m["handles"], "host_threads": m["threads"],
             "backend_private_kib": b["private"] // 1024, "temp_files": temp_files()}
        P(json.dumps(d)); return d
    snap("baseline")
    for (w_, h_, n) in ((64, 64, 1501), (320, 200, 1502), (1024, 768, 1003), (1024, 768, 1000), (1024, 768, 1001)):
        m0 = proc(); t0 = time.time()
        r = lab(f"doc = state['doc']\nresult = state['stress']('b1', {w_}, {h_}, {n})")
        m1 = proc(); r["host_cpu_pct"] = round(100 * (m1["cpu"] - m0["cpu"]) / (time.time() - t0), 1)
        P(json.dumps(r))
        if w_ != 1024 or n == 1001:
            time.sleep(0.5)
            k = (n - 1) % 4
            rr = c.rect("bmp", "rc2"); xw = win()
            bx, by = xw["cx"] + rr["x"] + 1, xw["cy"] + rr["y"] + 1
            sample = ex(f"im = w.grab(({int(bx)}, {int(by)}, {int(bx + min(rr['width'] - 2, 64))}, {int(by + 4)})); result = [list(im.getpixel((xx, 1)))[:3] for xx in (0, 1, {int(min(rr['width'] - 3, 63))})]")
            exp = [[((xx * 255 // max(w_ - 1, 1) + k * 60) & 255), (k * 80) & 255, 255 - (xx * 255 // max(w_ - 1, 1))] for xx in (0, 1)]
            P(f"  newest frame k={k}: on-screen first pixels {sample[:2]} expected (scaled image) ~{exp}")
            wl.wshot(f"final\\RC2-bitmap-{w_}x{h_}.png")
        snap(f"after {w_}x{h_} x{n}")

    H("bitmap error semantics")
    P(json.dumps(lab(r'''
doc = state['doc']; lw = state['lw']
out = {}
for b in list(doc._bitmaps): b.close()
bm = doc.create_bitmap('b1', 64, 64); fr = state['frames_for'](64, 64)
base = {"window_id": lw.id, "document_id": doc.id, "bitmap_id": "b1"}
def call(m, **kw):
    try: return rpc.call(m, {**base, **kw})
    except Exception as e: return {"error": [getattr(e, "code", None), str(e)]}
bm._map[0:64*64*4] = fr[0]
out["present_seq5"] = call("bitmap.present", sequence=5, slot=0)
out["stale_seq3"] = call("bitmap.present", sequence=3, slot=1)
out["bad_slot"] = call("bitmap.present", sequence=6, slot=2)
out["zero_seq"] = call("bitmap.present", sequence=0, slot=0)
out["missing_bitmap"] = call("bitmap.present", sequence=7, slot=0, bitmap_id="nope")
out["stale_document"] = call("bitmap.present", sequence=7, slot=0, document_id="stale")
out["missing_window"] = call("bitmap.present", sequence=7, slot=0, window_id="w999")
out["duplicate_create"] = call("bitmap.create", width=8, height=8)
out["too_big"] = call("bitmap.create", bitmap_id="huge", width=4096, height=4096, slots=2)
out["bad_format"] = call("bitmap.create", bitmap_id="fmt", width=8, height=8, pixel_format="BGRA8")
out["ninth"] = [("ok" if "error" not in call("bitmap.create", bitmap_id=f"m{i}", width=8, height=8) else "limit") for i in range(8)]
for i in range(8): call("bitmap.close", bitmap_id=f"m{i}")
big = []
for i in range(3):
    try: doc.create_bitmap(f"big{i}", 2048, 2048, slots=2); big.append("ok")
    except Exception as e: big.append(str(e)[:60])
out["aggregate_64MiB"] = big
for b in list(doc._bitmaps): b.close()
out["present_after_close"] = call("bitmap.present", sequence=99, slot=0)
result = out'''), indent=0))
    time.sleep(0.5); P("transport temp files after closing all bitmaps", temp_files())

    H("overload (unthrottled 2-thread producer at 1024x768) with live resize; responsiveness")
    place(100, 40, 1300, 900)
    pad = c.screen("pad", cap=T, window="rc2")

    def clicks(n=6):
        out = []
        for i in range(n):
            wl.move(*pad); time.sleep(0.1); t = time.time(); wl.xdo("click", 1)
            end = time.time() + 5
            while time.time() < end:
                ev = [e for e in events(t) if e["data"]["type"] == "click"]
                if ev:
                    out.append(round((ev[0]["t"] - t) * 1000, 1)); break
                time.sleep(0.01)
            time.sleep(0.2)
        return sorted(out)
    P("idle click->python ms (includes RDP + SSH relay)", clicks())
    lab(r'''
import threading as _th
doc = state['doc']
bm = doc.create_bitmap('b1', 1024, 768, slots=2); fr = state['frames_for'](1024, 768)
st = {"n": 0, "drops": 0}; stop = _th.Event()
def prod(k):
    i = k
    while not stop.is_set():
        if k == 0: bm.present(fr[i % 4]); st["n"] += 1
        else:
            r = bm.try_present(fr[i % 4]); st["drops" if r is None else "n"] += 1
            stop.wait(0.001)
        i += 1
ths = [_th.Thread(target=prod, args=(k,), daemon=True) for k in range(2)]
for t in ths: t.start()
state["ov"] = (stop, ths, st, bm)''')
    time.sleep(1); P("overloaded click->python ms", clicks())
    samples = []
    for k in range(4):
        X, Y, W, Hh = g(); wl.drag(X + W - 4, Y + Hh - 4, X + W - 400, Y + Hh - 300, steps=30, dt=0.02)
        X, Y, W, Hh = g(); wl.drag(X + W - 4, Y + Hh - 4, X + W + 396, Y + Hh + 296, steps=30, dt=0.02)
        m = proc(); samples.append((m["private"] // 1024, m["threads"], m["handles"]))
    r = lab('stop,ths,st,bm=state["ov"]; stop.set()\nfor t in ths: t.join(5)\nresult={"presented":st["n"],"try_present_dropped":st["drops"],"seq":bm._sequence}\nbm.close()')
    P("overload result", r, "| host (private KiB, threads, handles) during resize cycles", samples)

if not only or "bundle" in only:
    H("1000 displayed bundle PNG replacements (same size)")
    place(200, 40, 1200, 800)
    lab("doc = state['doc']\nresult = state['bundle_loop'](40)"); time.sleep(1)
    rows = []

    def sample(i):
        st = lab("doc = state['doc']\nresult = doc.resource_stats()"); m = proc()
        rows.append({"replacements": i, "host_private_kib": m["private"] // 1024, "decoded_cache": st["cache"]["bytes"], "cache_entries": st["cache"]["entries"]})
    sample(0)
    for k in range(20):
        lab(f"doc = state['doc']\nresult = state['bundle_loop'](50, start={k * 50})"); sample((k + 1) * 50)
    for r in rows[::4] + [rows[-1]]:
        P(json.dumps(r))
    xs = [r["replacements"] for r in rows[2:]]; ys = [r["host_private_kib"] for r in rows[2:]]; n = len(xs); mx = sum(xs) / n; my = sum(ys) / n
    P(f'private-bytes slope after warmup: {sum((a - mx) * (b - my) for a, b in zip(xs, ys)) / sum((a - mx) ** 2 for a in xs) * 1024:.0f} bytes/replacement; total {rows[-1]["host_private_kib"] - rows[0]["host_private_kib"]} KiB over 1000; decoded cache {sorted(set(r["decoded_cache"] for r in rows))}')

    H("intrinsic-size change and visible refresh")
    for (w_, h_, col, tag) in ((200, 100, (255, 0, 0), "red-200x100"), (200, 100, (0, 0, 255), "blue-200x100"), (300, 40, (0, 160, 0), "green-300x40"), (120, 160, (255, 200, 0), "yellow-120x160")):
        lab(f"doc = state['doc']\ndoc.register_resources('lab', {{'pic.png': (state['png']({w_},{h_},{col + (255,)}),'image/png')}})"); time.sleep(0.6)
        rr = c.rect("bimg", "rc2"); xw = win()
        px = ex(f"result = list(w.pixel({int(xw['cx'] + rr['x'] + rr['width'] / 2)}, {int(xw['cy'] + rr['y'] + rr['height'] / 2)}))")
        P(tag, "-> layout box", (rr["width"], rr["height"]), "centre pixel", px[:3])
    wl.wshot("final\\RC2-intrinsic.png")

    H("same-URL bundled font replacement")
    lab(r'''
doc = state['doc']
mono = open(r"C:\Windows\Fonts\consola.ttf", "rb").read(); serif = open(r"C:\Windows\Fonts\georgiab.ttf", "rb").read()
state['fonts'] = (mono, serif)
doc.register_resources('lab', {'pic.png': (state['png'](120, 160, (255, 200, 0, 255)), 'image/png'), 'font.ttf': (mono, 'font/ttf')})
doc.add_stylesheet("@font-face{font-family:LabFont;src:url(ui4me-res://lab/font.ttf)} #ftext{font-family:LabFont}")''')

    def ftext():
        time.sleep(2.5); rr = c.rect("ftext", "rc2"); xw = win()
        box = (int(xw["cx"] + rr["x"]), int(xw["cy"] + rr["y"]), int(xw["cx"] + rr["x"] + rr["width"]), int(xw["cy"] + rr["y"] + rr["height"]))
        return ex(f"import hashlib; im = w.grab({box}); result = [hashlib.sha256(im.tobytes()).hexdigest()[:16], im.size]"), rr["width"]
    a, wa = ftext()
    lab("doc = state['doc']; m, s = state['fonts']\ndoc.register_resources('lab', {'pic.png': (state['png'](120,160,(255,200,0,255)),'image/png'), 'font.ttf': (s,'font/ttf')})"); b, wb = ftext()
    lab("doc = state['doc']; m, s = state['fonts']\ndoc.register_resources('lab', {'pic.png': (state['png'](120,160,(255,200,0,255)),'image/png'), 'font.ttf': (m,'font/ttf')})"); c2, wc = ftext()
    P("text box width consolas/georgia-bold/consolas:", wa, wb, wc, "| mono vs serif differ:", a != b, "| mono restored identical:", a == c2)

if not only or "veto" in only:
    H("close veto (HTML button, WM_CLOSE, Alt+F4)")
    lab("state['veto'] = True"); place(200, 40, 1200, 800)
    t = time.time(); bx = c.screen("cls", cap=T, window="rc2"); wl.click(*bx); time.sleep(1)
    P("x button with veto: still open", bool(wl.wins(T)), len(events(t, "before_close")))
    t = time.time(); ex(f"w.post({win()['hwnd']}, 0x10)"); time.sleep(1); P("WM_CLOSE (taskbar/caption close) with veto: still open", bool(wl.wins(T)), len(events(t, "before_close")))
    wl.click(*c.screen("pad", cap=T, window="rc2")); time.sleep(0.3); t = time.time(); key("alt+F4"); time.sleep(1)
    P("Alt+F4 with veto: still open", bool(wl.wins(T)), len(events(t, "before_close")))
    lab("state['veto'] = False")

if not only or "teardown" in only:
    H("teardown: document replacement, backend kill")
    r = lab(r'''
doc = state['doc']; lw = state['lw']
a = doc.create_bitmap('b1', 64, 64); b = doc.create_bitmap('b2', 32, 32); a.present(bytes(64*64*4))
html = (HERE / 'lab.html').read_text(encoding='utf-8'); bms = list(doc._bitmaps); new = lw.load_html(html); state['doc2'] = new
cc = new.create_bitmap('b1', 64, 64); cc.present(bytes(64*64*4)); state['c'] = cc
result = {"old_closed": [x._closed for x in bms]}''')
    time.sleep(0.5); P("document replaced:", r, "| temp files", temp_files(), "| host handles", proc()["handles"])
    ex(f"import subprocess; subprocess.run(['taskkill', '/PID', '{bp}', '/F'], capture_output=True)"); time.sleep(2.5)
    P("backend killed: host alive", ex(f"import psutil; result = psutil.pid_exists({hp})"), "| temp files", temp_files())
    wl.wshot("final\\RC2-backend-killed.png")
    P("host exits after failure UI quit:", "(see N09)")
P("DONE")
