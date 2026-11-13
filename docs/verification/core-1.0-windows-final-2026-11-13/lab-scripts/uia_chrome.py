import time, wl
from wl import P, ex
OUT = r"F:\ui4me\lab\out\final\uia"
ex(r'''w.spawn([r"C:\Program Files\Google\Chrome\Application\chrome.exe", "--force-renderer-accessibility", "--no-first-run", "--user-data-dir=F:\\ui4me\\lab\\chrome-profile", r"F:\ui4me\lab\out\final\uia\field.html"])''')
x = wl.win("UIA field", 30); time.sleep(8)
wl.click(x["cx"] + 60, x["cy"] + 120); time.sleep(3)
r = ex(rf'''
result = w.run([r"F:\ui4me\lab\venv\Scripts\python.exe", r"F:\ui4me\lab\uia\uia_probe_matrix.py", "--pid", "{x['pid']}", "--title", "UIA field", "--automation-id", "name", "--client", "managed", "--client", "com", "--timeout", "20", "--output", r"{OUT}\chrome"], timeout=1400)''')
P(f"## chrome (pid {x['pid']}) rc {r['rc']}"); P(r["out"])
for y in wl.wins("UIA field"):
    ex(f"w.post({y['hwnd']}, 0x10)")
