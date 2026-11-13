"""Write a per-check evidence JSON and record it in the matching ledger."""
import hashlib, json, os, platform, subprocess, sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path("/home/toor/projects/ui4me-core")
FINAL = REPO / "verification/core-1.0-linux-final"
EV = FINAL / "evidence"
HOST = Path.home() / ".cargo/bin/ui4me-host"
SUITES = {"N": ("native_acceptance.py", "ledger-native"), "R": ("renderer_acceptance.py", "ledger-renderer"),
          "C": ("completion_acceptance.py", "ledger-completion")}


def identity():
    sys.path.insert(0, str(REPO / "tools"))
    import native_acceptance
    return {"host": str(HOST), "host_sha256": hashlib.sha256(HOST.read_bytes()).hexdigest(),
            "host_version": subprocess.check_output([str(HOST), "--version"], text=True).strip(),
            "source_sha256": native_acceptance.source_identity(),
            "base_commit": subprocess.check_output(["git", "-C", str(REPO), "rev-parse", "--short", "HEAD"], text=True).strip(),
            "os": platform.platform(), "desktop": os.environ.get("XDG_CURRENT_DESKTOP"),
            "session": os.environ.get("XDG_SESSION_TYPE"), "user": os.environ.get("USER")}


def record(check, status, actions, expected, observed, files=(), limitations=(), note=""):
    EV.mkdir(parents=True, exist_ok=True)
    files = [Path(f) if Path(f).is_absolute() else EV / f for f in files]
    doc = {"check": check, "status": status, "recorded_utc": datetime.now(timezone.utc).isoformat(),
           "build_identity": identity(), "actions": list(actions), "expected": expected,
           "observed": observed, "limitations": list(limitations),
           "evidence_files_sha256": {str(f.relative_to(EV) if f.is_relative_to(EV) else f):
                                     hashlib.sha256(f.read_bytes()).hexdigest() for f in files}}
    path = EV / f"{check}.json"
    path.write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    tool, ledger = SUITES[check[0]]
    r = subprocess.run([sys.executable, str(REPO / "tools" / tool), "--output", str(FINAL / ledger),
                        "--record", check, "--status", status, "--evidence", str(path),
                        "--note", note or observed[:300]], cwd=REPO, capture_output=True, text=True)
    if r.returncode not in (0, 1, 2):
        raise SystemExit(r.stderr)
    q = json.loads(r.stdout)["checks"][check]["status"]
    print(f"{check}: recorded {q}")
    return q
