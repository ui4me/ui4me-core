#!/usr/bin/env python3
"""Drive tests/native/Qualify.ps1 inside the interactive Windows RDP session.

Test tooling only. Runs on the Linux workstation:
- real input: xdotool on the Xvfb display that hosts the xfreerdp3 client, so
  Windows receives genuine RDP keyboard/mouse input in the user's session;
- observation and process launch: the lab agent running as the test user in that
  session (F:\\ui4me\\lab\\agent.py), reached through the admin SSH relay.

usage: drive_windows.py --edition pwsh|ps51 --out DIR [--mode full|auto|idle|bitmap-kill]
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

DISPLAY = os.environ.get("UI4ME_RDP_DISPLAY", ":88")
WIN_ROOT = r"F:\ui4me\ps"
KEYS = {"Tab": "Tab", "Enter": "Return", "Escape": "Escape"}


class Agent:
    def __init__(self):
        self.p = subprocess.Popen(
            ["ssh", "-o", "BatchMode=yes", "administrator@omegaui", r"F:\ui4me\lab\venv\Scripts\python.exe -u F:\ui4me\lab\relay.py"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")

    def __call__(self, code: str):
        self.p.stdin.write(json.dumps({"exec": code}) + "\n")
        self.p.stdin.flush()
        r = json.loads(self.p.stdout.readline())
        if not r["ok"]:
            raise RuntimeError(r["error"] + "\n" + r.get("trace", ""))
        return r["result"]


def xdo(*args):
    subprocess.run(["xdotool", *map(str, args)], env=dict(os.environ, DISPLAY=DISPLAY), check=True)


SCREEN = (1600, 1000)


def clamp(x, y):
    return min(max(int(round(x)), 0), SCREEN[0] - 1), min(max(int(round(y)), 0), SCREEN[1] - 1)


def click(x, y, button="left"):
    b = {"left": 1, "middle": 2, "right": 3}[button]
    xdo("mousemove", *clamp(x, y))
    time.sleep(0.08)
    xdo("click", b)
    time.sleep(0.08)


def drag(x0, y0, x1, y1, steps=25):
    xdo("mousemove", *clamp(x0, y0)); time.sleep(0.15)
    xdo("mousedown", 1); time.sleep(0.15)
    for i in range(1, steps + 1):
        xdo("mousemove", *clamp(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)); time.sleep(0.02)
    time.sleep(0.15)
    xdo("mouseup", 1); time.sleep(0.3)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--edition", choices=["pwsh", "ps51"], required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--mode", default="full")
    ap.add_argument("--timeout", type=float, default=900)
    a = ap.parse_args()
    out: Path = a.out
    out.mkdir(parents=True, exist_ok=True)
    agent = Agent()
    run_dir = WIN_ROOT + rf"\run\{a.edition}-{a.mode}"
    manifest = WIN_ROOT + rf"\src\powershell\tests\native\app-{a.edition}.toml"
    setup = agent(f"""
import shutil, glob
d = Path(r'{run_dir}')
if d.exists(): shutil.rmtree(d)
d.mkdir(parents=True)
home = Path(os.environ['USERPROFILE'])
mod = home / 'Documents' / ('PowerShell' if '{a.edition}' == 'pwsh' else 'WindowsPowerShell') / 'Modules' / 'UI4ME.Core' / '1.0.1'
hostexe = mod / 'runtimes' / 'win-x64' / 'ui4me_core' / '_bin' / 'ui4me-host.exe'
env = {{'UI4ME_PS_RESULTS': str(d), 'UI4ME_PS_MODE': '{a.mode}'}}
before = sorted(glob.glob(os.path.join(os.environ['TEMP'], 'ui4me-bitmap-*')))
t0 = time.time()
import subprocess
e = os.environ.copy(); e.update(env)
proc = subprocess.Popen([str(hostexe), 'run', '--app', r'{manifest}'], env=e, stdout=open(d / 'host.log', 'ab'), stderr=subprocess.STDOUT, creationflags=0x00000200)
if not hasattr(w, 'ps_hosts'): w.ps_hosts = {{}}
w.ps_hosts[proc.pid] = proc
pid = proc.pid
result = {{'pid': pid, 'host': str(hostexe), 'host_sha256': w.sha256(str(hostexe)), 'temp': os.environ['TEMP'], 'bitmaps_before': before}}
""")
    harness = {"edition": a.edition, "mode": a.mode, "setup": setup, "observations": []}
    host_pid = setup["pid"]
    harness_host_pid[0] = host_pid
    t0 = time.time()
    done, title = set(), None
    try:
        while time.time() - t0 < a.timeout:
            state = agent(f"""
