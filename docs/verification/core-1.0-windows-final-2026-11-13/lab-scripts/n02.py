import time, wl
from wl import P, ex, key
from startmenu import start_launch
ex(r'''import os; w.spawn([os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"), "run", "--app", os.path.expandvars(r"%LOCALAPPDATA%\ui4me-host-reference\app.toml")], cwd=os.path.expanduser("~"), stdout=r"F:\ui4me\lab\out\final\N02-first-instance.log")''')
wl.win("Python reference", 30); time.sleep(2)
procs = lambda: [(p["pid"], p["name"]) for p in ex("result = w.procs()") if p["name"] == "ui4me-host.exe" or "host-reference" in " ".join(p["cmdline"] or [])]
P("before second launch", procs())
start_launch("UI4ME Host Reference"); time.sleep(3)
P("after Start Menu second launch", procs(), "| windows", [x["title"] for x in wl.wins("UI4ME")])
log = ex(r'''result = open(r"F:\ui4me\lab\out\final\N02-first-instance.log", encoding="utf-8", errors="replace").read()''')
P("first instance received:", [l[:220] for l in log.splitlines() if "Second launch" in l])
wl.activate("Python reference"); key("ctrl+q"); time.sleep(3); P("after quit", procs())
