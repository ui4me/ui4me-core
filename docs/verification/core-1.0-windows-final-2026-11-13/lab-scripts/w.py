"""Win32 observation helpers for the UI4ME lab agent (DPI-aware process)."""
import ctypes
import os
import time
from ctypes import wintypes
from pathlib import Path

import psutil

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
dwm = ctypes.WinDLL("dwmapi")
OUT = Path(r"F:\ui4me\lab\out")
OUT.mkdir(parents=True, exist_ok=True)

WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
u32.EnumWindows.argtypes = [WNDENUMPROC, wintypes.LPARAM]
u32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
u32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
u32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
u32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
u32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
u32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
u32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
u32.GetWindow.restype = wintypes.HWND
u32.GetForegroundWindow.restype = wintypes.HWND
u32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
u32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
u32.GetDpiForWindow.argtypes = [wintypes.HWND]
u32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
u32.MoveWindow.argtypes = [wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.BOOL]
u32.GetGUIThreadInfo.argtypes = [wintypes.DWORD, ctypes.c_void_p]


def session_id():
    sid = wintypes.DWORD()
    k32.ProcessIdToSessionId(os.getpid(), ctypes.byref(sid))
    return sid.value


def _text(fn, hwnd, n=512):
    buf = ctypes.create_unicode_buffer(n)
    fn(hwnd, buf, n)
    return buf.value


def info(hwnd):
    hwnd = wintypes.HWND(hwnd) if isinstance(hwnd, int) else hwnd
    pid = wintypes.DWORD()
    u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    r = wintypes.RECT()
    u32.GetWindowRect(hwnd, ctypes.byref(r))
    ext = wintypes.RECT()  # visible frame bounds (excludes invisible resize borders)
    dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(ext), ctypes.sizeof(ext))
    c = wintypes.RECT()
    u32.GetClientRect(hwnd, ctypes.byref(c))
    p = wintypes.POINT(0, 0)
    u32.ClientToScreen(hwnd, ctypes.byref(p))
    owner = u32.GetWindow(hwnd, 4)
    value = ctypes.cast(hwnd, ctypes.c_void_p).value or 0
    return {
        "hwnd": value, "title": _text(u32.GetWindowTextW, hwnd), "class": _text(u32.GetClassNameW, hwnd),
        "pid": pid.value, "rect": [r.left, r.top, r.right - r.left, r.bottom - r.top],
        "frame": [ext.left, ext.top, ext.right - ext.left, ext.bottom - ext.top],
        "client": [p.x, p.y, c.right, c.bottom], "cx": p.x, "cy": p.y, "cw": c.right, "ch": c.bottom,
        "visible": bool(u32.IsWindowVisible(hwnd)), "iconic": bool(u32.IsIconic(hwnd)),
        "zoomed": bool(u32.IsZoomed(hwnd)), "enabled": bool(u32.IsWindowEnabled(hwnd)),
        "foreground": value == (ctypes.cast(u32.GetForegroundWindow(), ctypes.c_void_p).value or 0),
        "owner": ctypes.cast(owner, ctypes.c_void_p).value or 0,
        "style": u32.GetWindowLongPtrW(hwnd, -16) & 0xFFFFFFFF,
        "exstyle": u32.GetWindowLongPtrW(hwnd, -20) & 0xFFFFFFFF,
        "dpi": u32.GetDpiForWindow(hwnd),
    }


def windows(title=None, pid=None, cls=None, visible=True):
    found = []

    def cb(hwnd, _):
        if visible and not u32.IsWindowVisible(hwnd):
            return True
        t = _text(u32.GetWindowTextW, hwnd)
        if title is not None and title not in t:
            return True
        i = info(hwnd)
        if pid is not None and i["pid"] != pid:
            return True
        if cls is not None and i["class"] != cls:
            return True
        found.append(i)
        return True
    u32.EnumWindows(WNDENUMPROC(cb), 0)
    return found


