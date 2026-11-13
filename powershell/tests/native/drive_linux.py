#!/usr/bin/env python3
"""Drive tests/native/Qualify.ps1 on a real KDE Plasma Wayland session.

Test tooling only (Python is not part of the PowerShell runtime). Uses the
uinput daemon and KWin helpers from the 1.0 Linux lab
(docs/verification/core-1.0-linux-final-2026-11-13/lab-scripts): real pointer
and keyboard events go through the compositor, never synthetic DOM calls.

usage: drive_linux.py --host HOST --module PSD1 --out DIR [--mode full|auto|idle|bitmap-kill]
"""
from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "docs/verification/core-1.0-linux-final-2026-11-13/lab-scripts"))
import lab  # noqa: E402

KEYS = {"Tab": "KEY_TAB", "Enter": "KEY_ENTER", "Escape": "KEY_ESC"}


def records(path: Path):
    if not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            out.append(json.loads(line))
    return out


def ack(out: Path, rid: int, **fields):
    with (out / "acks.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps({"ack": rid, **fields}, ensure_ascii=False) + "\n")


def find(title_part: str):
    return lab.window(title_part)


def wait_for(pred, timeout=15.0, step=0.3):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return None


def app_processes(launcher_pid: int):
    """Host and backend PIDs. The host may re-exec itself inside a systemd user
    unit (containment), so look in that unit's cgroup as well as the child tree."""
    pids = set(proc_tree(launcher_pid))
    units = subprocess.run(["systemctl", "--user", "list-units", "--plain", "--no-legend", f"ui4me-host-*"],
                           capture_output=True, text=True).stdout.split()
    for unit in [u for u in units if u.startswith("ui4me-host-") and u.endswith(".service")]:
        cg = subprocess.run(["systemctl", "--user", "show", unit, "-p", "ControlGroup", "--value"],
                            capture_output=True, text=True).stdout.strip()
        procs = Path("/sys/fs/cgroup") / cg.lstrip("/") / "cgroup.procs"
        if cg and procs.exists():
            pids.update(int(p) for p in procs.read_text().split())
    out = {}
    for p in sorted(pids):
        try:
            out[p] = Path(f"/proc/{p}/exe").resolve().name
        except OSError:
            pass
    return out


def proc_tree(pid: int):
    try:
        out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    except OSError:
        return []
    kids = [int(p) for p in out]
    for k in list(kids):
        kids += proc_tree(k)
    return kids


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", required=True)
    ap.add_argument("--module", required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--mode", default="full")
    ap.add_argument("--manifest", default=str(Path(__file__).with_name("app-pwsh.toml")))
    ap.add_argument("--timeout", type=float, default=600)
    a = ap.parse_args()
    out: Path = a.out
    out.mkdir(parents=True, exist_ok=True)
    for name in ("results.jsonl", "acks.jsonl"):
        (out / name).unlink(missing_ok=True)
    env = dict(os.environ, UI4ME_PS_RESULTS=str(out), UI4ME_PS_MODULE=a.module, UI4ME_PS_MODE=a.mode)
    env.pop("PSGALLERY_API_KEY", None)
    env.pop("PYPI_API_KEY", None)
    log = (out / f"host-{a.mode}.log").open("w")
    t0 = time.time()
    host = subprocess.Popen([a.host, "run", "--app", a.manifest], env=env, stdout=log, stderr=subprocess.STDOUT)
    harness = {"mode": a.mode, "host": a.host, "manifest": a.manifest, "observations": []}
    done = set()
    exited_at = {}
    title = None
    results = out / "results.jsonl"
    try:
        while host.poll() is None and time.time() - t0 < a.timeout:
            recs = records(results)
            exited = [r for r in recs if r.get("kind") == "exit"]
            if exited and time.time() - exited_at.setdefault("t", time.time()) > 15 and title:
                # The backend ended but the host still shows a window (failure surface).
                harness["closed_after_backend_exit"] = True
                lab.kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }""" % json.dumps(title))
                exited_at["t"] = time.time() + 1e9
            for r in recs:
                if r.get("kind") == "ready" and title is None:
                    title = r["title"]
                    harness["ready_s"] = round(time.time() - t0, 3)
                    w = wait_for(lambda: find(title), 15)
                    harness["window_at_ready"] = w
                    lab.activate(title)
                    time.sleep(0.4)
                    lab.screenshot(out / f"ready-{a.mode}.png", region=(w["x"], w["y"], w["w"], w["h"]) if w else None)
                    if a.mode == "idle":
                        harness["idle"] = measure_idle(host.pid)
                        lab.kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }""" % json.dumps(title))
                    if a.mode == "bitmap-kill":
                        time.sleep(3)
                        harness["bitmap_files_while_streaming"] = bitmap_files()
                        pw = [p for p, name in app_processes(host.pid).items() if name == "pwsh"]
                        harness["killed_backend"] = pw
                        for k in pw:
                            os.kill(k, signal.SIGKILL)
                        time.sleep(2)
                        harness["bitmap_files_after_kill"] = bitmap_files()
                        harness["window_after_kill"] = find(title)
                        lab.kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }""" % json.dumps(title))
                if r.get("kind") != "request" or r["id"] in done:
                    continue
                done.add(r["id"])
                harness["observations"].append(perform(r, out, title))
            time.sleep(0.15)
        if host.poll() is None:
            harness["timeout"] = True
            host.send_signal(signal.SIGTERM)
        harness["host_exit"] = host.wait(30)
    finally:
        if host.poll() is None:
            host.kill()
        harness["total_s"] = round(time.time() - t0, 3)
        harness["bitmap_files_after_exit"] = bitmap_files()
        harness["windows_after_exit"] = [w for w in lab.windows() if title and title in w["caption"]]
        (out / f"harness-{a.mode}.json").write_text(json.dumps(harness, indent=2, ensure_ascii=False))
        log.close()
    recs = records(results)
    checks = [r for r in recs if r.get("kind") == "check"]
    failed = [c for c in checks if c["status"] != "pass"]
    print(json.dumps({"mode": a.mode, "checks": len(checks), "failed": [c["name"] + ": " + c.get("error", "") for c in failed],
                      "host_exit": harness.get("host_exit"), "fatal": [r for r in recs if r.get("kind") == "fatal"],
                      "bitmap_files_after_exit": harness["bitmap_files_after_exit"]}, indent=2, ensure_ascii=False))
    return 0 if not failed and harness.get("host_exit") == 0 and not harness.get("timeout") else 1


def bitmap_files():
    d = Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp"))
    return sorted(str(p) for p in d.glob("ui4me-bitmap-*"))


def measure_idle(host_pid: int, seconds: float = 20.0):
    time.sleep(40)  # past startup JIT tiering, GC and thread-pool trimming (~35 s)
    pids = list(app_processes(host_pid))
    clk = os.sysconf("SC_CLK_TCK")
    before = {p: lab.cpu_ticks(p) for p in pids if Path(f"/proc/{p}").exists()}
    time.sleep(seconds)
    after = {p: lab.cpu_ticks(p) for p in before if Path(f"/proc/{p}").exists()}
    res = {}
    for p, b in before.items():
        if p in after:
            name = Path(f"/proc/{p}/exe").resolve().name if Path(f"/proc/{p}/exe").exists() else str(p)
            res[f"{name}:{p}"] = {"cpu_percent": round((after[p] - b) / clk / seconds * 100, 2), "pss_kib": lab.pss_kib(p)}
    return res


def client_origin(title):
    w = find(title)
    return (w["cx"], w["cy"], w) if w else (None, None, None)


def perform(r, out: Path, title: str):
    action = r["action"]
    rid = r["id"]
    obs = {"request": r}
    try:
        if action in ("click", "clicks"):
            lab.activate(title)
            time.sleep(0.25)
            cx, cy, _ = client_origin(title)
            points = r["points"] if action == "clicks" else [r]
            for p in points:
                lab.click(cx + p["x"], cy + p["y"], r.get("button", "left"))
                time.sleep(0.05)
            ack(out, rid, done=True)
        elif action == "type":
            lab.type_text(r["text"])
            ack(out, rid, done=True)
        elif action == "key":
            lab.key(KEYS[r["key"]])
            ack(out, rid, done=True)
        elif action == "dialog-accept":
            w = wait_for(lambda: find(r["title"]), 30)
            obs["dialog"] = w
            if w:
                lab.activate(r["title"])
                time.sleep(0.8)
                lab.screenshot(out / "dialog-save.png", region=(w["x"], w["y"], w["w"], w["h"]))
                lab.key("KEY_ENTER")
            ack(out, rid, seen=bool(w), caption=w["caption"] if w else None)
        elif action == "dialog-observe":
            w = wait_for(lambda: find(r["title"]), 30)
            obs["dialog"] = w
            ack(out, rid, seen=bool(w), caption=w["caption"] if w else None)
        elif action == "dialog-gone":
            gone = wait_for(lambda: find(r["title"]) is None, 15) is not None
            ack(out, rid, gone=gone)
        elif action == "observe-window":
            want = r["expect"]
            def state():
                w = find(title)
                if not w:
                    return None
                s = "minimized" if w["minimized"] else ("maximized" if w["maximized"] == 3 else "normal")
                return w if s == want else None
            w = wait_for(state, 10)
            ack(out, rid, ok=bool(w), window=w)
        elif action == "drag":
            lab.activate(title)
            time.sleep(0.3)
            cx, cy, before = client_origin(title)
            x0, y0 = cx + r["x"], cy + r["y"]
            lab.drag(x0, y0, x0 + r["dx"], y0 + r["dy"], steps=25, dt=0.02)
            time.sleep(0.7)
            _, _, after = client_origin(title)
            moved = (after["x"], after["y"]) != (before["x"], before["y"]) and (after["w"], after["h"]) == (before["w"], before["h"])
            resized = (after["w"], after["h"]) != (before["w"], before["h"])
            if r.get("edge") == "se":
                lab.screenshot(out / "frameless.png", region=(after["x"], after["y"], after["w"], after["h"]))
            ack(out, rid, moved=moved, resized=resized, before=before, after=after, edge=r.get("edge"))
        elif action == "clipboard-read":
            text = subprocess.run(["wl-paste", "-n", "-t", "text/plain;charset=utf-8"], capture_output=True, timeout=10).stdout.decode("utf-8")
            ack(out, rid, text=text)
        elif action == "clipboard-write":
            # wl-copy forks a server process that owns the selection until replaced.
            subprocess.run(["wl-copy", "--", r["text"]], timeout=10, check=True)
            time.sleep(0.3)
            ack(out, rid, done=True)
        elif action == "activate":
            lab.activate(title)
            time.sleep(0.5)
            ack(out, rid, done=True)
        elif action == "native-close":
            lab.kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }""" % json.dumps(title))
            ack(out, rid, done=True)
        else:
            ack(out, rid, error=f"unknown action {action}")
    except Exception as error:  # report to the backend; it will fail the check
        obs["error"] = repr(error)
        ack(out, rid, error=repr(error))
    return obs


if __name__ == "__main__":
    sys.exit(main())
