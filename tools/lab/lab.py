"""KDE Wayland lab helpers for native qualification: uinput input, KWin window queries, screenshots.

Real input comes from inputd.py, a root uinput daemon started separately
(sudo python3 tools/lab/inputd.py SOCKET WIDTH HEIGHT). Test tooling only.
"""
import json, os, socket, subprocess, tempfile, time, uuid
from pathlib import Path

LAB = Path(__file__).resolve().parent
SOCK = "/run/user/1000/ui4me-lab-input.sock"
_conn = None


def _send(cmd):
    global _conn
    if _conn is None:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(SOCK)
        _conn = s.makefile("rw")
    _conn.write(json.dumps(cmd) + "\n")
    _conn.flush()
    reply = _conn.readline().strip()
    if reply != "ok":
        raise RuntimeError(reply)


LOGICAL = [2048, 1080]  # logical screen size; update after a scale change


def set_scale(scale, physical=(2048, 1080)):
    LOGICAL[0], LOGICAL[1] = physical[0] / scale, physical[1] / scale


def move(x, y):
    # inputd maps 0..2047 x 0..1079 onto the whole output; convert logical px.
    _send({"op": "move", "x": x * 2047 / (LOGICAL[0] - 1) if LOGICAL[0] != 2048 else x,
           "y": y * 1079 / (LOGICAL[1] - 1) if LOGICAL[1] != 1080 else y})


def button(name="left", down=True):
    _send({"op": "button", "button": name, "down": down})


def click(x, y, name="left", hold=0.05):
    move(x, y); time.sleep(0.05)
    button(name, True); time.sleep(hold); button(name, False); time.sleep(0.05)


def dblclick(x, y):
    move(x, y); time.sleep(0.05)
    for _ in range(2):
        button("left", True); time.sleep(0.03); button("left", False); time.sleep(0.08)


def drag(x0, y0, x1, y1, steps=20, dt=0.015, name="left", release=True):
    move(x0, y0); time.sleep(0.08)
    button(name, True); time.sleep(0.08)
    for i in range(1, steps + 1):
        move(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps); time.sleep(dt)
    time.sleep(0.08)
    if release:
        button(name, False); time.sleep(0.1)


def wheel(dy):
    _send({"op": "wheel", "dy": dy})


def key(code, down=None):
    if down is None:
        _send({"op": "key", "code": code, "down": True}); time.sleep(0.02)
        _send({"op": "key", "code": code, "down": False}); time.sleep(0.02)
    else:
        _send({"op": "key", "code": code, "down": down})


def combo(*codes):
    for c in codes:
        key(c, True); time.sleep(0.02)
    for c in reversed(codes):
        key(c, False); time.sleep(0.02)


_CHARS = {" ": "KEY_SPACE", "-": "KEY_MINUS", ".": "KEY_DOT", ",": "KEY_COMMA", "/": "KEY_SLASH",
          "\n": "KEY_ENTER", "=": "KEY_EQUAL", ";": "KEY_SEMICOLON", "'": "KEY_APOSTROPHE"}
_SHIFT = {"_": "KEY_MINUS", "!": "KEY_1", "@": "KEY_2", "#": "KEY_3", "?": "KEY_SLASH", ":": "KEY_SEMICOLON",
          "<": "KEY_COMMA", ">": "KEY_DOT", "&": "KEY_7", "(": "KEY_9", ")": "KEY_0", "+": "KEY_EQUAL"}


def type_text(text):
    for ch in text:
        if ch.isalpha() and ch.isascii():
            if ch.isupper():
                combo("KEY_LEFTSHIFT", "KEY_" + ch)
            else:
                key("KEY_" + ch.upper())
        elif ch.isdigit():
            key("KEY_" + ch)
        elif ch in _CHARS:
            key(_CHARS[ch])
        elif ch in _SHIFT:
            combo("KEY_LEFTSHIFT", _SHIFT[ch])
        else:
            raise ValueError(f"cannot type {ch!r}")


