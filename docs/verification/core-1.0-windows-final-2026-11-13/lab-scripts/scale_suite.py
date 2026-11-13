"""N08 (scale changes, surface recreation) and N14 (RDP disconnect/reconnect, resize) on Windows."""
import time
import wl
import rdpctl
from wl import P, key, uia, ex

C = "Desktop workspace"
HOST = r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"
APP = r"%LOCALAPPDATA%\ui4me-renderer-reference\app.toml"


def el(i):
    return uia(f'''e = uia.window(title="{C}")
r = uia.find(e, automation_id="{i}").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')


def focused():
    f = uia("result = uia.focused()"); return f["id"] or f["name"]


def val(i):
    return uia(f'''e = uia.window(title="{C}")
result = uia.pattern(uia.find(e, automation_id="{i}"), "Value").CurrentValue''')


def state(tag):
    x = wl.win(C, 20)
    s = ex("import ctypes; result = [ctypes.windll.user32.GetSystemMetrics(0), ctypes.windll.user32.GetSystemMetrics(1)]")
    P(f"{tag}: screen {s} window dpi {x['dpi']} outer {x['rect']} client {x['cw']}x{x['ch']} pid {x['pid']}")
    return x


def clicks(tag):
    res = []
    for t in ("notes", "name"):
        wl.click(*el(t)); time.sleep(0.5); res.append((t, focused()))
    P(f"{tag}: clicks", res)


ex(rf'''import os; w.spawn([os.path.expandvars(r"{HOST}"), "run", "--app", os.path.expandvars(r"{APP}")], cwd=os.path.expanduser("~"), stdout=r"F:\ui4me\lab\out\final\N08-N14-host.log")''')
x = state("start 100%"); hp = x["pid"]
ex(f"w.move({x['hwnd']}, 40, 20, 1100, 900)"); time.sleep(1)
wl.click(*el("name")); key("ctrl+a"); wl.type_("state before reconnect"); time.sleep(0.4)
wl.wshot("final\\N08-scale-100.png"); clicks("100%")

P("## N14 RDP disconnect / reconnect")
rdpctl.disconnect(); P("after disconnect: session", rdpctl.session())
time.sleep(10)
P("host alive while disconnected", ex(f"import psutil; result = psutil.pid_exists({hp})"), "| agent still answering in session", ex("result = w.session_id()"))
rdpctl.connect(); P("after reconnect: session", rdpctl.session())
x = state("reconnected 100%"); P("same host pid", x["pid"] == hp, "| Name text preserved", repr(val("name")))
wl.click(*el("name")); key("End"); wl.type_(" + typed after reconnect"); time.sleep(0.4); P("typing after reconnect", repr(val("name")))

P("## N08 scale 150% (RDP desktop scale factor)")
rdpctl.disconnect(); rdpctl.connect(scale=150)
x = state("150%"); time.sleep(1); wl.wshot("final\\N08-scale-150.png"); clicks("150%")
P("text preserved", repr(val("name")))
key("super+space"); time.sleep(1.2); wl.click(*el("notes")); key("ctrl+a", "BackSpace"); wl.type_("nihao"); time.sleep(1); key("space"); time.sleep(0.8)
P("Pinyin at 150%:", repr(val("notes")), ex("result = w.keyboard_layout()")); key("super+space"); time.sleep(1)
sec = [y["hwnd"] for y in wl.wins("UI4ME")]
wl.click(*el("second-window")); time.sleep(2)
s2 = [y for y in wl.wins("UI4ME") if y["hwnd"] not in sec]
P("second window created at 150%:", [(y["title"], y["dpi"], y["cw"], y["ch"]) for y in s2])
for y in s2:
    ex(f"w.post({y['hwnd']}, 0x10)")

P("## N08 back to 100% and RDP resize 1280x800")
rdpctl.disconnect(); rdpctl.connect(scale=100)
x = state("back to 100%"); clicks("100% again")
rdpctl.disconnect(); rdpctl.connect(w=1280, h=800)
x = state("RDP resized to 1280x800"); wl.wshot("final\\N14-rdp-1280x800.png"); clicks("1280x800")
rdpctl.disconnect(); rdpctl.connect()
x = state("restored 1600x1000"); P("same host pid through all reconnects", x["pid"] == hp)
P("second launch forwarded:", "(see N02)")
wl.activate(C); x = wl.win(C); ex(f"w.post({x['hwnd']}, 0x10)")
time.sleep(3); P("host exited after close", not ex(f"import psutil; result = psutil.pid_exists({hp})"))
P("DONE")
