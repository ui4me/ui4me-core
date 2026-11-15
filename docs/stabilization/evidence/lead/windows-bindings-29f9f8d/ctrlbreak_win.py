"""Windows: start an interrupt probe in its own process group, send CTRL_BREAK_EVENT, report exit and output.
Output goes to files (a pipe can die with the console event)."""
import os, signal, subprocess, sys, time
ws = r"F:\agent-workspaces\ui4me-core\stab-374b06f6"
scripts = os.path.join(ws, r"src\docs\stabilization\evidence\e-bindings\scripts")
module = os.path.join(ws, r"src\powershell\UI4ME.Core\UI4ME.Core.psd1")
host = os.environ.get("UI4ME_HOST_BINARY", os.path.join(ws, r"target\release\ui4me-host.exe"))
cases = {
  "python": [sys.executable, os.path.join(scripts, "interrupt_app.py")],
  "ps51": [os.path.join(os.environ["SystemRoot"], r"System32\WindowsPowerShell\v1.0\powershell.exe"), "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", os.path.join(scripts, "interrupt_app.ps1"), "-Module", module],
  "pwsh": [os.path.join(ws, r"pwsh7\pwsh.exe"), "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", os.path.join(scripts, "interrupt_app.ps1"), "-Module", module],
}
out_dir = sys.argv[1]; os.makedirs(out_dir, exist_ok=True)
for name in (sys.argv[2:] or list(cases)):
    env = dict(os.environ, UI4ME_TEST_MODE="1", UI4ME_HOST_BINARY=host, PYTHONPATH=os.path.join(ws, r"src\python"))
    so, se = os.path.join(out_dir, name + ".out.txt"), os.path.join(out_dir, name + ".err.txt")
    with open(so, "w") as fo, open(se, "w") as fe:
        p = subprocess.Popen(cases[name], env=env, stdout=fo, stderr=fe, stdin=subprocess.DEVNULL,
                             creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
        t0 = time.monotonic(); ready = False
        while time.monotonic() - t0 < 40 and p.poll() is None:
            time.sleep(0.25)
            if "READY" in open(so, encoding="utf-8", errors="replace").read(): ready = True; break
        time.sleep(1.0)
        t1 = time.monotonic()
        if p.poll() is None: os.kill(p.pid, signal.CTRL_BREAK_EVENT)
        try: p.wait(timeout=30); to = False
        except subprocess.TimeoutExpired: p.kill(); p.wait(); to = True
    code = p.returncode & 0xFFFFFFFF
    out = open(so, encoding="utf-8", errors="replace").read().split()
    print(f"CASE {name}: ready={ready} exit={code} (0x{code:08X}) after={time.monotonic()-t1:.2f}s timeout={to} stdout={out}", flush=True)
    time.sleep(2)
