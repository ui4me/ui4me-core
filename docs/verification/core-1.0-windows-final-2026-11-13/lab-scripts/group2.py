"""Windows final-binary group 2: R09, R07, N09/N10 (faults), N11."""
import json, re, sys, time
import wl
from wl import P, key, uia, ex
only = set(sys.argv[1:])
C = "Desktop workspace"
OUT = r"F:\ui4me\lab\out\final"
SRC = r"F:\ui4me\src"
PY = r"C:\Program Files\Python313\python.exe"
HOST = r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"


def spawn(argv, log, cwd=SRC):
    return ex(rf'''
import os
argv = [os.path.expandvars(a) for a in {argv!r}]
result = w.spawn(argv, cwd=r"{cwd}", stdout=os.path.join(r"{OUT}", {log!r}))''')


def logtext(log):
    return ex(rf'''import os; result = open(os.path.join(r"{OUT}", {log!r}), encoding="utf-8", errors="replace").read()''')


def status():
    return uia(f'''e = uia.window(title="{C}")
result = [x["name"] for x in uia.dump(uia.find(e, automation_id="status")) if x["type"] == "Text"]''')


def cpu(pid):
    return ex(f"result = w.mem({pid})['cpu']")


def alive(pid):
    return ex(f"import psutil; result = psutil.pid_exists({pid})")


def wait_exit(pid, t=15):
    s = time.time()
    while time.time() - s < t and alive(pid):
        time.sleep(0.2)
    return round(time.time() - s, 2)


