"""Linux-side driver helpers: in-session Windows agent (via SSH relay) + real RDP input (xdotool on :88)."""
import json, os, subprocess, time, textwrap
DISPLAY = ":88"
_relay = None


def _start():
    global _relay
    _relay = subprocess.Popen(["ssh", "-o", "BatchMode=yes", "-o", "ServerAliveInterval=20", "administrator@omegaui",
                               r"F:\ui4me\lab\venv\Scripts\python.exe -u F:\ui4me\lab\relay.py"],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)


def ex(code, timeout=None):
    """Run code in the session agent; returns `result` or raises."""
    if _relay is None or _relay.poll() is not None:
        _start()
    _relay.stdin.write(json.dumps({"exec": textwrap.dedent(code)}) + "\n"); _relay.stdin.flush()
    line = _relay.stdout.readline()
    if not line:
        raise RuntimeError("relay closed")
    r = json.loads(line)
    if not r["ok"]:
        raise RuntimeError(r["error"] + "\n" + r.get("trace", ""))
    return r["result"]


def xdo(*args):
    subprocess.run(["xdotool", *map(str, args)], env={**os.environ, "DISPLAY": DISPLAY}, check=True)


def move(x, y):
    xdo("mousemove", int(round(x)), int(round(y)))


BTN = {"left": 1, "middle": 2, "right": 3}


def click(x, y, button="left", hold=0.05):
    move(x, y); time.sleep(0.08)
    xdo("mousedown", BTN[button]); time.sleep(hold); xdo("mouseup", BTN[button]); time.sleep(0.08)


def dblclick(x, y):
    move(x, y); time.sleep(0.08)
    xdo("click", "--repeat", 2, "--delay", 80, 1); time.sleep(0.1)


def drag(x0, y0, x1, y1, steps=20, dt=0.02, button="left"):
    move(x0, y0); time.sleep(0.1)
    xdo("mousedown", BTN[button]); time.sleep(0.15)
    for i in range(1, steps + 1):
        move(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps); time.sleep(dt)
    time.sleep(0.15)
    xdo("mouseup", BTN[button]); time.sleep(0.2)


def key(*keys):
    for k in keys:
        xdo("key", "--delay", 60, k); time.sleep(0.05)


def type_(text, delay=40):
    xdo("type", "--delay", delay, text)


def shot(path):
    subprocess.run(["import", "-window", "root", path], env={**os.environ, "DISPLAY": DISPLAY}, check=True)
    return path


# ---- convenience wrappers around the agent ----
def win(title, timeout=10.0):
    return ex(f'result = w.window({title!r}, timeout={timeout})')


def wins(title=None):
    return ex(f'result = w.windows({title!r})')


def gone(title, timeout=10.0):
    end = time.time() + timeout
    while time.time() < end:
        if not wins(title):
            return True
        time.sleep(0.2)
    return False


def activate(title):
    """Real click on the window's caption area (decorated windows)."""
    x = win(title)
    fx, fy, fw, fh = x["frame"]
    click(fx + fw * 0.5, fy + 12)
    time.sleep(0.3)
    return x


def at(title, cx, cy, button="left"):
    """Real click at client coordinates of a window."""
    x = win(title)
    click(x["cx"] + cx, x["cy"] + cy, button)


def uia(code):
    return ex("import uia\n" + textwrap.dedent(code))


def clip_get():
    return ex("result = w.clipboard_get()")


def clip_set(text):
    return ex(f"w.clipboard_set({text!r})")


def wshot(name):
    return ex(f"result = w.screenshot({name!r})")


def P(*a):
    print(*a, flush=True)