import psutil
p = Path(r'{run_dir}') / 'results.jsonl'
lines = p.read_text(encoding='utf-8').splitlines() if p.exists() else []
result = {{'lines': lines, 'alive': psutil.pid_exists({host_pid}) and psutil.Process({host_pid}).status() != 'zombie'}}
""")
            recs = [json.loads(l) for l in state["lines"] if l.strip()]
            for r in recs:
                if r.get("kind") == "ready" and title is None:
                    title = r["title"]
                    harness["ready_s"] = round(time.time() - t0, 3)
                    win = agent(f"result = w.window({title!r}, timeout=15)")
                    harness["window_at_ready"] = win
                    # Activate with a real click on the window (SetForegroundWindow is blocked for jobs).
                    click(win["cx"] + 20, win["cy"] + 8)
                    time.sleep(0.5)
                    harness["taskbar_windows"] = agent(f"""
import psutil
pids = {{{host_pid}}} | {{c['pid'] for c in (w.tree({host_pid}) or [])}}
result = {{'tree': w.tree({host_pid}), 'visible_toplevel': [dict(title=x['title'], cls=x['class'], pid=x['pid'], owner=x['owner'], exstyle=x['exstyle']) for x in w.windows() if x['pid'] in pids]}}
""")
                    rx, ry, rw, rh = win["rect"]
                    agent(f"result = w.screenshot('ps-{a.edition}-{a.mode}-ready.png', bbox=({rx},{ry},{rx + rw},{ry + rh}))")
                    if a.mode == "idle":
                        harness["idle"] = measure_idle(agent, host_pid)
                        win = agent(f"result = w.window({title!r}, timeout=5)")
                        click(win["cx"] + 200, win["cy"] + 200)
                        xdo("key", "alt+F4")
                    if a.mode == "bitmap-kill":
                        time.sleep(3)
                        harness["kill"] = agent(f"""
import glob, psutil
files = sorted(glob.glob(os.path.join(os.environ['TEMP'], 'ui4me-bitmap-*')))
victims = [c['pid'] for c in (w.tree({host_pid}) or []) if c['name'].lower() in ('pwsh.exe', 'powershell.exe')]
for v in victims: psutil.Process(v).kill()
time.sleep(3)
result = {{'while_streaming': files, 'killed': victims, 'after_kill': sorted(glob.glob(os.path.join(os.environ['TEMP'], 'ui4me-bitmap-*')))}}
""")
                        win = agent(f"result = w.windows({title!r})")
                        harness["window_after_kill"] = win
                        if win:
                            click(win[0]["cx"] + 200, win[0]["cy"] + 200)
                            xdo("key", "alt+F4")
                if r.get("kind") != "request" or r["id"] in done:
                    continue
                done.add(r["id"])
                harness["observations"].append(perform(agent, r, run_dir, title))
            if not state["alive"]:
                break
            if [r for r in recs if r.get("kind") in ("exit",)] and time.time() - harness.setdefault("exit_seen", time.time()) > 15:
                win = agent(f"result = w.windows({title!r})") if title else []
                if win:
                    harness["closed_after_backend_exit"] = True
                    click(win[0]["cx"] + 200, win[0]["cy"] + 200)
                    xdo("key", "alt+F4")
                    harness["exit_seen"] = time.time() + 1e9
            time.sleep(0.3)
        else:
            harness["timeout"] = True
    finally:
        final = agent(f"""
