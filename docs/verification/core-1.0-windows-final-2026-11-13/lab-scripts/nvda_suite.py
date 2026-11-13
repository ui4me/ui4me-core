"""R05 on Windows: NVDA speech journey through the installed reference app (real keyboard)."""
import re, time
import wl
from wl import P, key, uia, ex
from startmenu import start_launch

C = "Desktop workspace"
LOG = r"F:\ui4me\lab\out\final\R05-nvda.log"


def el(i):
    return uia(f'''e = uia.window(title="{C}")
r = uia.find(e, automation_id="{i}").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')


def speech(since_lines=0):
    t = ex(rf'''result = open(r"{LOG}", encoding="utf-8", errors="replace").read() if __import__("os").path.exists(r"{LOG}") else ""''')
    lines = [l for l in t.splitlines() if l.strip()]
    out = []
    for i, l in enumerate(lines):
        if "Speaking" in l or "speech.speech.speak" in l:
            nxt = lines[i + 1] if i + 1 < len(lines) else ""
            out.append(nxt.strip() if "Speaking" in l and not re.search(r"\[.+\]", l) else l.split("Speaking", 1)[-1].strip())
    return out[since_lines:]


for x in wl.wins(C):
    ex(f"w.post({x['hwnd']}, 0x10)")
time.sleep(2)
ex(rf'''
import os, subprocess
if os.path.exists(r"{LOG}"): os.remove(r"{LOG}")
subprocess.Popen([r"F:\ui4me\qa-tools\nvda\nvda.exe", "--config-path=F:\\ui4me\\qa-tools\\nvda-config", "--log-level=12", r"--log-file={LOG}"])''')
time.sleep(10)
P("NVDA running:", bool([p for p in ex("import psutil; result = [p.name() for p in psutil.process_iter(['name'])]") if p.lower() == "nvda.exe"]))
start_launch("UI4ME Desktop Workspace")
x = wl.win(C, 30); ex(f"w.move({x['hwnd']}, 200, 20, 1200, 960)"); time.sleep(3)
n0 = len(speech())
wl.click(*el("name")); time.sleep(2.5); a = speech(n0); P("click Name ->", a); n0 += len(a)
key("Tab"); time.sleep(2.5); a = speech(n0); P("Tab ->", a); n0 += len(a)
key("Down"); time.sleep(2.5); a = speech(n0); P("Down (Priority) ->", a); n0 += len(a)
key("Tab"); time.sleep(2.5); a = speech(n0); P("Tab ->", a); n0 += len(a)
wl.type_("hello"); time.sleep(2); a = speech(n0); P("type 'hello' in Notes ->", a); n0 += len(a)
key("Tab", "Tab", "Tab"); time.sleep(2.5); a = speech(n0); P("Tab x3 ->", a); n0 += len(a)
key("space"); time.sleep(2.5); a = speech(n0); P("Space on checkbox ->", a); n0 += len(a)
uia(f'''e = uia.window(title="{C}")
uia.pattern(uia.find(e, automation_id="add-row"), "Invoke").Invoke(); result = None'''); time.sleep(3)
a = speech(n0); P("Add row (live status) ->", a); n0 += len(a)
key("Insert+Down"); time.sleep(6); key("control"); a = speech(n0); P("NVDA say-all (Insert+Down) ->", a[:25]); n0 += len(a)
wl.wshot("final\\R05-nvda.png")
ex(r'''import subprocess; subprocess.run([r"F:\ui4me\qa-tools\nvda\nvda.exe", "-q"], capture_output=True)''')
time.sleep(4)
P("NVDA running after quit:", bool([p for p in ex("import psutil; result = [p.name() for p in psutil.process_iter(['name'])]") if p.lower() == "nvda.exe"]))
x = wl.win(C); ex(f"w.post({x['hwnd']}, 0x10)")
P("DONE")