def window(title, timeout=10.0, **kw):
    deadline = time.monotonic() + timeout
    while True:
        ws = windows(title, **kw)
        if ws:
            return ws[0]
        if time.monotonic() > deadline:
            raise TimeoutError(f"window {title!r} not found")
        time.sleep(0.1)


def foreground():
    h = u32.GetForegroundWindow()
    return info(h) if h else None


def focus_info():
    class GUITHREADINFO(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.DWORD), ("flags", wintypes.DWORD), ("hwndActive", wintypes.HWND),
                    ("hwndFocus", wintypes.HWND), ("hwndCapture", wintypes.HWND), ("hwndMenuOwner", wintypes.HWND),
                    ("hwndMoveSize", wintypes.HWND), ("hwndCaret", wintypes.HWND), ("rcCaret", wintypes.RECT)]
    g = GUITHREADINFO(cbSize=ctypes.sizeof(GUITHREADINFO))
    u32.GetGUIThreadInfo(0, ctypes.byref(g))
    v = lambda h: ctypes.cast(h, ctypes.c_void_p).value or 0
    return {"active": v(g.hwndActive), "focus": v(g.hwndFocus), "capture": v(g.hwndCapture),
            "movesize": v(g.hwndMoveSize), "flags": g.flags}


def post(hwnd, msg, wp=0, lp=0):
    return bool(u32.PostMessageW(wintypes.HWND(hwnd), msg, wp, lp))


def show(hwnd, cmd):
    return bool(u32.ShowWindow(wintypes.HWND(hwnd), cmd))


def move(hwnd, x, y, width, height):
    return bool(u32.MoveWindow(wintypes.HWND(hwnd), x, y, width, height, True))


def screenshot(name, bbox=None):
    from PIL import ImageGrab
    img = ImageGrab.grab(bbox=tuple(bbox) if bbox else None, all_screens=True)
    path = OUT / name
    img.save(path)
    return str(path)


def pixel(x, y):
    from PIL import ImageGrab
    return ImageGrab.grab(bbox=(x, y, x + 1, y + 1), all_screens=True).getpixel((0, 0))


def grab(bbox):
    from PIL import ImageGrab
    return ImageGrab.grab(bbox=tuple(bbox), all_screens=True)


CF_UNICODETEXT = 13
k32.GlobalAlloc.restype = ctypes.c_void_p
k32.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
k32.GlobalLock.restype = ctypes.c_void_p
k32.GlobalLock.argtypes = [ctypes.c_void_p]
k32.GlobalUnlock.argtypes = [ctypes.c_void_p]
u32.GetClipboardData.restype = ctypes.c_void_p
u32.SetClipboardData.argtypes = [wintypes.UINT, ctypes.c_void_p]
u32.SetClipboardData.restype = ctypes.c_void_p


def _open_clipboard():
    for _ in range(50):
        if u32.OpenClipboard(None):
            return
        time.sleep(0.02)
    raise OSError("OpenClipboard failed")


def clipboard_get():
    _open_clipboard()
    try:
        h = u32.GetClipboardData(CF_UNICODETEXT)
        if not h:
            return None
        p = k32.GlobalLock(h)
        try:
            return ctypes.wstring_at(p)
        finally:
            k32.GlobalUnlock(h)
    finally:
        u32.CloseClipboard()


def clipboard_set(text):
    data = text.encode("utf-16-le") + b"\0\0"
    _open_clipboard()
    try:
        u32.EmptyClipboard()
        h = k32.GlobalAlloc(0x0002, len(data))
        p = k32.GlobalLock(h)
        ctypes.memmove(p, data, len(data))
        k32.GlobalUnlock(h)
        if not u32.SetClipboardData(CF_UNICODETEXT, h):
            raise OSError("SetClipboardData failed")
    finally:
        u32.CloseClipboard()


