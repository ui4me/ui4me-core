#!/usr/bin/env python3
"""Native qualification of direct startup (`python app.py`, `uv run app.py`, `pwsh -File app.ps1`).

Test tooling only. Runs a small app through its normal launcher on a real desktop,
presses its button with real pointer input, closes it like a user, and checks exit
status, the callbacks' observations and that no host process is left behind.

Linux (KDE Plasma, Wayland): real input through tools/lab/inputd.py (root uinput
daemon), window geometry from KWin. Windows: the lab agent inside the interactive RDP
session launches and observes; real input is xdotool on the Xvfb display that hosts
the xfreerdp3 client (see powershell/tests/native/drive_windows.py).

usage:
  native_direct_start.py [--platform linux|windows] --launcher python|uv|pwsh|powershell --workdir DIR
                         --out DIR [--module PSD1] [--scenario ...] --command CMD [ARG...]
  CMD is the launcher prefix, for example: /venv/bin/python   or   uv run   or   pwsh -NoProfile -File
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
PAGE = ('<body style="margin:0;font-family:sans-serif">'
        '<button id="press" style="position:absolute;left:20px;top:20px;width:300px;height:120px;font-size:24px">'
        'Press</button><p id="status" style="position:absolute;left:20px;top:160px">idle</p></body>')
BUTTON = (170, 80)  # client coordinates of the button centre

PYTHON_APP = '''import json, os, sys, time
RESULTS = os.environ["UI4ME_DIRECT_RESULTS"]
def record(**fields):
    with open(RESULTS, "a", encoding="utf-8") as f:
        f.write(json.dumps({**fields, "pid": os.getpid(), "t": time.time()}, ensure_ascii=False) + "\\n")
record(event="module", role="backend" if os.environ.get("UI4ME_ENDPOINT") else "launcher")
from ui4me_core import Application
TITLE = os.environ["UI4ME_DIRECT_TITLE"]
PAGE = %r
def main():
    with Application.start(name=TITLE, app_id="org.ui4me.qualification.DirectStart") as app:
        window = app.open_html(PAGE, title=TITLE, width=480, height=320)
        document = window.document
        count = 0
        def pressed(event):
            nonlocal count
            count += 1
            document.set_text("status", f"pressed {count}")
            record(event="click", count=count, text=document.get_text("status"))
            if "--crash" in sys.argv:
                os._exit(3)
        document.on("click", pressed, element="press")
        record(event="ready", argv=sys.argv[1:], secret_visible="UI4ME_TOKEN" in os.environ,
               executable=sys.executable)
        app.run()
        record(event="orderly")
if __name__ == "__main__":
    main()
''' % PAGE

POWERSHELL_APP = '''param([string]$First, [string]$Second, [switch]$Crash)
$ErrorActionPreference = 'Stop'
if ($env:UI4ME_DIRECT_MODULE) { Import-Module $env:UI4ME_DIRECT_MODULE } else { Import-Module UI4ME.Core }
function Write-Record([hashtable]$Fields) {
    $Fields.pid = $PID; $Fields.t = [DateTime]::UtcNow.ToString('o')
    [IO.File]::AppendAllText($env:UI4ME_DIRECT_RESULTS, ((ConvertTo-Json $Fields -Compress) + "`n"), (New-Object Text.UTF8Encoding($false)))
}
Write-Record @{ event = 'module'; role = $(if ($env:UI4ME_ENDPOINT) { 'backend' } else { 'launcher' }) }
$app = Start-UI4MEScript -Name $env:UI4ME_DIRECT_TITLE -AppId 'org.ui4me.qualification.DirectStartPS'
$window = New-UI4MEWindow -Application $app -Title $env:UI4ME_DIRECT_TITLE -Width 480 -Height 320
$doc = Open-UI4MEHtml -Window $window -Html '%s'
$script:count = 0
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element press -Callback {
    $script:count++
    Set-UI4MEDocumentText -Document $doc -Element status -Text "pressed $($script:count)"
    Write-Record @{ event = 'click'; count = $script:count; text = (Get-UI4MEDocumentText -Document $doc -Element status) }
    if ($Crash) { [Environment]::Exit(3) }
}
Write-Record @{ event = 'ready'; argv = @($First, $Second); secret_visible = [bool]$env:UI4ME_TOKEN; edition = $PSVersionTable.PSEdition; version = $PSVersionTable.PSVersion.ToString() }
Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Write-Record @{ event = 'orderly' }
Close-UI4ME -Application $app
''' % PAGE.replace("'", "''")

ARGS = ["a b", "Ünï 😀"]


def records(text: str) -> list[dict]:
    return [json.loads(line) for line in text.splitlines() if line.strip()]


class Linux:
    """Local KDE Plasma Wayland session."""

    def __init__(self):
        sys.path.insert(0, str(ROOT / "tools/lab"))
        import lab
        self.lab = lab

    def prepare(self, folder, name: str, body: str):
        folder = Path(folder)
        folder.mkdir(parents=True, exist_ok=True)
        (folder / name).write_text(body, encoding="utf-8-sig" if name.endswith(".ps1") else "utf-8")
        return str(folder)

    def launch(self, command, folder, env):
        out = open(Path(folder) / f"launcher-{uuid.uuid4().hex[:6]}.log", "wb")
        process = subprocess.Popen(command, cwd=folder, env={**os.environ, **env}, stdout=out,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        process.log = out.name
        return process

    def poll(self, process):
        return process.poll()

    def output(self, process):
        return Path(process.log).read_text(encoding="utf-8", errors="replace")

    def results(self, path):
        p = Path(path)
        return records(p.read_text(encoding="utf-8")) if p.exists() else []

    def window(self, title):
        return self.lab.window(title)

    def click(self, title, x, y):
        # Other windows may cover the new one: make sure it is really active (on top)
        # before pressing, as a user would bring it to the front first.
        for _ in range(10):
            self.lab.activate(title)
            time.sleep(0.4)
            w = self.lab.window(title)
            if w and w["active"]:
                break
        else:
            raise RuntimeError(f"cannot activate {title!r}: {w}")
        target = (w["cx"] + x, w["cy"] + y)
        for _ in range(20):
            # Press only when the pointer is really on the target of an active test
            # window: a person may be using the same desktop and moving the mouse.
            # The kernel drops an unchanged absolute position: step next to the target
            # first, so the pointer really moves even if another device moved it away.
            self.lab.move(target[0] + 3, target[1] + 3)
            time.sleep(0.05)
            self.lab.move(*target)
            time.sleep(0.15)
            pos = json.loads(self.lab.kwin_script("OUT(JSON.stringify(workspace.cursorPos));", wait=0.3)[-1])
            current = self.lab.window(title)
            if abs(pos["x"] - target[0]) <= 2 and abs(pos["y"] - target[1]) <= 2 and current and current["active"]:
                self.lab.button("left", True)
                time.sleep(0.05)
                self.lab.button("left", False)
                return
            self.lab.activate(title)
            time.sleep(0.5)
        raise RuntimeError("pointer did not stay on the test window (desktop in use?)")

    def close(self, title):
        self.lab.kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }""" % json.dumps(title))

    def interrupt(self, process):
        os.killpg(process.pid, signal.SIGINT)  # what a terminal Ctrl+C does

    def alive(self, pid):
        return Path(f"/proc/{pid}").exists()

    def leftovers(self):
        hosts = []
        for proc in Path("/proc").iterdir():
            if proc.name.isdigit():
                try:
                    if os.readlink(proc / "exe").endswith("/ui4me-host"):
                        hosts.append(int(proc.name))
                except OSError:
                    pass
        units = subprocess.run(["systemctl", "--user", "list-units", "--plain", "--no-legend", "ui4me-host-*"],
                               capture_output=True, text=True).stdout.split()
        return {"hosts": hosts, "units": [u for u in units if u.startswith("ui4me-host-")]}


