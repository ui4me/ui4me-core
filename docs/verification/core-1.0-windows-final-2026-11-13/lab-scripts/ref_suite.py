"""Windows final-binary reference-app suite: N01 N02 N12 R11 R01 R02 R04 R05 R06 N08 R03 R08."""
import sys
import time
import wl
from wl import P, key, uia, ex, click
from startmenu import start_launch

C = "Desktop workspace"
only = set(sys.argv[1:])


def el(i, name=None):
    sel = f'automation_id="{i}"' if i else f'name="{name}"'
    return uia(f'''e = uia.window(title="{C}")
r = uia.find(e, {sel}).CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2, r.left, r.top, r.right, r.bottom]''')


def d(i):
    return uia(f'''e = uia.window(title="{C}")
result = uia.describe(uia.find(e, automation_id="{i}"))''')


def val(i):
    return d(i).get("value")


def press(i, name=None):
    x = el(i, name); click(x[0], x[1]); time.sleep(0.4)


def rows():
    return uia(f'''e = uia.window(title="{C}")
t = uia.find(e, control="Table")
out = []
for x in uia.dump(t, depth=3):
    if x["level"] == 1 and x["id"].startswith("row-"):
        out.append(x["id"])
cells = [x["name"] for x in uia.dump(t, depth=3) if x["level"] == 2]
result = [out, cells[-4:]]''')


def status():
    return uia(f'''e = uia.window(title="{C}")
result = [x["name"] for x in uia.dump(uia.find(e, automation_id="status")) if x["type"] == "Text"]''')


def focused():
    f = uia("result = uia.focused()")
    return f["id"] or f["name"]


def procs():
    return ex('''
out = []
for p in w.procs():
    cl = " ".join(p["cmdline"] or [])
    if "renderer_demo" in cl or (p["name"] == "ui4me-host.exe" and "renderer-reference" in cl):
        import psutil
        try:
            pp = psutil.Process(p["pid"]); cwd = pp.cwd(); parent = psutil.Process(p["ppid"]).name()
        except Exception as e:
            cwd = parent = repr(e)
        out.append({"pid": p["pid"], "ppid": p["ppid"], "parent": parent, "name": p["name"], "exe": p["exe"], "cwd": cwd, "session": p["session"], "cmd": cl[:200]})
result = out''')


if not only or "N01" in only:
    P("## N01/N02/N12/R11/R01")
    host = ex(r'''import os; result = os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe")''')
    P("installed host", host, "sha256", ex(f"result = w.sha256({host!r})"))
    start_launch("UI4ME Desktop Workspace", "final\\N01-start-entry.png")
    x = wl.win(C, 30); time.sleep(2)
    P("N01 window", {k: x[k] for k in ("title", "class", "pid", "rect", "dpi")})
    for p in procs():
        P("  N12", p)
    P("N01 console windows of the app tree:", ex('''
pids = set()
for p in w.procs():
    if "renderer_demo" in " ".join(p["cmdline"] or []) or p["name"] == "ui4me-host.exe": pids.add(p["pid"])
result = [(x["title"], x["class"]) for x in w.windows() if x["pid"] in pids and x["class"] != "Window Class"]'''))
    P("N01 taskbar buttons:", uia('''
a = uia.A(); tb = a.GetRootElement().FindFirst(uia.U.TreeScope_Children, a.CreatePropertyCondition(uia.U.UIA_ClassNamePropertyId, "Shell_TrayWnd"))
result = [d["name"] for d in uia.dump(tb, depth=8, max_nodes=200) if d["type"] == "Button" and d["name"] and "UI4ME" in d["name"]]'''))
    wl.wshot("final\\R01-window.png")
    start_launch("UI4ME Desktop Workspace")
    time.sleep(2)
    P("N02 after second launch: windows", len(wl.wins(C)), "backends", len([p for p in procs() if "renderer_demo" in p["cmd"]]),
      "hosts", len([p for p in procs() if p["name"] == "ui4me-host.exe"]))

hw = wl.win(C, 30)["hwnd"]
ex(f"w.move({hw}, 200, 20, 1200, 960)"); time.sleep(1.5)

if not only or "R02" in only:
    P("## R02")
    press("name"); key("ctrl+a"); wl.type_("<tag> & value"); time.sleep(0.5); P("name", repr(val("name")))
    b = d("notify").get("toggle"); press("notify"); P("checkbox", b, "->", d("notify").get("toggle"))
    press("work"); P("radio Work selected", d("work").get("selected"), "Personal", d("personal").get("selected"))
    press("estimate"); key("Up", "Up"); time.sleep(0.4); P("estimate", d("estimate").get("range"))
    v0 = d("progress").get("range"); press("progress"); v1 = d("progress").get("range"); key("Right", "Right"); time.sleep(0.4)
    P("slider", v0, "-> click", v1, "-> +2", d("progress").get("range"))
    press("priority"); time.sleep(0.6); key("Down", "Return"); time.sleep(0.6); P("priority", val("priority"))
    r0 = rows(); press("add-row"); time.sleep(0.8); P("rows", len(r0[0]), "->", rows(), "| status", status())
    wl.wshot("final\\R02-row-added.png")
    press("theme"); time.sleep(0.8); wl.wshot("final\\R02-palette-toggled.png"); press("theme")

