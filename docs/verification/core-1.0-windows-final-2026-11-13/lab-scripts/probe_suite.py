"""Windows final-binary probe suite: N03 N07 N04 N05 N13 (N06 in ime_suite).

Real input: xdotool -> xfreerdp3 -> Windows RDP session. Observation: in-session
agent (Win32, native UIA via COM, clipboard API) and the probe's own log.
"""
import json
import sys
import time
import wl
from wl import P, win, wins, key, click, at, uia, ex

HOST = r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"
APP = r"%LOCALAPPDATA%\ui4me-host-reference\app.toml"
OUT = r"F:\ui4me\lab\out\final"
M, U = "Python reference", "Unicode"
only = set(sys.argv[1:])


def launch(logname):
    pid = ex(rf'''
import os
os.makedirs(r"{OUT}", exist_ok=True)
log = os.path.join(r"{OUT}", {logname!r})
open(log, "w").close()
result = w.spawn([os.path.expandvars(r"{HOST}"), "run", "--app", os.path.expandvars(r"{APP}")], cwd=os.path.expanduser("~"), stdout=log)
''')
    mh, uh = win(M, 30)["hwnd"], win(U, 30)["hwnd"]
    ex(f"w.move({mh}, 100, 80, 900, 590); w.move({uh}, 1020, 80, 540, 390)")
    time.sleep(1.0)
    return pid


def out(logname):
    return ex(rf'''
import json, os
rows = []
for line in open(os.path.join(r"{OUT}", {logname!r}), encoding="utf-8", errors="replace"):
    try:
        rec = json.loads(line)
    except ValueError:
        rows.append(line.rstrip()); continue
    f = rec.get("fields", {{}})
    if "backend output" in json.dumps(rec):
        rows.append(f.get("text", f.get("line", "")))
result = rows
''')


def last(logname, k):
    return [t for t in out(logname) if k in t][-1:]


def wait_exit(pid, timeout=20):
    return ex(f'''
import psutil
try:
    result = psutil.Process({pid}).wait({timeout})
except psutil.NoSuchProcess:
    result = "gone"
''')


def quit_from(title):
    wl.activate(title)
    key("ctrl+q")


if not only or "N03" in only or "N07" in only:
    P("## N03 / N07")
    pid = launch("N03-probe-run.log")
    hu = win(U)["hwnd"]
    ex(f"w.move({hu}, 900, 300, 600, 420)"); time.sleep(0.8)
    x = win(U); P("resize", x["rect"], "client", x["cw"], x["ch"])
    for name, cmd in (("maximize", 3), ("restore", 9), ("minimize", 6), ("restore", 9)):
        ex(f"w.show({hu}, {cmd})"); time.sleep(1)
        x = wins(U)[0] if wins(U) else ex(f"result = w.info({hu})")
        P(name, x["rect"], "max", x["zoomed"], "min", x["iconic"])
    wl.wshot("final\\N03-two-windows.png")
    # Real close: click the second window's caption close button (top-right of frame).
    x = win(U); fx, fy, fw, fh = x["frame"]
    click(fx + fw - 22, fy + 14); time.sleep(1.5)
    P("caption close of second window ->", [y["title"] for y in wins("UI4ME")], "| backend", [t for t in out("N03-probe-run.log") if "Close accepted" in t])
    m = win(M)
    tree = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
