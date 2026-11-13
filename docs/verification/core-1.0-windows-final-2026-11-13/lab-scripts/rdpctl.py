"""Disconnect / reconnect the ui4me_test RDP session from Linux (xfreerdp3 on Xvfb :88)."""
import os, subprocess, time
L = os.path.dirname(os.path.abspath(__file__))


def pids():
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                if os.readlink(f"/proc/{p}/exe").endswith("xfreerdp3"):
                    out.append(int(p))
            except OSError:
                pass
    return out


def disconnect():
    for p in pids():
        os.kill(p, 15)
    end = time.time() + 10
    while pids() and time.time() < end:
        time.sleep(0.2)
    time.sleep(2)


def connect(w=1600, h=1000, scale=100):
    env = dict(os.environ, W=str(w), H=str(h), SCALE=str(scale))
    subprocess.run(["bash", os.path.join(L, "rdp.sh")], env=env, check=True)
    time.sleep(12)


def session():
    r = subprocess.run(["ssh", "-o", "BatchMode=yes", "administrator@omegaui", "query user"], capture_output=True, text=True)
    return [l.split() for l in r.stdout.splitlines() if "ui4me_test" in l]