def procs(names=("ui4me-host.exe", "python.exe", "pythonw.exe")):
    out = []
    for p in psutil.process_iter(["pid", "ppid", "name", "exe", "cmdline", "create_time"]):
        if p.info["name"] and p.info["name"].lower() in names:
            try:
                if p.pid == os.getpid():
                    continue
                sid = wintypes.DWORD()
                k32.ProcessIdToSessionId(p.pid, ctypes.byref(sid))
                out.append({**p.info, "session": sid.value})
            except psutil.Error:
                pass
    return out


def tree(pid):
    try:
        p = psutil.Process(pid)
        return [{"pid": c.pid, "name": c.name(), "ppid": c.ppid()} for c in p.children(recursive=True)]
    except psutil.Error:
        return None


def mem(pid):
    p = psutil.Process(pid)
    m = p.memory_full_info()
    return {"private": m.private, "wset": m.wset, "uss": m.uss, "handles": p.num_handles(),
            "threads": p.num_threads(), "cpu": sum(p.cpu_times()[:2])}


def sha256(path):
    import hashlib
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def spawn(argv, cwd=None, env=None, stdout=None):
    import subprocess
    e = os.environ.copy()
    if env:
        e.update(env)
    out = open(stdout, "ab") if stdout else subprocess.DEVNULL
    p = subprocess.Popen(argv, cwd=cwd, env=e, stdout=out, stderr=subprocess.STDOUT,
                         creationflags=0x00000200)  # CREATE_NEW_PROCESS_GROUP
    return p.pid


def run(argv, timeout=120, cwd=None, env=None):
    import subprocess
    e = os.environ.copy()
    if env:
        e.update(env)
    r = subprocess.run(argv, cwd=cwd, env=e, capture_output=True, timeout=timeout,
                       creationflags=0x08000000)  # CREATE_NO_WINDOW
    return {"rc": r.returncode, "out": r.stdout.decode("utf-8", "replace")[-20000:],
            "err": r.stderr.decode("utf-8", "replace")[-20000:]}


_lab = None
LAB_STATE = Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "ui4me-wlab-state"


def lab(request, reconnect=False):
    """Send one request to the wlab backend control pipe (same user)."""
    global _lab
    from multiprocessing.connection import Client
    if _lab is None or reconnect:
        key = (LAB_STATE / "ctl.key").read_text().strip().encode()
        _lab = Client(r"\\.\pipe\ui4me-wlab", family="AF_PIPE", authkey=key)
    try:
        _lab.send(request)
        return _lab.recv()
    except (OSError, EOFError):
        _lab = None
        raise


def lab_exec(code, quiet=True):
    r = lab({"kind": "exec", "code": code, "quiet": quiet})
    if not r["ok"]:
        raise RuntimeError(r.get("error", "") + "\n" + r.get("trace", ""))
    return r["result"]


def lab_events(since=0.0, kind=None):
    import json as _json
    out = []
    p = LAB_STATE / "events.jsonl"
    if not p.exists():
        return out
    for line in p.open(encoding="utf-8"):
        try:
            e = _json.loads(line)
        except ValueError:
            continue
        if e["t"] >= since and (kind is None or e["kind"] == kind):
            out.append(e)
    return out


def keyboard_layout():
    """Input locale of the foreground window's thread (e.g. 0x0804 Chinese, 0x0409 US)."""
    u32.GetKeyboardLayout.restype = ctypes.c_void_p
    h = u32.GetForegroundWindow()
    tid = u32.GetWindowThreadProcessId(h, None)
    return hex((u32.GetKeyboardLayout(tid) or 0) & 0xFFFF)


def cycles(pid):
    """Exact CPU cycles consumed by a process (QueryProcessCycleTime)."""
    h = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    try:
        c = ctypes.c_ulonglong()
        k32.QueryProcessCycleTime(h, ctypes.byref(c))
        return c.value
    finally:
        k32.CloseHandle(h)


def cpu_hz():
    import winreg
    k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
    return winreg.QueryValueEx(k, "~MHz")[0] * 1_000_000
