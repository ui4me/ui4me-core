"""N06: Microsoft Pinyin + layout in the probe editor (real RDP keyboard input)."""
import time
import wl
from wl import P, win, key, uia, ex
import probe_suite_lib as L

pid = L.launch("N06-probe-run.log")
m = win(L.M)
def text():
    return uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
result = uia.document_text(uia.find(e, control="Edit"))''')
r = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
ed = uia.find(e, control="Edit"); r = ed.CurrentBoundingRectangle
result = [r.left, r.top, r.right, r.bottom]''')
wl.click((r[0] + r[2]) / 2, (r[1] + r[3]) / 2); time.sleep(0.3)
key("ctrl+a", "BackSpace"); time.sleep(0.3)
P("layout", ex("import importlib; importlib.reload(w); result = w.keyboard_layout()"))
key("super+space"); time.sleep(1.2)
P("layout after Win+Space", ex("result = w.keyboard_layout()"))
wl.type_("nihao"); time.sleep(1); wl.wshot("final\\N06-probe-preedit.png"); P("during preedit", repr(text()))
key("space"); time.sleep(0.8); P("commit", repr(text()))
wl.type_("shijie"); time.sleep(0.8); key("Escape"); time.sleep(0.6); P("Escape", repr(text()))
key("shift+Left"); time.sleep(0.2); wl.type_("wen"); time.sleep(0.8); key("space"); time.sleep(0.8); P("compose over selected char", repr(text()))
wl.type_("zhong"); time.sleep(0.8); key("Tab"); time.sleep(1.0)
P("Tab during composition: focus", uia("result = uia.focused()['name']"), "text", repr(text()))
key("shift+Tab"); time.sleep(0.6); wl.type_("hao"); time.sleep(0.6); key("space"); time.sleep(0.8)
P("window responsive after focus change; commit again", repr(text()))
copy = uia(f'''
e = uia.window(pid={m["pid"]}, title="Python reference")
r = uia.find(e, name="Copy").CurrentBoundingRectangle
result = [(r.left + r.right) / 2, (r.top + r.bottom) / 2]''')
wl.type_("shijie"); time.sleep(0.8); wl.click(*copy); time.sleep(1.0)
P("mouse focus change during composition: focus", uia("result = uia.focused()['name']"), "text", repr(text()))
wl.click((r[0] + r[2]) / 2, (r[1] + r[3]) / 2); time.sleep(0.4); key("End"); wl.type_("hao"); time.sleep(0.6); key("space"); time.sleep(0.8)
P("back in editor, compose again", repr(text()))
key("super+space"); time.sleep(1.2); P("layout back", ex("result = w.keyboard_layout()"))
key("ctrl+a", "BackSpace"); wl.type_("ASCII ok"); time.sleep(0.5); P("ASCII after switch back", repr(text()))
wl.activate(L.M); key("ctrl+q"); P("exit", L.wait_exit(pid))
