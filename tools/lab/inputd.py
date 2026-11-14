#!/usr/bin/env python3
"""Root uinput daemon: absolute pointer + keyboard driven over a unix socket.

Commands (one JSON object per line):
  {"op":"move","x":X,"y":Y}            screen pixels (output geometry W x H)
  {"op":"button","button":"left|right|middle","down":true|false}
  {"op":"wheel","dy":N}
  {"op":"key","code":"KEY_A","down":true|false}
"""
import json, os, socket, sys
from evdev import UInput, ecodes as e, AbsInfo

W, H = int(sys.argv[2]), int(sys.argv[3])
path = sys.argv[1]
MAX = 32767
caps = {
    e.EV_KEY: [e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE],
    e.EV_ABS: [(e.ABS_X, AbsInfo(0, 0, MAX, 0, 0, 0)), (e.ABS_Y, AbsInfo(0, 0, MAX, 0, 0, 0))],
    e.EV_REL: [e.REL_WHEEL, e.REL_HWHEEL],
}
ptr = UInput(caps, name="ui4me-lab-pointer", bustype=e.BUS_USB, vendor=0x1234, product=0x5678)
kbd_keys = [v for k, v in e.ecodes.items() if k.startswith("KEY_") and v < 0x2ff]
kbd = UInput({e.EV_KEY: sorted(set(kbd_keys))}, name="ui4me-lab-keyboard", bustype=e.BUS_USB,
             vendor=0x1234, product=0x5679)
if os.path.exists(path):
    os.unlink(path)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(path)
os.chmod(path, 0o666)
srv.listen(4)
BTN = {"left": e.BTN_LEFT, "right": e.BTN_RIGHT, "middle": e.BTN_MIDDLE}
print("ready", flush=True)
while True:
    conn, _ = srv.accept()
    f = conn.makefile("rw")
    for line in f:
        try:
            c = json.loads(line)
            op = c["op"]
            if op == "move":
                ptr.write(e.EV_ABS, e.ABS_X, round(c["x"] * MAX / (W - 1)))
                ptr.write(e.EV_ABS, e.ABS_Y, round(c["y"] * MAX / (H - 1)))
                ptr.syn()
            elif op == "button":
                ptr.write(e.EV_KEY, BTN[c["button"]], 1 if c["down"] else 0)
                ptr.syn()
            elif op == "wheel":
                ptr.write(e.EV_REL, e.REL_WHEEL, int(c["dy"]))
                ptr.syn()
            elif op == "key":
                kbd.write(e.EV_KEY, e.ecodes[c["code"]], 1 if c["down"] else 0)
                kbd.syn()
            elif op == "quit":
                f.write("bye\n"); f.flush(); conn.close(); sys.exit(0)
            f.write("ok\n")
        except Exception as error:  # report, keep serving
            f.write(f"error {error}\n")
        f.flush()
    conn.close()
