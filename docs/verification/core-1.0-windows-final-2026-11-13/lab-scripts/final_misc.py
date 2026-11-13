import json, time
import wl
from wl import P, ex
OUT = r"F:\ui4me\lab\out\final"
for tag, extra in (("renderer-system", ["--renderer", "--dom-cycles", "400"]), ("renderer-reduce", ["--renderer", "--reduced-motion", "reduce", "--dom-cycles", "400"]), ("probe", [])):
    r = ex(rf'''
import os
result = w.run([r"C:\Program Files\Python313\python.exe", r"F:\ui4me\src\tools\measure_resources.py", "--host", os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"), "--startup-runs", "5", "--idle", "60", "--cycles", "400", "--sample-every", "50", "--output", r"{OUT}\measure-{tag}.json"] + {extra!r}, cwd=r"F:\ui4me\src", timeout=1500)''')
    d = ex(rf'''import json; result = json.load(open(r"{OUT}\measure-{tag}.json"))''')
    s = d["sustained"]; st = sorted(x["startup_to_ready_ms"] for x in d["startup"]); ff = sorted(x["first_frame_since_host_start_ms"] for x in d["startup"])
    wc = [(x["cycles"], x["host"]["private_kib"]) for x in s["window_cycles"]]
    dc = [(x["dom_cycles"], x["host"]["private_kib"]) for x in s.get("dom_cycles", [])]
    P(tag, "rc", r["rc"], json.dumps({"startup_to_ready_ms_median": st[2], "first_frame_since_host_start_ms_median": ff[2],
        "idle_cpu_pct(GetProcessTimes)": [s["idle"]["host_cpu_percent"], s["idle"]["backend_cpu_percent"]],
        "memory_after_idle_private_kib": s["idle"]["memory_after_idle"]["combined"]["private_kib"],
        "host_private_kib_window_cycles": [wc[0][1], min(v for _, v in wc), max(v for _, v in wc), wc[-1][1]],
        "host_private_kib_dom_cycles": [dc[0][1], dc[-1][1]] if dc else None}))
r = ex(r'''
import os
result = w.run([r"F:\ui4me\qa-venv\Scripts\python.exe", r"F:\ui4me\src\tools\browser_reference.py", "--host", os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"), "--output", r"F:\ui4me\lab\out\final\browser"], cwd=r"F:\ui4me\src", env={"PLAYWRIGHT_BROWSERS_PATH": r"F:\ui4me\qa-browsers"}, timeout=1400)''')
P("R12 browser reference rc", r["rc"]); P(r["out"][-600:])
