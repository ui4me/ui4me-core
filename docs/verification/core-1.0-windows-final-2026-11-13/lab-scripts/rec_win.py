"""Write a per-check Windows evidence JSON (in the session) and record it in the ledger.

Evidence files listed by name are hashed on Windows (paths under EV) or uploaded
from the Linux driver transcripts directory first.
"""
import base64, json, os, subprocess, sys
import wl

FINAL = r"F:\ui4me\verification\core-1.0-windows-final"
EV = FINAL + r"\evidence"
SUITES = {"N": ("native_acceptance.py", "ledger-native"), "R": ("renderer_acceptance.py", "ledger-renderer"),
          "C": ("completion_acceptance.py", "ledger-completion")}
LOCAL = os.path.join(os.path.dirname(os.path.abspath(__file__)), "transcripts")


def upload(name):
    """Copy a Linux-side transcript into the Windows evidence directory."""
    src = os.path.join(LOCAL, name)
    subprocess.run(["scp", "-q", src, "administrator@omegaui:" + EV.replace("\\", "/") + "/" + name], check=True)


def record(check, status, actions, expected, observed, files=(), limitations=(), note=""):
    for f in files:
        if os.path.exists(os.path.join(LOCAL, f)):
            upload(f)
    doc = {"check": check, "status": status, "actions": list(actions), "expected": expected,
           "observed": observed, "limitations": list(limitations), "files": list(files), "note": note or observed[:300]}
    tool, ledger = SUITES[check[0]]
    r = wl.ex(rf'''
import json, os, platform, subprocess, hashlib, sys
from datetime import datetime, timezone
import base64
doc = json.loads(base64.b64decode({base64.b64encode(json.dumps(doc, ensure_ascii=False).encode("utf-8")).decode()!r}).decode("utf-8"))
ev = r"{EV}"
os.makedirs(ev, exist_ok=True)
host = os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe")
sys.path.insert(0, r"F:\ui4me\src\tools")
import native_acceptance
ident = {{"host": host, "host_sha256": w.sha256(host),
          "host_version": subprocess.run([host, "--version"], capture_output=True, text=True).stdout.strip(),
          "source_sha256": native_acceptance.source_identity(), "base_commit": open(r"F:\ui4me\SOURCE_TREE.local").read().strip(),
          "os": platform.platform(), "release": platform.release(), "version": platform.version(),
          "session": w.session_id(), "user": os.environ.get("USERNAME"), "desktop": "Windows interactive RDP session (xfreerdp3 client), 1600x1000, 96 DPI unless stated"}}
files = {{}}
for f in doc.pop("files"):
    p = f if os.path.isabs(f) else os.path.join(ev, f)
    files[os.path.relpath(p, ev) if p.startswith(ev) else p] = w.sha256(p)
note = doc.pop("note")
out = {{"check": doc["check"], "status": doc["status"], "recorded_utc": datetime.now(timezone.utc).isoformat(),
        "build_identity": ident, **{{k: doc[k] for k in ("actions", "expected", "observed", "limitations")}},
        "evidence_files_sha256": files}}
path = os.path.join(ev, doc["check"] + ".json")
open(path, "w", encoding="utf-8").write(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
r = subprocess.run([r"C:\Program Files\Python313\python.exe", r"F:\ui4me\src\tools\{tool}", "--output", r"{FINAL}\{ledger}",
                    "--record", doc["check"], "--status", doc["status"], "--evidence", path, "--note", note],
                   cwd=r"F:\ui4me\src", capture_output=True, text=True)
led = json.load(open(os.path.join(r"{FINAL}\{ledger}", "native-acceptance.json" if "{ledger}" == "ledger-native" else ("renderer-acceptance.json" if "{ledger}" == "ledger-renderer" else "completion-acceptance.json")), encoding="utf-8")) if False else None
import glob
lf = [f for f in glob.glob(os.path.join(r"{FINAL}\{ledger}", "*.json")) if not f.endswith("backend.json")]
st = json.load(open(lf[0], encoding="utf-8"))["checks"][doc["check"]]["status"] if lf else None
result = [r.returncode, st, r.stderr[-2000:], lf]''')
    rc, out, err, _ = r
    if rc not in (0, 1, 2):
        raise SystemExit(f"{check}: record failed {rc}: {err}")
    status_now = out
    print(f"{check}: recorded {status_now}", flush=True)
    return status_now


if __name__ == "__main__":
    print(json.dumps(sys.argv))
