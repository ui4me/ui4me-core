"""Driver helpers for the workstream-N native probes (runs ON linuxDEV, inside the KDE session env).

Mixes three channels in one timeline:
  * injected input: ydotool via uinput (real kernel input device -> KWin -> wl_keyboard/wl_pointer),
  * AT-SPI: pyatspi over the real accessibility bus (AccessKit unix adapter in the host),
  * the backend API: commands appended to the probe app's cmd.jsonl (see probe_app.py).
"""
from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path

HOME = Path.home()
REPO = HOME / "projects/ui4me-core-stab"
PROBE = REPO / "n-probe"
HOST = REPO / "target/release/ui4me-host"
BG = (232, 240, 255)  # body background of every probe page (#e8f0ff)

ENV = dict(os.environ)
ENV.update({
    "XDG_RUNTIME_DIR": f"/run/user/{os.getuid()}", "WAYLAND_DISPLAY": "wayland-0",
    "DBUS_SESSION_BUS_ADDRESS": f"unix:path=/run/user/{os.getuid()}/bus",
    "XDG_SESSION_TYPE": "wayland", "XDG_CURRENT_DESKTOP": "KDE",
    "YDOTOOL_SOCKET": f"/run/user/{os.getuid()}/.ydotool_socket",
    "PATH": f"{HOME}/lab/bin:" + os.environ.get("PATH", ""),
    "UI4ME_HOST_BINARY": str(HOST), "PYTHONPATH": str(REPO / "python"),
})
ENV.pop("DISPLAY", None)

# evdev key codes (linux/input-event-codes.h)
KEY = {"tab": 15, "enter": 28, "ctrl": 29, "shift": 42, "esc": 1, "a": 30, "c": 46, "s": 31,
       "v": 47, "x": 45, "z": 44, "space": 57, "end": 107, "home": 102, "alt": 56}


def sh(cmd: str, check=True) -> str:
    r = subprocess.run(["bash", "-c", cmd], env=ENV, capture_output=True, text=True)
    if check and r.returncode:
        raise RuntimeError(f"{cmd}: {r.stderr}")
    return r.stdout


class App:
    def __init__(self, scenario: str, out: Path):
        self.out = out
        out.mkdir(parents=True, exist_ok=True)
        self.n = 0
        self.log = open(out / "app.log", "w")
        self.proc = subprocess.Popen(["python3", str(PROBE / "probe_app.py"), scenario, str(out)],
                                     env=ENV, stdout=self.log, stderr=subprocess.STDOUT)
        self.wait_result(0, 30)
        time.sleep(1.5)

    def results(self):
        p = self.out / "results.jsonl"
        return [json.loads(x) for x in p.read_text().splitlines() if x.strip()] if p.exists() else []

    def events(self):
        p = self.out / "events.jsonl"
        return [json.loads(x) for x in p.read_text().splitlines() if x.strip()] if p.exists() else []

    def wait_result(self, cid, timeout=15):
        end = time.time() + timeout
        while time.time() < end:
            for r in self.results():
                if r["id"] == cid:
                    return r
            if self.proc.poll() is not None:
                raise RuntimeError(f"probe app exited {self.proc.returncode}")
            time.sleep(0.1)
        raise TimeoutError(cid)

    def cmd(self, op, **kw):
        self.n += 1
        with open(self.out / "cmd.jsonl", "a") as fh:
            fh.write(json.dumps({"id": self.n, "op": op, **kw}) + "\n")
        r = self.wait_result(self.n)
        if "error" in r:
            raise RuntimeError(r["error"])
        return r["ok"]

    def close(self):
        try:
            self.cmd("quit")
        except Exception:
            pass
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.log.close()


# ---------------------------------------------------------------- injected input
def keys(*seq: str, delay=0.08):
    """keys('tab'), keys('ctrl+s'): press modifiers, tap key, release (evdev codes via ydotool)."""
    for combo in seq:
        parts = combo.split("+")
        codes = [KEY[p] for p in parts]
        args = [f"{c}:1" for c in codes] + [f"{c}:0" for c in reversed(codes)]
        sh("ydotool key -d 40 " + " ".join(args))
        time.sleep(delay)
    time.sleep(0.4)