result = [(d["level"], d["type"], d["name"], d.get("value")) for d in uia.dump(e)]
''')
    P("UIA tree", tree)
    wl.activate(M); order = []
    for i in range(6):
        key("Tab"); time.sleep(0.4)
        order.append(uia("result = uia.focused()['name']"))
    P("Tab order", order)
    P("SetFocus(Paste)", uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
uia.find(e, name="Paste").SetFocus(); time.sleep(0.3)
result = uia.focused()["name"]'''))
    P("Select first 6 chars via TextPattern + Invoke(Copy):", uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
ed = uia.find(e, control="Edit")
sel = uia.select_text(ed, 0, 6); time.sleep(0.3)
uia.pattern(uia.find(e, name="Copy"), "Invoke").Invoke(); time.sleep(0.8)
result = {{"selected": sel, "selection": uia.text_selection(ed), "clipboard": w.clipboard_get()}}'''))
    if not only or "N04" in only:
        P("## N04 clipboard")
        r = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
ed = uia.find(e, control="Edit"); r = ed.CurrentBoundingRectangle
result = [r.left, r.top, r.right, r.bottom, uia.document_text(ed)]''')
        click((r[0] + r[2]) / 2, (r[1] + r[3]) / 2); time.sleep(0.3)
        key("ctrl+a", "ctrl+c"); time.sleep(0.6)
        v = wl.clip_get()
        P("probe -> other app clipboard: equal", v == r[4], "len", len(v or ""))
        wl.clip_set("Unicode from another app: Příliš žluťoučký kůň — 日本語 — 😀"); time.sleep(0.4)
        key("ctrl+a", "ctrl+v"); time.sleep(0.8)
        P("other app -> probe", repr(uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
result = uia.document_text(uia.find(e, control="Edit"))''')))
        before = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
result = uia.document_text(uia.find(e, control="Edit"))''')
        ex('w.clipboard_set("x" * (2_200_000))'); time.sleep(0.5)
        key("End", "ctrl+v"); time.sleep(2)
        after = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
result = uia.document_text(uia.find(e, control="Edit"))''')
        P("2.2 MB paste: unchanged", after == before, len(before), "| log", [t for t in out("N03-probe-run.log") if "exceeds" in t][-1:])
        wl.clip_set("restored")
    quit_from(M); P("exit", wait_exit(pid))

if not only or "N05" in only:
    P("## N05 dialogs")
    lg = "N05-probe-run.log"
    pid = launch(lg)

    def dlg():
        return [(d["title"], d["class"], d["owner"], d["enabled"]) for d in wins() if d["class"] == "#32770"]

    def owner_state():
        return {t: win(t)["enabled"] for t in (M, U) if wins(t)}
    mh, uh = win(M)["hwnd"], win(U)["hwnd"]
    ex(f"w.move({mh}, 100, 80, 900, 590); w.move({uh}, 1020, 80, 540, 390)"); time.sleep(0.8)
    wl.activate(M); key("F3"); time.sleep(2.5)
    P("F3", dlg(), "owner hwnd main", mh, "enabled", owner_state()); wl.wshot("final\\N05-open-dialog.png")
    key("Escape"); time.sleep(1.5); P("Escape", dlg(), last(lg, "dialog.open_files"), owner_state())
    wl.activate(M); key("F3"); time.sleep(2.5)
    ex(r'''
import os
os.makedirs(r"F:\ui4me\lab\files", exist_ok=True)
open(r"F:\ui4me\lab\files\žluťoučký 日本.txt", "w", encoding="utf-8").write("unicode name")
''')
    wl.clip_set(r"F:\ui4me\lab\files\žluťoučký 日本.txt"); time.sleep(0.3)
    key("ctrl+v"); time.sleep(0.5); key("Return"); time.sleep(2)
    P("select Unicode file", dlg(), last(lg, "dialog.open_files"))
    wl.activate(M); key("F4"); time.sleep(2.5); P("F4", dlg())
    wl.clip_set(r"F:\ui4me\lab\files\saved by probe ✓.txt"); time.sleep(0.3)
    key("ctrl+a", "ctrl+v"); time.sleep(0.5); key("Return"); time.sleep(2)
    P("save", dlg(), last(lg, "dialog.save_file"))
    wl.activate(M); key("F5"); time.sleep(2.5); P("F5", dlg()); wl.wshot("final\\N05-folder-dialog.png")
    key("Escape"); time.sleep(1.5); P("Escape", dlg(), last(lg, "dialog.choose_folder"))
    wl.activate(M); key("F3"); time.sleep(2.5); P("F3 again", dlg(), owner_state())
    wl.activate(U); key("F7"); time.sleep(2); P("F7 from second window", dlg(), last(lg, "dialog.open_files"))
    wl.activate(M); key("F4"); time.sleep(2.5); P("F4 in main, then Ctrl+Q from the second window", dlg(), owner_state())
    t = time.time(); wl.activate(U); key("ctrl+q")
    P("quit with save dialog open:", wait_exit(pid), "after %.2f s" % (time.time() - t), "dialogs", dlg(), last(lg, "dialog.save_file"))
    lg = "N05b-probe-run.log"
    pid = launch(lg)
    mh = win(M)["hwnd"]
    wl.activate(M); key("F3"); time.sleep(2.5); P("F3 then parent close", dlg())
    ex(f"w.post({mh}, 0x10)"); time.sleep(2.5)
    P("after parent WM_CLOSE", dlg(), last(lg, "dialog.open_files"), [y["title"] for y in wins("UI4ME")])
    wl.activate(U); key("ctrl+q"); P("exit", wait_exit(pid))

if not only or "N13" in only:
    P("## N13 drag and drop")
    lg = "N13-probe-run.log"
    ex(r'''
import os, subprocess
d = r"F:\ui4me\lab\dnd"
os.makedirs(d, exist_ok=True)
open(os.path.join(d, "žluťoučký 日本.txt"), "w", encoding="utf-8").write("drag me")
open(os.path.join(d, "source.txt"), "w", encoding="utf-8-sig").write("drag this text: Příliš žluťoučký")
w.spawn(["explorer.exe", d])
w.spawn([r"C:\Program Files\Windows NT\Accessories\wordpad.exe", os.path.join(d, "source.txt")])
''')
    time.sleep(4)
    pid = launch(lg); time.sleep(1)
    ex(f'''
for t, r in (("dnd", (40, 60, 700, 500)), ("source - WordPad", (40, 600, 700, 330)), ("Python reference", (780, 60, 800, 560)), ("Unicode", (1000, 640, 500, 300))):
    x = w.window(t); w.move(x["hwnd"], *r)
''')
    time.sleep(1.5)
    item = uia('''
e = uia.window(title="dnd")
it = uia.find(e, name="žluťoučký 日本", control="ListItem")
r = it.CurrentBoundingRectangle
result = [r.left, r.top, r.right, r.bottom]''')
    pm = win(M); tx, ty = pm["cx"] + 400, pm["cy"] + 280
    wl.activate("dnd")
    wl.drag(item[0] + 30, (item[1] + item[3]) / 2, tx, ty, steps=30, dt=0.03)
    time.sleep(1.5); wl.wshot("final\\N13-after-file-drop.png")
    wl.activate("source - WordPad"); key("ctrl+a"); time.sleep(0.3)
    doc = uia('''
e = uia.window(title="source - WordPad")
d = uia.find(e, control="Document")
rects = uia.pattern(d, "Text").DocumentRange.GetBoundingRectangles()
result = list(rects)[:4]''')
    sx, sy = doc[0] + min(60, doc[2] / 2), doc[1] + doc[3] / 2
    wl.move(sx, sy); time.sleep(0.2); wl.xdo("mousedown", 1); time.sleep(0.6)
    for i in range(1, 31):
        wl.move(sx + (tx - sx) * i / 30, sy + (ty - sy) * i / 30); time.sleep(0.04)
    time.sleep(0.3); wl.xdo("mouseup", 1)
    time.sleep(1.5); wl.wshot("final\\N13-after-text-drop.png")
    P("drops", [t for t in out(lg) if "dropped" in t])
    quit_from(M); P("exit", wait_exit(pid))
    ex('''
for t in ("source - WordPad", "dnd"):
    for x in w.windows(t): w.post(x["hwnd"], 0x10)
''')
P("DONE")
