import os, signal, subprocess, sys, time
host, mode = sys.argv[1], sys.argv[2]
env = dict(os.environ, UI4ME_TEST_MODE="1", UI4ME_HOST_BINARY=host,
           PYTHONPATH=os.path.join(os.path.dirname(__file__), "../../../../../python"))
p = subprocess.Popen([sys.executable, os.path.join(os.path.dirname(__file__), "interrupt_app.py")], env=env,
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
for line in p.stdout:
    if "READY" in line: break
t = time.monotonic()
if mode == "group": os.killpg(p.pid, signal.SIGINT)
elif mode == "term": os.kill(p.pid, signal.SIGTERM)
else: os.kill(p.pid, signal.SIGINT)
out, err = p.communicate(timeout=60)
print(f"mode={mode} exit={p.returncode} after={time.monotonic()-t:.2f}s stdout={out.split()} defect_msg={'terminated by' in err}")
time.sleep(0.5)
print("leftovers:", [l for l in subprocess.run(["pgrep", "-af", "interrupt_app"], capture_output=True, text=True).stdout.splitlines()])