class Windows:
    """The interactive ui4me_test RDP session, through the lab agent."""

    def __init__(self, display=":88"):
        self.display = display
        self.agent = subprocess.Popen(
            ["ssh", "-o", "BatchMode=yes", "administrator@omegaui",
             r"F:\ui4me\lab\venv\Scripts\python.exe -u F:\ui4me\lab\relay.py"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
        self.procs = {}

    def __call__(self, code):
        self.agent.stdin.write(json.dumps({"exec": code}) + "\n")
        self.agent.stdin.flush()
        reply = json.loads(self.agent.stdout.readline())
        if not reply["ok"]:
            raise RuntimeError(reply["error"] + "\n" + reply.get("trace", ""))
        return reply["result"]

    def prepare(self, folder, name, body):
        encoding = "utf-8-sig" if name.endswith(".ps1") else "utf-8"
        return self(f"""
d = Path({str(folder)!r}); d.mkdir(parents=True, exist_ok=True)
(d / {name!r}).write_text({body!r}, encoding={encoding!r})
result = str(d)""")

    def launch(self, command, folder, env):
        token = uuid.uuid4().hex[:6]
        pid = self(f"""
import subprocess
e = os.environ.copy(); e.update({env!r})
log = open(Path({folder!r}) / 'launcher-{token}.log', 'wb')
p = subprocess.Popen({command!r}, cwd={folder!r}, env=e, stdout=log, stderr=subprocess.STDOUT,
                     creationflags=0x00000200 | 0x08000000)
if not hasattr(w, 'direct'): w.direct = {{}}
w.direct[p.pid] = (p, log.name)
result = p.pid""")
        return pid

    def poll(self, pid):
        return self(f"p, _ = w.direct[{pid}]\nresult = p.poll()")

    def output(self, pid):
        return self(f"_, log = w.direct[{pid}]\nresult = Path(log).read_text(encoding='utf-8', errors='replace')")

    def results(self, path):
        text = self(f"p = Path({path!r})\nresult = p.read_text(encoding='utf-8') if p.exists() else ''")
        return records(text)

    def window(self, title):
        found = self(f"m = [x for x in w.windows() if {title!r} in x['title']]\nresult = m[0] if m else None")
        if found:
            found["cx"], found["cy"] = found.get("cx", found["rect"][0]), found.get("cy", found["rect"][1])
        return found

    def xdo(self, *args):
        subprocess.run(["xdotool", *map(str, args)], env=dict(os.environ, DISPLAY=self.display), check=True)

    def click(self, title, x, y):
        win = self(f"result = w.window({title!r}, timeout=10)")
        self.xdo("mousemove", win["cx"] + x, win["cy"] + y)
        time.sleep(0.1)
        self.xdo("click", 1)

    def close(self, title):
        self(f"""m = [x for x in w.windows() if {title!r} in x['title']]
for x in m: w.post(x['hwnd'], 0x0010)
result = len(m)""")

    def interrupt(self, pid):
        # A forced end of the launcher (Task Manager, a CI timeout). The kill-on-close
        # job must take the host and backend with it.
        self(f"p, _ = w.direct[{pid}]\np.kill()\nresult = True")

    def alive(self, pid):
        return self(f"import psutil\nresult = psutil.pid_exists({pid})")

    def leftovers(self):
        return self("""import psutil
result = {'hosts': [p.pid for p in psutil.process_iter(['name']) if (p.info['name'] or '').lower() == 'ui4me-host.exe']}""")


def wait(predicate, timeout=30.0, step=0.3):
    end = time.time() + timeout
    while time.time() < end:
        value = predicate()
        if value:
            return value
        time.sleep(step)
    return None


def scenario(desk, kind, command, folder, results, title, env, *, crash=False):
    launch_env = {**env, "UI4ME_DIRECT_RESULTS": results, "UI4ME_DIRECT_TITLE": title}
    args = ARGS + (["--crash"] if crash and "pwsh" not in kind and "powershell" not in kind else [])
    if crash and ("pwsh" in kind or "powershell" in kind):
        args = ARGS + ["-Crash"]
    started = time.time()
    proc = desk.launch(command + args, folder, launch_env)
    report = {"command": command + args}
    ready = wait(lambda: [r for r in desk.results(results) if r["event"] == "ready"], 90)
    report["ready_s"] = round(time.time() - started, 2) if ready else None
    win = wait(lambda: desk.window(title), 30) if ready else None
    report["window"] = bool(win)
    return proc, report


def run_suite(desk, kind, command, out_dir, folder, env, scenarios):
    report = {"launcher": kind, "command": command, "scenarios": {}}
    for name in scenarios:
        token = uuid.uuid4().hex[:6]
        results = str(Path(folder) / f"results-{name}-{token}.jsonl")
        title = f"UI4ME direct {kind} {name} {token}"
        row = {}
        try:
            if name == "duplicate":
                first, row = scenario(desk, kind, command, folder, results, title, env)
                second = desk.launch(command + ARGS, folder, {**env, "UI4ME_DIRECT_RESULTS": results + ".second",
                                                              "UI4ME_DIRECT_TITLE": title + " second"})
                row["second_exit"] = wait(lambda: desk.poll(second) is not None and (desk.poll(second),), 60)
                row["second_window"] = bool(desk.window(title + " second"))
                row["first_alive"] = desk.poll(first) is None
                desk.close(title)
                row["first_exit"] = wait(lambda: desk.poll(first) is not None and (desk.poll(first),), 60)
                proc = first
                ok = (row["second_exit"] == (0,) and not row["second_window"] and row["first_alive"]
                      and row["first_exit"] == (0,))
            else:
                proc, row = scenario(desk, kind, command, folder, results, title, env, crash=(name == "crash"))
                if name == "normal":
                    for expected in (1, 2):
                        desk.click(title, *BUTTON)
                        wait(lambda: [r for r in desk.results(results) if r["event"] == "click" and r["count"] == expected], 15)
                    time.sleep(0.5)
                    desk.close(title)
                    row["exit"] = wait(lambda: desk.poll(proc) is not None and (desk.poll(proc),), 60)
                elif name == "interrupt":
                    time.sleep(1.0)
                    desk.interrupt(proc)
                    row["exit"] = wait(lambda: desk.poll(proc) is not None and (desk.poll(proc),), 60)
                elif name == "crash":
                    desk.click(title, *BUTTON)
                    # The backend exits abruptly; the host keeps the window open as its
                    # failure surface (no orderly quit) until the user closes it.
                    clicked = wait(lambda: [r for r in desk.results(results) if r["event"] == "click"], 15)
                    # Close only once the backend has really gone (Windows PowerShell 5.1 takes a
                    # few seconds to exit); a close racing a dying backend needs a second close.
                    if clicked:
                        row["backend_gone"] = bool(wait(lambda: not desk.alive(clicked[0]["pid"]), 30))
                    time.sleep(2)
                    row["error_window"] = bool(desk.window(title)) and desk.poll(proc) is None
                    desk.close(title)
                    row["exit"] = wait(lambda: desk.poll(proc) is not None and (desk.poll(proc),), 60)
                recs = desk.results(results)
                row["roles"] = [r.get("role") for r in recs if r["event"] == "module"]
                row["clicks"] = [(r["count"], r["text"]) for r in recs if r["event"] == "click"]
                row["ready"] = next((r for r in recs if r["event"] == "ready"), None)
                row["orderly"] = any(r["event"] == "orderly" for r in recs)
                ok = bool(row.get("window")) and row["roles"][:2] == ["launcher", "backend"]
                if row["ready"]:
                    ok = ok and row["ready"]["argv"][:2] == ARGS and row["ready"]["secret_visible"] is False
                if name == "normal":
                    ok = ok and row["clicks"] == [(1, "pressed 1"), (2, "pressed 2")] and row["orderly"] \
                        and row["exit"] == (0,)
                elif name == "interrupt":
                    # Linux: the launcher reports 128+SIGINT, and the supervised host shuts the
                    # app down gracefully (it may still record its orderly end). Windows: the
                    # killed launcher's exit code is the kill's; the host must still be gone.
                    # PowerShell finishes the script once the host is gone and reports
                    # the host's status; the guarantee checked here is the cleanup.
                    expected = 130 if isinstance(desk, Linux) and kind in ("python", "uv") else None
                    ok = ok and row["exit"] is not None and (expected is None or row["exit"][0] == expected)
                elif name == "crash":
                    ok = ok and row["clicks"][:1] == [(1, "pressed 1")] and row["error_window"] \
                        and row["exit"] is not None and row["exit"][0] != 0
            time.sleep(2)
            row["leftovers"] = desk.leftovers()
            ok = ok and not any(row["leftovers"].values())
            row["launcher_output"] = desk.output(proc)[-4000:]
        except Exception as error:  # record, clean up, continue with the next scenario
            ok, row["error"] = False, repr(error)
            try:
                desk.close(title)
                time.sleep(3)
                row["leftovers_after_error"] = desk.leftovers()
            except Exception:
                pass
        row["status"] = "PASS" if ok else "FAIL"
        report["scenarios"][name] = row
        print(f"{kind} {name}: {row['status']}", flush=True)
    return report


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--platform", choices=("linux", "windows"), default="linux")
    ap.add_argument("--launcher", required=True, help="label: python, uv, pwsh, powershell")
    ap.add_argument("--command", nargs=argparse.REMAINDER, required=True,
                    help="launcher prefix; must be the last option (everything after it belongs to it)")
    ap.add_argument("--workdir", required=True, help="app folder (on the target machine)")
    ap.add_argument("--module", help="UI4ME.Core.psd1 to import (PowerShell); default: installed module")
    ap.add_argument("--host-binary", help="UI4ME_HOST_BINARY for the app (development builds)")
    ap.add_argument("--pythonpath", help="PYTHONPATH for the app (source checkout)")
    ap.add_argument("--scenario", action="append", choices=("normal", "interrupt", "duplicate", "crash"))
    ap.add_argument("--out", required=True, type=Path)
    a = ap.parse_args()
    desk = Linux() if a.platform == "linux" else Windows()
    powershell = a.launcher in ("pwsh", "powershell")
    name = "app.ps1" if powershell else "app.py"
    folder = desk.prepare(a.workdir, name, POWERSHELL_APP if powershell else PYTHON_APP)
    env = {}
    if a.module:
        env["UI4ME_DIRECT_MODULE"] = a.module
    if a.host_binary:
        env["UI4ME_HOST_BINARY"] = a.host_binary
    if a.pythonpath:
        env["PYTHONPATH"] = a.pythonpath
    command = a.command + [name]
    report = run_suite(desk, a.launcher, command, a.out, folder, env,
                       a.scenario or ["normal", "interrupt", "duplicate", "crash"])
    report["platform"] = a.platform
    report["status"] = "PASS" if all(s["status"] == "PASS" for s in report["scenarios"].values()) else "FAIL"
    a.out.mkdir(parents=True, exist_ok=True)
    target = a.out / f"direct-{a.platform}-{a.launcher}.json"
    target.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({"status": report["status"], "report": str(target)}))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