def kwin_script(js, wait=0.6):
    """Run a KWin script; return lines it print()s (read back from the user journal)."""
    tag = "UI4MELAB" + uuid.uuid4().hex[:8]
    body = js.replace("OUT(", f"print('{tag} ' + ")
    path = Path(tempfile.gettempdir()) / f"kwin-{tag}.js"
    path.write_text(body)
    since = time.strftime("%Y-%m-%d %H:%M:%S")
    num = subprocess.check_output(["qdbus6", "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript",
                                   str(path), tag], text=True).strip()
    subprocess.run(["qdbus6", "org.kde.KWin", f"/Scripting/Script{num}", "org.kde.kwin.Script.run"],
                   check=True, capture_output=True)
    time.sleep(wait)
    subprocess.run(["qdbus6", "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", tag],
                   capture_output=True)
    path.unlink(missing_ok=True)
    out = subprocess.run(["journalctl", "--user", "-q", "--since", since, "-o", "cat", "--no-pager"],
                         capture_output=True, text=True).stdout
    return [l.split(tag + " ", 1)[1] for l in out.splitlines() if tag + " " in l]


WINDOWS_JS = r"""
var ws = workspace.windowList();
var r = [];
for (var i = 0; i < ws.length; i++) { var w = ws[i]; if (!w.normalWindow && !w.dialog) continue;
  r.push({caption: w.caption, pid: w.pid, res: w.resourceClass, desktopFile: w.desktopFileName,
    x: w.frameGeometry.x, y: w.frameGeometry.y, w: w.frameGeometry.width, h: w.frameGeometry.height,
    cx: w.clientGeometry.x, cy: w.clientGeometry.y, cw: w.clientGeometry.width, ch: w.clientGeometry.height,
    active: w.active, minimized: w.minimized, maximized: (w.maximizeMode||0), fullScreen: w.fullScreen,
    noBorder: w.noBorder, transient: w.transient, modal: w.modal, keepAbove: w.keepAbove,
    transientFor: w.transientFor ? w.transientFor.caption : null});
}
OUT(JSON.stringify(r));
"""


def windows():
    lines = kwin_script(WINDOWS_JS)
    return json.loads(lines[-1]) if lines else []


def window(caption_part):
    for w in windows():
        if caption_part in w["caption"]:
            return w
    return None


def wait_window(caption_part, timeout=15):
    deadline = time.time() + timeout
    while time.time() < deadline:
        w = window(caption_part)
        if w:
            return w
        time.sleep(0.4)
    raise TimeoutError(caption_part)


def activate(caption_part):
    kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ workspace.activeWindow = ws[i]; OUT('ok'); break; } }""" % json.dumps(caption_part))


def set_geometry(caption_part, x, y, w, h):
    kwin_script("""var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].frameGeometry = {x:%d,y:%d,width:%d,height:%d}; OUT('ok'); break; } }""" % (json.dumps(caption_part), x, y, w, h))


def screenshot(path, region=None):
    """Full-screen screenshot via spectacle (background, no notification)."""
    path = Path(path)
    subprocess.run(["spectacle", "-b", "-n", "-f", "-o", str(path)], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
    if region:
        from PIL import Image
        img = Image.open(path)
        x, y, w, h = region
        img.crop((x, y, x + w, y + h)).save(path)
    return path


def pss_kib(pid):
    total = 0
    with open(f"/proc/{pid}/smaps_rollup") as f:
        for line in f:
            if line.startswith("Pss:"):
                total = int(line.split()[1])
    return total


def cpu_ticks(pid):
    with open(f"/proc/{pid}/stat") as f:
        parts = f.read().rsplit(")", 1)[1].split()
    return int(parts[11]) + int(parts[12])


def dnd(x0, y0, x1, y1, hover=1.0):
    """Inter-application drag: start slowly past the toolkit threshold, travel, hover, drop."""
    move(x0, y0); time.sleep(0.3)
    button("left", True); time.sleep(0.3)
    for i in range(1, 16):
        move(x0 + i * 2, y0 + i); time.sleep(0.03)
    time.sleep(0.4)
    sx, sy = x0 + 30, y0 + 15
    for i in range(1, 51):
        move(sx + (x1 - sx) * i / 50, sy + (y1 - sy) * i / 50); time.sleep(0.025)
    for i in range(10):
        move(x1 + (i % 2) * 3, y1); time.sleep(hover / 10)
    button("left", False); time.sleep(0.8)
