#!/usr/bin/env python3
"""Run one adversarial scenario against a headless ui4me-host and summarize."""
import json
import os
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
HOST = os.environ.get("ADV_HOST", str(HERE / "ui4me-host"))


def run(scenario, timeout=90, startup_ms=15000, shutdown_ms=5000, extra_env=None):
    out = HERE / "runs" / f"{scenario}-{uuid.uuid4().hex[:6]}"
    out.mkdir(parents=True)
    evidence = out / "evidence.jsonl"
    manifest = out / "app.toml"
    app_id = "org.ui4me.Adv" + uuid.uuid4().hex[:8]
    manifest.write_text("\n".join([
        "[app]", f'id = "{app_id}"', 'name = "Adv"', "",
        "[backend]", f'program = "{sys.executable}"',
        f'args = {json.dumps([str(HERE / "adv_backend.py"), scenario, str(evidence)])}',
        f'cwd = "{HERE}"', "", "[host]", f"startup_timeout_ms = {startup_ms}",
        f"shutdown_timeout_ms = {shutdown_ms}", "max_windows = 64", f'instance = "{os.environ.get("ADV_INSTANCE","single")}"', ""]))
    env = dict(os.environ)
    env.update(extra_env or {})
    env["RUST_LOG"] = env.get("RUST_LOG", "ui4me_host=info")
    start = time.monotonic()
    with open(out / "host.log", "wb") as log:
        p = subprocess.Popen([HOST, "run", "--app", str(manifest), "--headless"],
                             stdout=log, stderr=subprocess.STDOUT, env=env, cwd=HERE)
        try:
            code = p.wait(timeout)
        except subprocess.TimeoutExpired:
            p.kill()
            code = "TIMEOUT"
    elapsed = round(time.monotonic() - start, 2)
    print(f"== {scenario}: host exit={code} after {elapsed}s ({out})")
    if evidence.exists():
        for line in evidence.read_text().splitlines():
            print("  ", line)
    tail = (out / "host.log").read_text(errors="replace").splitlines()
    interesting = [l for l in tail if '"level":"WARN"' in l or '"level":"ERROR"' in l]
    for l in interesting[-12:]:
        try:
            j = json.loads(l)
            print("   host:", j.get("level"), json.dumps(j.get("fields")))
        except ValueError:
            print("   host:", l[:300])
    for l in tail:
        if "panicked" in l:
            print("   PANIC:", l[:400])
    return code


if __name__ == "__main__":
    scen = sys.argv[1]
    extra = {}
    for a in sys.argv[2:]:
        k, v = a.split("=", 1)
        extra[k] = v
    t = int(extra.pop("TIMEOUT", "90"))
    s_ms = int(extra.pop("STARTUP_MS", "15000"))
    run(scen, timeout=t, startup_ms=s_ms, extra_env=extra)