if not only or "R04" in only:
    P("## R04")
    press("name"); order = [focused()]
    for i in range(13):
        key("Tab"); time.sleep(0.35); order.append(focused())
    P("tab order", order); key("shift+Tab"); time.sleep(0.3); P("shift-tab", focused())
    press("name"); key("ctrl+a", "BackSpace"); time.sleep(0.2); n0 = len(rows()[0])
    key("Return"); time.sleep(0.8); P("empty required + Enter: rows", n0, "->", len(rows()[0]), "focus", focused())
    wl.wshot("final\\R04-invalid-required.png")
    wl.type_("Typed item"); key("Return"); time.sleep(0.8); P("implicit submit: last row", rows()[1])
    press("notes"); key("ctrl+a", "BackSpace"); wl.type_("alpha beta gamma"); time.sleep(0.3)
    n = el("notes"); wl.drag(n[2] + 12, n[3] + 14, n[2] + 60, n[3] + 14, steps=10)
    P("drag selection", uia(f'''e = uia.window(title="{C}")
result = uia.text_selection(uia.find(e, automation_id="notes"))'''))
    key("ctrl+x"); time.sleep(0.3); a1 = val("notes"); cl = wl.clip_get()
    key("ctrl+z"); time.sleep(0.3); a2 = val("notes")
    key("ctrl+shift+z"); time.sleep(0.3); a3 = val("notes")
    key("ctrl+y"); time.sleep(0.3)
    P("cut/undo/redo", repr(a1), repr(a2), repr(a3), "| clipboard", repr(cl))
    for i in range(4):
        press("add-row"); time.sleep(0.3)
    first = lambda: uia(f'''e = uia.window(title="{C}")
result = uia.find(e, automation_id="row-1").CurrentBoundingRectangle.top''')
    t = el("table-scroll") if False else uia(f'''e = uia.window(title="{C}")
r = uia.find(e, control="Table").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')
    wl.move(*t); time.sleep(0.3); y0 = first()
    for i in range(6):
        wl.xdo("click", 5); time.sleep(0.1)
    time.sleep(0.6); P("wheel down: row-1 top", y0, "->", first()); wl.wshot("final\\R04-table-scrolled.png")
    for i in range(6):
        wl.xdo("click", 4); time.sleep(0.1)
    time.sleep(0.4); P("wheel up: row-1 top", first())
    x = el("notes"); click(x[0], x[1], "right"); time.sleep(0.5); P("right-click on notes: focus", focused()); key("Escape")

if not only or "R05" in only:
    P("## R05 (UIA patterns)")
    P(uia(f'''e = uia.window(title="{C}")
ev = uia.listen(e)
out = {{}}
name = uia.find(e, automation_id="name")
uia.pattern(name, "Value").SetValue("Set via UIA"); time.sleep(0.5); out["SetValue"] = uia.pattern(name, "Value").CurrentValue
out["select"] = uia.select_text(name, 0, 3); time.sleep(0.3); out["GetSelection"] = uia.text_selection(name)
cb = uia.find(e, automation_id="notify"); t0 = uia.pattern(cb, "Toggle").CurrentToggleState
uia.pattern(cb, "Toggle").Toggle(); time.sleep(0.5); out["Toggle"] = [t0, uia.pattern(cb, "Toggle").CurrentToggleState]
sl = uia.find(e, automation_id="progress"); uia.pattern(sl, "RangeValue").SetValue(75); time.sleep(0.5); out["RangeValue"] = uia.pattern(sl, "RangeValue").CurrentValue
pr = uia.find(e, automation_id="priority"); x = uia.pattern(pr, "ExpandCollapse"); x.Expand(); time.sleep(0.5); out["expanded"] = x.CurrentExpandCollapseState
low = uia.find(pr, name="Low"); uia.pattern(low, "SelectionItem").Select(); time.sleep(0.6); out["combo value"] = uia.pattern(pr, "Value").CurrentValue
try:
    x.Collapse(); time.sleep(0.3)
except Exception as err:
    out["collapse"] = repr(err)
n0 = len([1 for d in uia.dump(uia.find(e, control="Table"), depth=1) if d["level"] == 1])
uia.pattern(uia.find(e, automation_id="add-row"), "Invoke").Invoke(); time.sleep(1.5)
out["Invoke(Add row) rows"] = [n0, len([1 for d in uia.dump(uia.find(e, control="Table"), depth=1) if d["level"] == 1])]
st = uia.find(e, automation_id="status"); out["status live"] = st.GetCurrentPropertyValue(uia.U.UIA_LiveSettingPropertyId)
cells = [d for d in uia.dump(uia.find(e, control="Table"), depth=3) if d["level"] == 2]
last = uia.find(e, name=cells[-4]["name"])
before = last.CurrentBoundingRectangle.top
uia.pattern(last, "ScrollItem").ScrollIntoView(); time.sleep(0.8)
out["ScrollIntoView last row"] = [before, last.CurrentBoundingRectangle.top, last.CurrentIsOffscreen]
out["selection unchanged"] = uia.text_selection(name)
time.sleep(0.5)
out["events"] = sorted(set((x["event"], (x.get("name") or "")[:30]) for x in uia.events()))
uia.unlisten()
result = out'''))

if not only or "R06" in only or "N08" in only:
    P("## R06 / N08")
    before = [x["hwnd"] for x in wl.wins("UI4ME")]
    press("second-window"); time.sleep(2)
    sec = [x for x in wl.wins("UI4ME") if x["hwnd"] not in before][0]; P("second window", sec["title"], "owner", sec["owner"])
    sh = sec["hwnd"]; ex(f"w.move({sh}, 900, 100, 700, 500)"); time.sleep(1); x = ex(f"result = w.info({sh})"); P("resize", x["rect"], x["cw"], x["ch"])
    wl.wshot("final\\R06-second-small.png")
    for op, cmd in (("maximize", 3), ("restore", 9), ("minimize", 6), ("restore", 9)):
        ex(f"w.show({sh}, {cmd})"); time.sleep(1); x = ex(f"result = w.info({sh})"); P(op, x["rect"], "max", x["zoomed"], "min", x["iconic"])
    ex(f"w.move({hw}, 100, 20, 520, 900)"); time.sleep(1.2); wl.wshot("final\\R06-main-narrow.png")
    press("name"); P("narrow click Name ->", focused())
    ex(f"w.post({sh}, 0x10)"); time.sleep(1.5); P("after closing second", [x["title"] for x in wl.wins("UI4ME")])
    ex(f"w.move({hw}, 200, 20, 1200, 960)"); time.sleep(1)

if not only or "R03" in only:
    P("## R03 (Microsoft Pinyin + clipboard)")
    press("notes"); key("ctrl+a", "BackSpace"); time.sleep(0.3)
    key("super+space"); time.sleep(1.2); P("layout", ex("result = w.keyboard_layout()"))
    wl.type_("nihao"); time.sleep(1.2); wl.wshot("final\\R03-preedit-textarea.png"); P("preedit value", repr(val("notes")))
    key("space"); time.sleep(0.8); P("commit", repr(val("notes")))
    wl.type_("shijie"); time.sleep(0.8); key("Escape"); time.sleep(0.6); P("Escape", repr(val("notes")))
    wl.type_("zhongwen"); time.sleep(1.0); key("2"); time.sleep(0.8); P("candidate 2", repr(val("notes")))
    key("ctrl+a"); wl.type_("ceshi"); time.sleep(0.8); key("space"); time.sleep(0.8); P("compose over selection", repr(val("notes")))
    wl.type_("hao"); time.sleep(0.8); press("name"); time.sleep(0.8); P("focus change mid-preedit: Notes", repr(val("notes")), "| Name", repr(val("name")))
    key("super+space"); time.sleep(1.2); P("layout back", ex("result = w.keyboard_layout()"))
    press("notes"); key("ctrl+a", "ctrl+c"); time.sleep(0.5); P("Ctrl+C -> other app", repr(wl.clip_get()))
    wl.clip_set("Žluťoučký 日本 😀 from another app"); time.sleep(0.5)
    press("name"); key("ctrl+a", "ctrl+v"); time.sleep(0.8); P("other app -> Ctrl+V", repr(val("name")))

if not only or "R08" in only:
    P("## R08")
    dl = lambda: [(x["title"], x["owner"] == hw, x["enabled"]) for x in wl.wins() if x["class"] == "#32770"]
    press("choose-file"); time.sleep(2.5); P("picker", dl(), "owner enabled", ex(f"result = w.info({hw})['enabled']")); wl.wshot("final\\R08-file-dialog.png")
    key("Escape"); time.sleep(1.5); P("cancel ->", status(), dl())
    wl.clip_set(r"F:\ui4me\lab\files\žluťoučký 日本.txt")
    press("choose-file"); time.sleep(2.5); key("ctrl+v"); time.sleep(0.6); key("Return"); time.sleep(2.5); P("select ->", status(), dl())
    press("show-dialog"); time.sleep(1.2); wl.wshot("final\\R08-html-dialog.png")
    P("HTML dialog focus", focused(), [x["name"] for x in uia(f'''e = uia.window(title="{C}")
result = uia.dump(e, depth=20, max_nodes=600)''') if x["type"] == "Window" and x["level"] > 0])
    key("Escape"); time.sleep(0.8); P("Escape -> focus", focused())
    press("show-popover"); time.sleep(1.2); wl.wshot("final\\R08-popover.png"); P("popover open, focus", focused())
    key("Escape"); time.sleep(0.8); P("Escape popover -> focus", focused())
    hp = wl.win(C)["pid"]
    press("choose-file"); time.sleep(2.5); P("picker open", dl())
    ex(f"w.post({hw}, 0x10)"); time.sleep(3)
    P("parent close -> picker", dl(), "app windows", [x["title"] for x in wl.wins("UI4ME")],
      "host alive", ex(f"import psutil; result = psutil.pid_exists({hp})"))
P("DONE")
