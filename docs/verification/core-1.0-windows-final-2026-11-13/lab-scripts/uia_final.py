import json, time
import wl
from wl import P, ex
OUT = r"F:\ui4me\lab\out\final\uia"
ex(r'''
import os
from pathlib import Path
os.makedirs(r"F:\ui4me\lab\out\final\uia", exist_ok=True)
Path(r"F:\ui4me\lab\out\final\uia\np.txt").write_text("A useful desktop tool")
Path(r"F:\ui4me\lab\out\final\uia\field.html").write_text("<title>UIA field</title><input id=name value='A useful desktop tool' autofocus>")
w.spawn([os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"), "run", "--app", os.path.expandvars(r"%LOCALAPPDATA%\ui4me-renderer-reference\app.toml")], cwd=os.path.expanduser("~"))
w.spawn([r"C:\Program Files\Windows NT\Accessories\wordpad.exe", r"F:\ui4me\lab\out\final\uia\np.txt"])
w.spawn([r"C:\Program Files\Google\Chrome\Application\chrome.exe", "--force-renderer-accessibility", "--no-first-run", "--user-data-dir=F:\\ui4me\\lab\\chrome-profile", r"F:\ui4me\lab\out\final\uia\field.html"])''')
time.sleep(10)
targets = [("ui4me", "Desktop workspace", ["--automation-id", "name"]), ("wordpad", "np - WordPad", ["--first-edit"]), ("chrome", "UIA field", ["--automation-id", "name"])]
summary = {}
for name, title, sel in targets:
    x = wl.win(title, 20)
    r = ex(rf'''
result = w.run([r"F:\ui4me\lab\venv\Scripts\python.exe", r"F:\ui4me\lab\uia\uia_probe_matrix.py", "--pid", "{x['pid']}", "--title", {title.split(" - ")[0] if name == "wordpad" else title!r}, *{sel!r}, "--client", "managed", "--client", "com", "--timeout", "20", "--output", r"{OUT}\{name}"], timeout=1400)''')
    P(f"## {name} (pid {x['pid']}) rc {r['rc']}"); P(r["out"])
    rep = ex(rf'''import json; result = json.load(open(r"{OUT}\{name}\report.json"))''')
    summary[name] = {f"{row['client']}-{row['apartment']}-{row['mode']}": row["status"] for row in rep["results"]}
    if name != "wordpad":
        err = ex(rf'''
import glob
out = {{}}
for f in glob.glob(r"{OUT}\{name}\*selection-text-process.log") + glob.glob(r"{OUT}\{name}\*select-read-process.log"):
    t = open(f, encoding="utf-8", errors="replace").read()
    out[f.rsplit("\\", 1)[1]] = [l for l in t.splitlines() if "Exception" in l][:1]
result = out''')
        P("managed-client exceptions:", err)
P("## original 0.2 reproduction (managed client in Windows PowerShell), UI4ME")
r = ex(r'''
import subprocess
out = []
for step in (1, 2, 3, 4, 5):
    for ps in ("-STA", "-MTA"):
        try:
            p = subprocess.run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", ps, "-File", r"F:\ui4me\lab\uia\repro.ps1", "-Step", str(step)], capture_output=True, timeout=30, creationflags=0x08000000)
            lines = p.stdout.decode("utf-8", "replace").strip().splitlines()
            err = [l for l in p.stderr.decode("utf-8", "replace").splitlines() if "Exception" in l][:1]
            out.append(f"step {step} {ps} rc={p.returncode:#x} last={lines[-1] if lines else None!r} err={err}")
        except subprocess.TimeoutExpired:
            out.append(f"step {step} {ps} TIMEOUT")
result = out''')
[P(l) for l in r]
P("SUMMARY", json.dumps(summary, indent=1))
ex('''
for t in ("Desktop workspace", "WordPad", "UIA field"):
    for x in w.windows(t): w.post(x["hwnd"], 0x10)''')
