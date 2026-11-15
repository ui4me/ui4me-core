import os, signal, subprocess, sys, time
env = dict(os.environ)
p = subprocess.Popen([sys.executable, "e12_ctrlc.py"], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                     start_new_session=True, env=env,
                     preexec_fn=lambda: signal.signal(signal.SIGINT, signal.SIG_DFL))
time.sleep(3)
os.killpg(p.pid, signal.SIGINT)     # what a terminal Ctrl+C does to the foreground group
t0 = time.time()
try:
    out, err = p.communicate(timeout=20)
except subprocess.TimeoutExpired:
    os.killpg(p.pid, signal.SIGKILL); out, err = p.communicate()
    print("TIMEOUT")
print("launcher exit", p.returncode, "after", round(time.time() - t0, 2), "s")
print("-- stdout\n" + out.decode()); print("-- stderr\n" + err.decode()[-3000:])