import glob, psutil
code = None
time.sleep(1)
try:
    code = w.ps_hosts[{host_pid}].wait(20)
except Exception as error:
    code = repr(error)
d = Path(r'{run_dir}')
result = {{'host_exit': code, 'bitmaps_after': sorted(glob.glob(os.path.join(os.environ['TEMP'], 'ui4me-bitmap-*'))),
          'results': (d / 'results.jsonl').read_text(encoding='utf-8') if (d / 'results.jsonl').exists() else '',
          'host_log': (d / 'host.log').read_text(encoding='utf-8', errors='replace')[-6000:] if (d / 'host.log').exists() else '',
          'windows_left': w.windows({title!r}) if {title!r} else []}}
""")
        harness["final"] = {k: v for k, v in final.items() if k not in ("results",)}
        (out / "results.jsonl").write_text(final["results"], encoding="utf-8")
        (out / f"harness-{a.edition}-{a.mode}.json").write_text(json.dumps(harness, indent=2, ensure_ascii=False))
    recs = [json.loads(l) for l in final["results"].splitlines() if l.strip()]
    checks = [r for r in recs if r.get("kind") == "check"]
    failed = [c["name"] + ": " + c.get("error", "") for c in checks if c["status"] != "pass"]
    print(json.dumps({"edition": a.edition, "mode": a.mode, "checks": len(checks), "failed": failed,
                      "host_exit": final["host_exit"], "fatal": [r for r in recs if r.get("kind") == "fatal"],
                      "bitmaps_after": final["bitmaps_after"]}, indent=2, ensure_ascii=False))
    return 0 if not failed else 1


harness_host_pid = [0]


def measure_idle(agent, host_pid, seconds=20):
    return agent(f"""
