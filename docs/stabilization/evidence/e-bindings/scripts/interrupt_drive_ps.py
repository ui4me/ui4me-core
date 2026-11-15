import os, signal, subprocess, sys, time
host, mode, module = sys.argv[1:4]
env = dict(os.environ, UI4ME_TEST_MODE="1", UI4ME_HOST_BINARY=host)
p = subprocess.Popen(["pwsh", "-NoProfile", "-File", os.path.join(os.path.dirname(__file__), "interrupt_app.ps1"), "-Module", module],
                     env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
for line in p.stdout:
    if "READY" in line: break
t = time.monotonic()
if mode == "group": os.killpg(p.pid, signal.SIGINT)
else: os.kill(p.pid, signal.SIGINT)
out, err = p.communicate(timeout=60)
print(f"mode={mode} exit={p.returncode} after={time.monotonic()-t:.2f}s stdout={out.split()}")
