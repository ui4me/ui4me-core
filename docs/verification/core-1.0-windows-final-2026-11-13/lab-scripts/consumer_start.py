import time, wl
from wl import P, ex
from startmenu import start_launch
ex(r'''import os; c = os.path.expandvars(r"%LOCALAPPDATA%\ui4me-consumer\app.toml"); open(c, "a").close()''')
ex("import os; os.environ['CONSUMER_HOLD'] = '6'")
start_launch("Independent UI4ME Consumer", "final\\consumer-start-entry.png")
x = wl.win("Independent", 30); time.sleep(1.5)
procs = ex('''
import psutil
out = []
for p in psutil.process_iter(["pid", "ppid", "name", "exe"]):
    if p.info["name"] in ("ui4me-host.exe", "python.exe") and p.info["exe"] and "ui4me-wheel" in p.info["exe"]:
        out.append({**p.info, "parent": psutil.Process(p.info["ppid"]).name()})
result = out''')
P("window", x["title"], x["class"], "pid", x["pid"]); [P("  proc", p) for p in procs]
P("console windows in app tree", [(y["title"], y["class"]) for y in wl.wins() if y["pid"] in [p["pid"] for p in procs] and y["class"] == "ConsoleWindowClass"])
P("taskbar", wl.uia('''
a = uia.A(); tb = a.GetRootElement().FindFirst(uia.U.TreeScope_Children, a.CreatePropertyCondition(uia.U.UIA_ClassNamePropertyId, "Shell_TrayWnd"))
result = [d["name"] for d in uia.dump(tb, depth=8, max_nodes=200) if d["type"] == "Button" and d["name"] and "Consumer" in d["name"]]'''))
wl.wshot("final\\consumer-running.png")
time.sleep(8)
P("report", ex(r'''import os, json; result = json.load(open(os.path.expandvars(r"%LOCALAPPDATA%\ui4me-consumer\consumer-report.json")))'''))