def el(i):
    return uia(f'''e = uia.window(title="{C}")
r = uia.find(e, automation_id="{i}").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')


def val(i):
    return uia(f'''e = uia.window(title="{C}")
result = uia.pattern(uia.find(e, automation_id="{i}"), "Value").CurrentValue''')


if not only or "R09" in only:
    P("## R09 updates covered/minimized")
    spawn([PY, "tools/run_renderer.py", "--host", HOST, "--updates", "0.25"], "R09-updates-run.log")
    x = wl.win(C, 30); hp = x["pid"]; time.sleep(4); ex(f"w.move({x['hwnd']}, 300, 100, 1000, 800)"); time.sleep(1)
    upd = lambda: re.findall(r"UPDATE (\d+)", logtext("R09-updates-run.log"))
    # Cover the window completely with a maximized topmost Notepad.
    ex(r'''w.spawn(["notepad.exe"])'''); n = wl.win("Notepad", 10)
    ex(f"w.show({n['hwnd']}, 3)"); time.sleep(1)
    c0, t0, u0, s0 = cpu(hp), time.time(), upd(), status(); time.sleep(45); wl.wshot("final\\R09-main-fully-covered.png")
    P(f"covered 45 s: host CPU {100 * (cpu(hp) - c0) / (time.time() - t0):.2f}% UPDATE {u0[-1:]} -> {upd()[-1:]} status {s0} -> {status()}")
    ex(f"w.post({n['hwnd']}, 0x10)"); time.sleep(1)
    ex(f"w.show({x['hwnd']}, 6)"); time.sleep(1)
    c0, t0, u0, s0 = cpu(hp), time.time(), upd(), status(); time.sleep(45)
    P(f"minimized 45 s: host CPU {100 * (cpu(hp) - c0) / (time.time() - t0):.2f}% UPDATE {u0[-1:]} -> {upd()[-1:]} status {s0} -> {status()}")
    ex(f"w.show({x['hwnd']}, 9)"); time.sleep(1.5); wl.activate(C)
    wl.wshot("final\\R09-restored.png"); P("restored status", status())
    wl.click(*el("name")); key("End"); wl.type_(" ok"); time.sleep(0.4); P("typing after restore", repr(val("name")))
    # Cover UI4ME with a maximized TOPMOST Notepad, then give UI4ME keyboard focus through
    # its real taskbar button (it activates but stays underneath the topmost window).
    ex(r'''w.spawn(["notepad.exe"])'''); n = wl.win("Notepad", 10); ex(f"w.show({n['hwnd']}, 3)"); time.sleep(1)
    ex(f"import ctypes; ctypes.windll.user32.SetWindowPos(ctypes.c_void_p({n['hwnd']}), ctypes.c_void_p(-1), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)"); time.sleep(0.5)
    tb = uia('''
a = uia.A(); tb = a.GetRootElement().FindFirst(uia.U.TreeScope_Children, a.CreatePropertyCondition(uia.U.UIA_ClassNamePropertyId, "Shell_TrayWnd"))
b = [d for d in uia.dump(tb, depth=8, max_nodes=200) if d["type"] == "Button" and d["name"] and "Desktop Workspace" in d["name"]][0]
result = [b["rect"][0] + b["rect"][2] / 2, b["rect"][1] + b["rect"][3] / 2]''')
    for attempt in range(3):
        wl.click(*tb); time.sleep(1)
        if ex("result = w.foreground()")["pid"] == hp:
            break
    fg = ex("result = w.foreground()"); P("foreground (keyboard focus) before Ctrl+Shift+Q:", fg["title"], "| UI4ME covered by topmost Notepad:", ex(f"result = w.info({n['hwnd']})['exstyle'] & 8 != 0"))
    wl.wshot("final\\R09-quit-while-covered.png")
    key("ctrl+shift+q"); P("Ctrl+Shift+Q while covered: exit after", wait_exit(hp), "s")
    ex(f"w.post({n['hwnd']}, 0x10)")

    P("## R09 hold callback")
    spawn([PY, "tools/run_renderer.py", "--host", HOST, "--hold-callback"], "R09-hold-run.log")
    x = wl.win(C, 30); hp = x["pid"]; time.sleep(4); ex(f"w.move({x['hwnd']}, 300, 100, 1000, 800)"); time.sleep(1)
    time.sleep(2); wl.click(*el("add-row")); time.sleep(2); P("HOLD marker", "HOLD:" in logtext("R09-hold-run.log"), "| rows", uia(f'''e = uia.window(title="{C}")
result = len([d for d in uia.dump(uia.find(e, control="Table"), depth=1) if d["level"] == 1])'''))
    wl.click(*el("notes")); wl.type_("typed while python is blocked"); time.sleep(0.5); P("typing while blocked", repr(val("notes")))
    wl.xdo("click", 5); wl.wshot("final\\R09-hold-typing.png")
    ex(f"w.post({x['hwnd']}, 0x10)"); P("close while blocked: exit after", wait_exit(hp, 15), "s")

    P("## R09 crash after ready")
    spawn([PY, "tools/run_renderer.py", "--host", HOST, "--crash-after-ready", "4"], "R09-crash-run.log")
    x = wl.win(C, 30); hp = x["pid"]; ex(f"w.move({x['hwnd']}, 300, 100, 1000, 700)"); time.sleep(7)
    wl.wshot("final\\R09-crash-failure-ui.png"); x = wl.win(C); P("window after crash", x["title"])
    q = uia(f'''e = uia.window(title="{C}")
result = [(d["type"], d["name"]) for d in uia.dump(e, depth=10) if d["name"]]''')
    P("failure UI accessibility", q[:12])
    try:
        qb = uia(f'''e = uia.window(title="{C}")
r = uia.find(e, name="Quit", control="Button").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')
        wl.click(*qb)
    except RuntimeError:
        wl.activate(C); key("ctrl+q")
    P("failure UI Quit: exit after", wait_exit(hp), "s")

if not only or "R07" in only:
    P("## R07 broken assets")
    B = r"F:\ui4me\lab\broken-app"
    ex(rf'''
import os, shutil
if os.path.exists(r"{B}"): shutil.rmtree(r"{B}")
shutil.copytree(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-renderer-reference"), r"{B}")
os.remove(r"{B}\ui\landscape.png")
for f in os.listdir(r"{B}\ui\fonts"):
    open(os.path.join(r"{B}\ui\fonts", f), "wb").write(os.urandom(2000))
t = open(r"{B}\app.toml", encoding="utf-8").read().replace("org.ui4me.RendererReference", "org.ui4me.RendererBroken").replace(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-renderer-reference").replace("\\", "\\\\"), r"{B}".replace("\\", "\\\\"))
open(r"{B}\app.toml", "w", encoding="utf-8").write(t)
result = None''')
    spawn([HOST, "run", "--app", B + r"\app.toml"], "R07-native.log", cwd=r"F:\ui4me\lab")
    x = wl.win(C, 30); hp = x["pid"]; ex(f"w.move({x['hwnd']}, 300, 60, 1100, 900)"); time.sleep(2); wl.wshot("final\\R07-broken-native.png")
    wl.click(*el("name")); key("End"); wl.type_(" still usable"); time.sleep(0.3); P("typing", repr(val("name")))
    for i in range(10):
        uia(f'''e = uia.window(title="{C}")
uia.pattern(uia.find(e, automation_id="second-window"), "Invoke").Invoke(); result = None'''); time.sleep(0.8)
        for s in wl.wins("Second document"):
            ex(f"w.post({s['hwnd']}, 0x10)")
        time.sleep(0.4)
    P("10 window cycles: windows", [y["title"] for y in wl.wins() if y["pid"] == hp])
    wl.click(*el("quit")); P("Quit exit after", wait_exit(hp), "s")
    r = ex(rf'''import os; result = w.run([os.path.expandvars(r"{HOST}"), "render", "--html", r"{B}\ui\index.html", "--output", r"{OUT}\R07.png", "--width", "1100", "--height", "900"], timeout=120)''')
    open("R07-broken-render.json", "w").write(r["out"]); P("render diagnostics codes", sorted(set(re.findall(r'"(DECODE_FAILED|FILE_NOT_FOUND)"', r["out"]))), "rc", r["rc"])

if not only or "N09" in only:
    P("## N09/N10 fault fixtures (native, installed host)")
    F = r"F:\ui4me\lab\out\final\faults"
    for m in ("veto-close", "veto-quit", "freeze-close", "freeze-quit", "log-flood", "managed-tree"):
        extra = ["--unread-host-output"] if m == "log-flood" else []
        r = ex(rf'''import os; result = w.run([r"{PY}", "tools/run_faults.py", "{m}", "--host", os.path.expandvars(r"{HOST}"), "--duration", "2", "--timeout", "60", "--output", r"{F}\{m}"] + {extra!r}, cwd=r"{SRC}", timeout=150)''')
        open(f"fault-{m}.txt", "w").write(r["out"] + r["err"]); P(m, "rc", r["rc"], r["out"].strip().splitlines()[-1][:200] if r["out"].strip() else r["err"][-200:])
    r = ex(rf'''import os; result = w.run([r"{PY}", "tools/run_faults.py", "managed-tree", "--host", os.path.expandvars(r"{HOST}"), "--kill-host", "--timeout", "60", "--output", r"{F}\managed-tree-kill-host"], cwd=r"{SRC}", timeout=150)''')
    open("fault-managed-tree-kill-host.txt", "w").write(r["out"] + r["err"])
    P("managed-tree --kill-host rc", r["rc"], "descendants", json.loads(r["out"][r["out"].index("{"):]).get("managed_descendants") if "{" in r["out"] else r["out"][-300:])
    spawn([PY, "tools/run_faults.py", "crash-after-ready", "--host", HOST, "--duration", "2", "--timeout", "60", "--output", F + r"\crash-after-ready"], "fault-crash-after-ready.txt")
    ws = []
    for i in range(40):
        ws = [y for y in wl.wins() if y["class"] == "Window Class" and "UI4ME" in y["title"]]
        if ws:
            break
        time.sleep(0.5)
    time.sleep(5); wl.wshot("final\\N09-crash-failure-ui.png")
    P("failure window", [y["title"] for y in ws])
    wl.activate(ws[0]["title"]); key("ctrl+q"); time.sleep(5)
    P("crash-after-ready output tail", logtext("fault-crash-after-ready.txt")[-400:])

if not only or "N11" in only:
    P("## N11 external app survives host exit")
    D = r"F:\ui4me\lab\probe-ext"
    ex(rf'''
import os, shutil
if os.path.exists(r"{D}"): shutil.rmtree(r"{D}")
shutil.copytree(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-host-reference"), r"{D}")
open(r"F:\ui4me\lab\files\external-open.txt", "w", encoding="utf-8").write("opened by UI4ME")
t = open(r"{D}\app.toml", encoding="utf-8").read()
t = t.replace('demo.py"]', 'demo.py", "--external-target", "F:\\\\ui4me\\\\lab\\\\files\\\\external-open.txt"]').replace(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-host-reference").replace("\\", "\\\\"), r"{D}".replace("\\", "\\\\")).replace("org.ui4me.HostReference", "org.ui4me.HostExternal")
open(r"{D}\app.toml", "w", encoding="utf-8").write(t)
result = t''')
    notepads = lambda: [(y["pid"], y["title"]) for y in wl.wins("Notepad")]
    for k, mode in enumerate(("quit", "kill")):
        for y in wl.wins("Notepad"):
            ex(f"w.post({y['hwnd']}, 0x10)")
        time.sleep(1)
        spawn([HOST, "run", "--app", D + r"\app.toml"], f"N11-run{k + 1}.log", cwd=r"F:\ui4me\lab")
        x = wl.win("Python reference", 30); hp = x["pid"]; time.sleep(1); wl.activate("Python reference"); key("F8"); time.sleep(4)
        np = notepads(); P(mode, "after F8 notepad", np, "parent of notepad", [ex(f"import psutil; result = psutil.Process({p}).ppid()") for p, _ in np])
        if k == 0:
            wl.wshot("final\\N11-external-open.png"); wl.activate("Python reference"); key("ctrl+q")
        else:
            ex(f"import subprocess; subprocess.run(['taskkill', '/PID', '{hp}', '/F'], capture_output=True)")
        time.sleep(3)
        P(mode, ": host alive", alive(hp), "| notepad", notepads(), "| probe backends", len([p for p in ex("result = w.procs()") if "probe-ext" in " ".join(p["cmdline"] or [])]))
    for y in wl.wins("Notepad"):
        ex(f"w.post({y['hwnd']}, 0x10)")
P("DONE")
