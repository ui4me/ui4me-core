"""Resize-to-present latency, frame cost and idle CPU on Windows (installed host, debug frame timing)."""
import itertools, json, time
import wl, cw as c
from wl import P, ex
import c_suite_lib as L
T = "UI4ME RC2 lab"
ex("import os; os.environ['RUST_LOG'] = 'ui4me_host=debug'")
L.restart()
ex("import os; os.environ.pop('RUST_LOG', None)")
LOG = r"F:\ui4me\lab\out\final\C-lab-host-%d.log" % L._n[0]
c.code(r'''
lw = app.open_file(HERE / "lab.html", title="UI4ME RC2 lab", width=1000, height=700, decorations=False, min_width=320, min_height=240)
windows["rc2"] = lw; state["doc"] = lw.document; state["lw"] = lw
doc = lw.document
doc.register_resources('lab', {'pic.png': (b"", 'image/png')}) if False else None''')
c.code("doc = state['doc']\n" + open("stress_win.py").read())
x = wl.win(T, 20); hp = x["pid"]
HZ = ex("result = w.cpu_hz()")
cyc = lambda: ex(f"result = [w.cycles({hp}), time.time()]")
q = lambda a, p: round(sorted(a)[min(len(a) - 1, int(p * len(a)))], 2) if a else None


def frames_since(t0):
    rows = ex(rf'''
import json, datetime
out = []
for l in open(r"{LOG}", encoding="utf-8", errors="replace"):
    try: d = json.loads(l)
    except ValueError: continue
    if d.get("level") == "DEBUG" and d["fields"].get("message") == "frame presented":
        ts = datetime.datetime.fromisoformat(d["timestamp"].replace("Z", "+00:00")).timestamp()
        if ts >= {t0}: out.append(d["fields"])
result = out''')
    return rows


def place():
    ex(f"w.move({wl.win(T)['hwnd']}, 200, 60, 1000, 700)"); time.sleep(1)


def idle(tag, secs=15):
    time.sleep(2); a = cyc(); t0 = time.time(); time.sleep(secs); b = cyc(); m = ex(f"result = w.mem({hp})")
    P(json.dumps({"tag": tag, "host_cpu_pct_cycles": round(100 * (b[0] - a[0]) / HZ / (b[1] - a[1]), 2), "frames": len(frames_since(t0)), "private_kib": m["private"] // 1024}))


def resize(tag):
    place(); t0 = time.time(); a = cyc()
    for k in range(3):
        X, Y, W, H = wl.win(T)["rect"]; wl.drag(X + W - 5, Y + H - 5, X + W - 305, Y + H - 205, steps=40, dt=0.016); time.sleep(0.3)
        X, Y, W, H = wl.win(T)["rect"]; wl.drag(X + W - 5, Y + H - 5, X + W + 295, Y + H + 195, steps=40, dt=0.016); time.sleep(0.3)
    b = cyc()
    fr = frames_since(t0); lat = [f["resize_to_present_ms"] for f in fr if f.get("resize_to_present_ms") is not None]; fms = [f["frame_ms"] for f in fr]
    P(json.dumps({"tag": tag, "frames": len(fr), "distinct_sizes": len([k for k, _ in itertools.groupby((f.get("width"), f.get("height")) for f in fr)]),
                  "resize_to_present_ms": {"p50": q(lat, .5), "p95": q(lat, .95), "max": q(lat, 1)}, "frame_ms": {"p50": q(fms, .5), "p95": q(fms, .95)},
                  "host_cpu_pct_cycles": round(100 * (b[0] - a[0]) / HZ / (b[1] - a[1]), 1)}))
place(); wl.click(1550, 950); idle("static idle (unfocused)")
resize("resize: static")
s = c.code('doc = state["doc"]\nresult = doc.add_stylesheet("@keyframes spin{to{transform:rotate(360deg)}} #pad{animation:spin 2s linear infinite}")')
idle("CSS animation running", 10); resize("resize: CSS animation"); c.code(f"doc = state['doc']\ndoc.remove_stylesheet({s['sheet'] if isinstance(s, dict) else s})"); idle("after animation removed")
c.code(r'''
import threading as _th
doc = state["doc"]
bm = doc.create_bitmap('b1', 320, 200); fr = state['frames_for'](320, 200); stop = _th.Event()
def prod():
    i = 0; import time as _t; t = _t.perf_counter()
    while not stop.is_set():
        bm.present(fr[i % 4]); i += 1; d = t + i / 60 - _t.perf_counter()
        if d > 0: stop.wait(d)
th = _th.Thread(target=prod, daemon=True); th.start(); state['sc'] = (stop, th, bm)''')
idle("60 fps 320x200 bitmap", 10); resize("resize: 60 fps bitmap")
c.code("stop, th, bm = state['sc']; stop.set(); th.join(3); bm.close()"); idle("after bitmap closed")
P("DONE")