def move(x, y):
    sh(f"lab-move {x} {y}")


def click(x, y):
    sh(f"lab-click {x} {y}")
    time.sleep(0.4)


def press_release(x1, y1, x2, y2):
    """Left press at (x1,y1), relative move, release at (x2,y2)."""
    sh(f"lab-drag {x1} {y1} {x2} {y2}")
    time.sleep(0.4)


# ---------------------------------------------------------------- screenshots
def shot(path: Path):
    sh(f"lab-shot {path}")
    return path


def content_box(path: Path):
    """Bounding box (x0, y0, x1, y1) of the probe page background colour = window content area."""
    from PIL import Image
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    xs, ys = [], []
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            if px[x, y] == BG:
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    # refine the exact top-left edge
    x0, y0 = min(xs), min(ys)
    while x0 > 0 and px[x0 - 1, y0 + 2] == BG:
        x0 -= 1
    while y0 > 0 and px[x0 + 2, y0 - 1] == BG:
        y0 -= 1
    return (x0, y0, max(xs) + 1, max(ys) + 1)


def colour_box(path: Path, rgb, region=None, tol=0):
    from PIL import Image
    im = Image.open(path).convert("RGB")
    px = im.load()
    x0, y0, x1, y1 = region or (0, 0, *im.size)
    xs, ys = [], []
    for y in range(y0, y1):
        for x in range(x0, x1):
            p = px[x, y]
            if all(abs(p[i] - rgb[i]) <= tol for i in range(3)):
                xs.append(x)
                ys.append(y)
    return (min(xs), min(ys), max(xs) + 1, max(ys) + 1, len(xs)) if xs else None


def small_png(src: Path, dst: Path, crop=None, max_w=640):
    from PIL import Image
    im = Image.open(src).convert("RGB")
    if crop:
        im = im.crop(crop)
    if im.width > max_w:
        im = im.resize((max_w, round(im.height * max_w / im.width)))
    im = im.quantize(colors=64)
    im.save(dst, optimize=True)
    return dst.stat().st_size


# ---------------------------------------------------------------- AT-SPI
def atspi_app(name_part="N probe", timeout=10):
    import pyatspi
    end = time.time() + timeout
    while time.time() < end:
        desktop = pyatspi.Registry.getDesktop(0)
        for i in range(desktop.childCount):
            a = desktop.getChildAtIndex(i)
            if a is not None and name_part.lower() in (a.name or "").lower():
                return a
        time.sleep(0.5)
    names = [desktop.getChildAtIndex(i).name for i in range(desktop.childCount)]
    raise RuntimeError(f"AT-SPI app not found; apps: {names}")


def walk(acc, depth=0, max_depth=40):
    yield acc, depth
    if depth >= max_depth:
        return
    for i in range(acc.childCount):
        c = acc.getChildAtIndex(i)
        if c is not None:
            yield from walk(c, depth + 1, max_depth)


def find(app, name=None, role=None):
    for acc, _ in walk(app):
        try:
            if (name is None or (acc.name or "") == name) and (role is None or acc.getRoleName() == role):
                return acc
        except Exception:
            continue
    return None


def focused(app):
    import pyatspi
    for acc, _ in walk(app):
        try:
            if acc.getState().contains(pyatspi.STATE_FOCUSED):
                return acc
        except Exception:
            continue
    return None


def extents(acc):
    import pyatspi
    comp = acc.queryComponent()
    out = {}
    for label, ct in (("window", pyatspi.WINDOW_COORDS), ("screen", pyatspi.DESKTOP_COORDS)):
        e = comp.getExtents(ct)
        out[label] = [e.x, e.y, e.width, e.height]
    return out


def describe(acc):
    if acc is None:
        return None
    d = {"name": acc.name, "role": acc.getRoleName()}
    try:
        d["extents"] = extents(acc)
    except Exception as exc:
        d["extents_error"] = repr(exc)
    return d