import psutil
time.sleep(40)  # past startup JIT tiering and GC
pids = [{host_pid}] + [c['pid'] for c in (w.tree({host_pid}) or [])]
names = {{p: psutil.Process(p).name() for p in pids}}
c0 = {{p: w.cycles(p) for p in pids}}; t0 = time.time()
time.sleep({seconds})
c1 = {{p: w.cycles(p) for p in pids}}; dt = time.time() - t0
hz = w.cpu_hz()
result = {{f"{{names[p]}}:{{p}}": {{'cpu_percent': round((c1[p] - c0[p]) / hz / dt * 100, 2), 'private_mb': round(w.mem(p)['private'] / 2**20, 1)}} for p in pids}}
""")


def perform(agent, r, run_dir, title):
    action, rid = r["action"], r["id"]
    obs = {"request": r}
    reply = {}
    try:
        win = agent(f"result = w.window({title!r}, timeout=10)")
        cx, cy = win["cx"], win["cy"]
        if action in ("click", "clicks"):
            points = r["points"] if action == "clicks" else [r]
            for p in points:
                click(cx + p["x"], cy + p["y"], r.get("button", "left"))
            reply = {"done": True}
        elif action == "type":
            xdo("type", "--delay", "40", r["text"])
            reply = {"done": True}
        elif action == "key":
            xdo("key", KEYS[r["key"]])
            reply = {"done": True}
        elif action in ("dialog-accept", "dialog-observe"):
            d = None
            for _ in range(60):
                found = agent(f"result = [x for x in w.windows({r['title']!r}) if x['pid'] != 0]")
                if found:
                    d = found[0]
                    break
                time.sleep(0.5)
            obs["dialog"] = d
            if d and action == "dialog-accept":
                time.sleep(1.0)
                agent(f"result = w.screenshot('ps-dialog-save.png', bbox=({d['rect'][0]},{d['rect'][1]},{d['rect'][0]+d['rect'][2]},{d['rect'][1]+d['rect'][3]}))")
                xdo("key", "Return")
            reply = {"seen": bool(d), "caption": d["title"] if d else None, "owner": d["owner"] if d else None}
        elif action == "dialog-gone":
            gone = False
            for _ in range(30):
                if not agent(f"result = w.windows({r['title']!r})"):
                    gone = True
                    break
                time.sleep(0.5)
            reply = {"gone": gone}
        elif action == "observe-window":
            ok, last = False, None
            for _ in range(30):
                last = agent(f"result = w.windows({title!r}, visible=False)")[0]
                s = "minimized" if last["iconic"] else ("maximized" if last["zoomed"] else "normal")
                if s == r["expect"]:
                    ok = True
                    break
                time.sleep(0.3)
            reply = {"ok": ok, "window": last}
        elif action == "activate":
            click(cx + 20, cy + 8)
            reply = {"done": True}
        elif action == "drag":
            before = win
            x0, y0 = cx + r["x"], cy + r["y"]
            drag(x0, y0, x0 + r["dx"], y0 + r["dy"])
            time.sleep(0.6)
            after = agent(f"result = w.window({title!r}, timeout=5)")
            fb, fa = before["frame"], after["frame"]
            moved = (fa[0], fa[1]) != (fb[0], fb[1]) and (fa[2], fa[3]) == (fb[2], fb[3])
            resized = (fa[2], fa[3]) != (fb[2], fb[3])
            if r.get("edge") == "se":
                agent(f"result = w.screenshot('ps-frameless.png', bbox=({fa[0]},{fa[1]},{fa[0]+fa[2]},{fa[1]+fa[3]}))")
            reply = {"moved": moved, "resized": resized, "before": fb, "after": fa, "edge": r.get("edge")}
        elif action == "clipboard-read":
            reply = {"text": agent("result = w.clipboard_get()")}
        elif action == "clipboard-write":
            agent(f"w.clipboard_set({r['text']!r}); result = True")
            reply = {"done": True}
        elif action == "uia":
            pid = harness_host_pid[0]
            res = agent(f"""
log = Path(r'{WIN_ROOT}') / 'uia-probe.log'
r = w.run([r'F:\\ui4me\\lab\\venv\\Scripts\\python.exe', r'{WIN_ROOT}\\uia_com_probe.py', '--pid', '{pid}', '--automation-id', {r['automation_id']!r}, '--mode', 'document-text', '--log', str(log)], timeout=60)
result = {{'rc': r['rc'], 'log': log.read_text(encoding='utf-8') if log.exists() else r['err']}}
""")
            reply = res
        elif action == "ime-pinyin":
            before = agent("result = w.keyboard_layout()")
            xdo("key", "super+space"); time.sleep(1.0)
            during = agent("result = w.keyboard_layout()")
            xdo("type", "--delay", "120", r["text"]); time.sleep(0.6)
            xdo("key", "space"); time.sleep(0.8)
            xdo("key", "super+space"); time.sleep(1.0)
            after = agent("result = w.keyboard_layout()")
            reply = {"layout_before": before, "layout_during": during, "layout_after": after}
        elif action == "native-close":
            # Real Alt+F4 on the focused application window.
            click(cx + 200, cy + 15)
            xdo("key", "alt+F4")
            reply = {"done": True}
        else:
            reply = {"error": f"unknown action {action}"}
    except Exception as error:
        obs["error"] = repr(error)
        reply = {"error": repr(error)}
    line = json.dumps({"ack": rid, **reply}, ensure_ascii=False)
    agent(f"""
with open(Path(r'{run_dir}') / 'acks.jsonl', 'a', encoding='utf-8') as f:
    f.write({line!r} + '\\n')
result = True
""")
    obs["reply"] = reply
    return obs


if __name__ == "__main__":
    sys.exit(main())
