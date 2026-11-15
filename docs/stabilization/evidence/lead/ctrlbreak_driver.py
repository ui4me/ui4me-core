"""Send CTRL_BREAK to a headless UI4ME app in its own process group; report exit code and shutdown delivery."""
import os, signal, subprocess, sys, time
mark = os.path.abspath(sys.argv[1]); env = dict(os.environ, UI4ME_TEST_MODE="1", CB_MARK=mark)
if os.path.exists(mark): os.remove(mark)
p = subprocess.Popen([sys.executable, "ctrlbreak_app.py"], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                     creationflags=subprocess.CREATE_NEW_PROCESS_GROUP, text=True, encoding="utf-8", errors="replace")
lines = []
t0 = time.monotonic()
while time.monotonic() - t0 < 20:
    line = p.stdout.readline()
    if not line: break
    lines.append(line.rstrip())
    if "READY" in line: break
time.sleep(1.0)
os.kill(p.pid, signal.CTRL_BREAK_EVENT)
t1 = time.monotonic()
try:
    out, _ = p.communicate(timeout=15)
except subprocess.TimeoutExpired:
    p.kill(); out, _ = p.communicate()
    print("DRIVER: TIMEOUT")
lines += out.splitlines()
print("DRIVER: exit", p.returncode, "after", round(time.monotonic() - t1, 2), "s")
for l in lines:
    if any(k in l for k in ("READY", "BACKEND_SHUTDOWN", "RUN_RETURNED", "termination", "shutdown", "Traceback", "Error", "error")):
        print("  ", l[:220])
time.sleep(6)
print("MARK FILE:")
print(open(mark, encoding="utf-8").read() if os.path.exists(mark) else "  (none)")
