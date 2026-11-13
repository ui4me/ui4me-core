"""Windows final-binary completion suite C01-C12 on the installed C-lab."""
import base64, json, re, struct, sys, threading, time, zlib
import wl
import cw as c
from wl import P, key, ex, uia

C = c.C
only = set(sys.argv[1:])
_n = [0]
LAB = r"%LOCALAPPDATA%\ui4me-core1-clab"


def restart(width=1000, height=760):
    try:
        c.call("app.request_quit")
    except Exception:
        pass
    ex(r'''
import psutil, time
end = time.time() + 15
while time.time() < end and any("ui4me-core1-clab" in " ".join(p.info["cmdline"] or []) for p in psutil.process_iter(["cmdline"])):
    time.sleep(0.2)
''')
    _n[0] += 1
    ex(rf'''
import os, shutil
st = w.LAB_STATE
(st / "events.jsonl").unlink(missing_ok=True)
w._lab = None
os.environ["CLAB_W"] = "{width}"; os.environ["CLAB_H"] = "{height}"
w.spawn([os.path.expandvars(r"%USERPROFILE%\.cargo\bin\ui4me-host.exe"), "run", "--app", os.path.expandvars(r"{LAB}\app.toml")],
        cwd=os.path.expanduser("~"), stdout=r"F:\ui4me\lab\out\final\C-lab-host-{_n[0]}.log")
''')
    x = wl.win(C, 30); time.sleep(2)
    ex(f"w.move({x['hwnd']}, 300, 20, 1016, 799)"); time.sleep(1)
    P("  [lab started:", [e["data"] for e in c.events(0, "step")], "]")
    return x


def geo():
    x = wl.win(C); return (x["cw"], x["ch"])


def outer():
    return wl.win(C)


if not only or "C01" in only:
    restart()
    P("## C01")
    ws = lambda **k: c.call("window.set", window="main", **k)
    get = lambda: c.call("window.get", window="main")["result"]

    def br(dx, dy):
        x = outer(); rx, ry, rw, rh = x["rect"]  # outer rect includes the invisible resize border
        sx, sy = rx + rw - 4, ry + rh - 4
        wl.drag(sx, sy, sx + dx, sy + dy, steps=30, dt=0.02); time.sleep(0.7); return geo()
    P("initial", geo(), get()["requested"]["size_constraints"])
    P("drag -800,-600 ->", br(-800, -600)); ex(f"w.move({outer()['hwnd']}, 20, 20, 1016, 799)"); time.sleep(0.8); P("drag +700,+300 ->", br(700, 300))
    ws(min_width=600, min_height=400, max_width=900, max_height=700); time.sleep(0.8); P("limits 600x400..900x700 ->", geo())
    ex(f"w.move({outer()['hwnd']}, 300, 40, 916, 739)"); time.sleep(0.6); P("drag -600,-500 ->", br(-600, -500), "| +600,+500 ->", br(600, 500))
    ws(min_width=None, min_height=None); P("min cleared: drag -800,-600 ->", br(-800, -600))
    ws(max_width=None, max_height=None); ex(f"w.move({outer()['hwnd']}, 20, 20, 700, 500)"); time.sleep(0.6); P("max cleared: drag +1000,+400 ->", br(1000, 400))
    before = get()["requested"]["size_constraints"]
    for bad in ({"min_width": 500}, {"min_width": 0, "min_height": 0}, {"min_width": 900, "min_height": 900, "max_width": 800, "max_height": 800}, {"max_width": 100000, "max_height": 100000}):
        r = ws(**bad); P("invalid", bad, "->", r.get("code"), (r.get("error") or "")[:70])
    P("unchanged after invalid", get()["requested"]["size_constraints"] == before)
    h = outer()["hwnd"]; ex(f"w.move({h}, 300, 40, 1016, 776)"); time.sleep(0.6)
    for op, cmd in (("maximize", 3), ("restore", 9), ("minimize", 6), ("restore", 9)):
        ex(f"w.show({h}, {cmd})"); time.sleep(1); o = get()["observed"]
        P(op, {k: o.get(k) for k in ("maximized", "fullscreen", "focused", "minimized", "position", "visible")})
    wl.click(1500, 900); time.sleep(0.8); P("desktop clicked -> observed focused", get()["observed"]["focused"])
    wl.activate(C); time.sleep(0.5); P("caption clicked -> observed focused", get()["observed"]["focused"])
    r = ws(visible=False); time.sleep(0.8); P("visible=False", r.get("ok"), "native window visible:", bool(wl.wins(C)), "observed", get()["observed"]["visible"])
    r = ws(visible=True); time.sleep(0.8); P("visible=True", r.get("ok"), "native window visible:", bool(wl.wins(C)), "observed", get()["observed"]["visible"])
    wl.click(1500, 900); time.sleep(0.6); P("desktop focused; foreground", ex("result = w.foreground()['title']"))
    r = ws(focus=True); time.sleep(0.8); P("focus=True", r.get("ok"), "foreground", ex("result = w.foreground()['title']"), "observed focused", get()["observed"]["focused"])
    r = ws(x=10, y=10); time.sleep(0.8); P("x=10,y=10", r.get("ok"), "native outer", outer()["rect"][:2], "observed", get()["observed"]["position"])
    r = ws(minimized=True); time.sleep(0.8); P("minimized=True", r.get("ok"), "iconic", ex(f"result = w.info({h})['iconic']"), "observed", get()["observed"]["minimized"])
    r = ws(minimized=False); time.sleep(0.8); P("minimized=False", r.get("ok"), "iconic", ex(f"result = w.info({h})['iconic']"), "observed", get()["observed"]["minimized"])

    r = c.call("window.create", title="hidden C01", width=300, height=200, visible=False); time.sleep(0.8)
    hid = r.get("result", {}).get("window_id") if r.get("ok") else None
    P("hidden create", r.get("ok"), r.get("error"), "listed visible:", bool(wl.wins("hidden C01")), "native exists:", bool(ex("result = w.windows('hidden C01', visible=False)")))
    if hid:
        c.call("window.set", window_id=hid, visible=True); time.sleep(0.8); P("  shown ->", bool(wl.wins("hidden C01")))
        c.call("window.request_close", window_id=hid); time.sleep(0.8); P("  closed ->", bool(ex("result = w.windows('hidden C01', visible=False)")))
    wl.click(1500, 900); time.sleep(0.5)
    for kind in ("informational", "critical"):
        P("attention", kind, c.call("window.request_attention", window="main", kind=kind).get("result")); time.sleep(1.5)
    wl.wshot("final\\C01-attention-taskbar.png")
    P("attention cancel", c.call("window.request_attention", window="main", kind=None))
    ws(min_width=480, min_height=360, max_width=1400, max_height=1000)

if not only or "C02" in only:
    if _n[0] == 0:
        restart()
    P("## C02")
    css = "@media (prefers-color-scheme: dark){#gridcap{color:rgb(200,1,1)}} @media (prefers-color-scheme: light){#gridcap{color:rgb(1,1,200)}} #gridnote{transition-duration:1s} @media (prefers-reduced-motion: reduce){#gridnote{transition-duration:0s}} @media (forced-colors: active){#gridnote{outline-style:solid}}"
    c.op("add_stylesheet", css=css)

    def probe(tag):
        env = c.op("get_environment").get("result"); pref = c.call("desktop.preferences").get("result", {})
        cs = c.op("get_computed_style", element="gridcap", properties=["color"])["result"]["properties"]["color"]["value"]
        tr = c.op("get_computed_style", element="gridnote", properties=["transition-duration"])["result"]["properties"]["transition-duration"]["value"]
        P(f"{tag}: prefs cs={pref.get('color_scheme')} rm={pref.get('reduced_motion')} hc={pref.get('high_contrast')} src={pref.get('source')} | env cs={env['color_scheme']}({env['color_scheme_source']}) rm={env['reduced_motion']}({env['reduced_motion_source']}) hc={env.get('high_contrast')} | gridcap={cs} gridnote={tr}")

    def scheme(light):
        ex(rf'''
import winreg, ctypes
k = winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", 0, winreg.KEY_SET_VALUE)
winreg.SetValueEx(k, "AppsUseLightTheme", 0, winreg.REG_DWORD, {1 if light else 0}); winreg.CloseKey(k)
r = ctypes.c_size_t()
ctypes.windll.user32.SendMessageTimeoutW(0xFFFF, 0x001A, 0, "ImmersiveColorSet", 2, 3000, ctypes.byref(r))
''')
        time.sleep(2)

    def motion(on):
        # SPI_SETCLIENTAREAANIMATION (0x1043); Windows "Show animations" off = reduce.
        ex(f"import ctypes; ctypes.windll.user32.SystemParametersInfoW(0x1043, 0, ctypes.c_void_p({1 if on else 0}), 3)")
        time.sleep(2)
    orig = ex(r'''
import winreg, ctypes
k = winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
v = winreg.QueryValueEx(k, "AppsUseLightTheme")[0]
b = ctypes.c_int(); ctypes.windll.user32.SystemParametersInfoW(0x1042, 0, ctypes.byref(b), 0)
result = [v, b.value]''')
    P("original AppsUseLightTheme, ClientAreaAnimation", orig)
    probe("initial"); t = time.time(); scheme(not orig[0]); probe("OS flipped"); P("  notifications", len(c.events(t, "desktop.preferences_changed")))
    scheme(True); probe("OS light"); scheme(False); probe("OS dark")
    c.op("set_environment", color_scheme="light"); scheme(False); probe("override light + OS dark")
    c.op("set_environment", color_scheme="system"); time.sleep(1); probe("override system")
    c.op("set_environment", color_scheme="dark"); scheme(True); probe("override dark + OS light")
    c.op("set_environment", color_scheme="system"); time.sleep(1); probe("override system")
    t = time.time(); motion(False); probe("OS animations off"); P("  notifications", len(c.events(t, "desktop.preferences_changed")))
    motion(True); probe("OS animations on")
    c.op("set_environment", reduced_motion="reduce"); probe("rm reduce"); c.op("set_environment", reduced_motion="no-preference"); probe("rm no-preference"); c.op("set_environment", reduced_motion="system"); probe("rm system")
    scheme(bool(orig[0])); motion(bool(orig[1])); probe("restored")
    hp = outer()["pid"]
    a = ex(f"result = w.mem({hp})"); time.sleep(30); b = ex(f"result = w.mem({hp})")
    P("idle 30 s host CPU seconds", round(b["cpu"] - a["cpu"], 3), "handles", a["handles"], "->", b["handles"])

if not only or "C04" in only:
    restart()
    P("## C04")
    val, sel = c.val, c.sel
    P("t1", repr(val("t1")))
    c.op("set_control_selection", element="t1", start=1, end=4, direction="forward"); P("forward", sel("t1"))
    c.op("set_control_selection", element="t1", start=2, end=4, direction="backward"); P("backward", sel("t1"))
    r = c.op("set_control_selection", element="t1", start=3, end=5); P("split surrogate", r.get("code"), "| unchanged", sel("t1"))
    r = c.op("replace_control_range", element="t1", replacement="X", start=3, end=4); P("replace split", r.get("code"), repr(val("t1")))
    c.op("replace_control_range", element="t1", replacement="ZZ", start=2, end=4, selection_mode="select"); P("replace emoji", repr(val("t1")), sel("t1"))
    c.op("edit_command", command="undo", element="t1"); P("undo", repr(val("t1"))); c.op("edit_command", command="redo", element="t1"); P("redo", repr(val("t1"))); c.op("edit_command", command="undo", element="t1"); P("undo", repr(val("t1")))
    c.click_el("t1"); key("Home"); offs = []
    for i in range(7):
        key("Right"); time.sleep(0.15); offs.append(sel("t1")["start"])
    P("caret offsets", offs)
    key("Left", "BackSpace"); time.sleep(0.3); P("Backspace at 8", repr(val("t1"))); key("ctrl+z"); time.sleep(0.3)
    c.op("set_control_selection", element="t1", start=2, end=2); key("Delete"); time.sleep(0.3); P("Delete at 2", repr(val("t1"))); key("ctrl+z"); time.sleep(0.3); P("restored", repr(val("t1")))
    for el in ("ro", "cancel-field"):
        c.click_el(el); b = val(el); key("End"); wl.type_("zz"); time.sleep(0.4); P("typing", el, b, "->", val(el))
    wl.clip_set("pasted"); c.click_el("cancel-field"); key("ctrl+v"); time.sleep(0.5); P("paste cancel-field", val("cancel-field"))
    P("programmatic replace cancel-field", c.op("replace_control_range", element="cancel-field", replacement="prog", start=0, end=0).get("result"), val("cancel-field"))
    P("beforeinput cancel-field", [(e.get("input_type"), e.get("default_prevented")) for e in c.doc_events(0, {"beforeinput"}) if e.get("element") == "cancel-field"])
    for el in ("t2", "mail", "pw"):
        c.op("set_value", element=el, value="")
    lay = lambda: ex("result = w.keyboard_layout()")

    def ime(el, focus="pointer"):
        if focus == "pointer":
            c.click_el(el)
        else:
            c.op("focus", element=el); time.sleep(0.4)
        key("End"); a = lay(); key("super+space"); time.sleep(1.0); b = lay()
        wl.type_("nihao"); time.sleep(0.9); pre = val(el); key("space"); time.sleep(0.7); post = val(el)
        key("super+space"); time.sleep(0.8); return f"layout {a}->{b} preedit={pre!r} commit={post!r} back={lay()}"
    for el in ("t2", "mail", "ta", "pw", "ro"):
        P("IME", el, ime(el))
    c.op("set_value", element="t2", value=""); P("IME t2 programmatic focus", ime("t2", "program")); wl.wshot("final\\C04-after-ime.png")
    # The focus target must lie outside Microsoft Pinyin's candidate window (it opens under t2
    # and covers "mail"; a click there picks a candidate in the IME's own window).
    c.op("set_value", element="t2", value=""); c.op("set_value", element="req", value="")
    c.click_el("t2"); key("super+space"); time.sleep(1.0); wl.type_("zhong"); time.sleep(0.8); wl.wshot("final\\C04-preedit-t2.png")
    cand = [x["rect"] for x in wl.wins() if x["class"].startswith("Microsoft.IME.UIManager.CandidateWindow")]
    rx, ry = c.screen("req"); P("candidate window", cand, "click target req at", (round(rx), round(ry)))
    c.click_el("req"); time.sleep(0.8); P("focus change mid-composition: t2", repr(val("t2")), "req", repr(val("req")), "focused", c.op("get_focused_element").get("result", {}).get("element"))
    wl.type_("wen"); time.sleep(0.7); key("space"); time.sleep(0.6); P("  next composition in req", repr(val("req")), "| t2", repr(val("t2")))
    c.click_el("t2"); key("End"); wl.type_("shi"); time.sleep(0.6); key("Escape"); time.sleep(0.5); P("Escape in t2", repr(val("t2")))
    key("super+space"); time.sleep(1.0); P("layout back", lay())

if not only or "C05" in only:
    if _n[0] == 0:
        restart()
    P("## C05")
    val = c.val
    P("form_data", c.op("get_form_data", element="form-a")["result"])
    P("form_data sub2", c.op("get_form_data", element="form-a", submitter="sub2")["result"]["submitter"])
    t = time.time(); P("request_submit sub1", c.op("request_submit", element="form-a", submitter="sub1")["result"]); time.sleep(0.4); P("  events", [(e["submitter"], e["default_prevented"]) for e in c.doc_events(t, {"submit"})])
    t = time.time(); c.click_el("sub2"); time.sleep(0.5); P("click Send", [(e["submitter"], e["default_prevented"]) for e in c.doc_events(t, {"submit"})])
    t = time.time(); c.click_el("req"); key("End", "Return"); time.sleep(0.5); P("Enter", [(e["submitter"], e["default_prevented"]) for e in c.doc_events(t, {"submit"})])
    t = time.time(); c.op("focus", element="sub1"); key("space"); time.sleep(0.5); P("Space on Save", [(e["submitter"]) for e in c.doc_events(t, {"submit"})])
    c.op("set_value", element="req", value=""); P("valueMissing", c.op("get_validity", element="req")["result"]["flags"]["valueMissing"])
    t = time.time(); P("submit invalid", c.op("request_submit", element="form-a", submitter="sub1")["result"], "submits", len(c.doc_events(t, {"submit"})), "invalid", len(c.doc_events(t, {"invalid"})))
    c.op("set_value", element="req", value="filled"); c.op("set_custom_validity", element="req", message="Custom problem")
    t = time.time(); P("custom invalid", c.op("request_submit", element="form-a")["result"], "submits", len(c.doc_events(t, {"submit"})))
    c.op("set_custom_validity", element="req", message=""); t = time.time(); P("cleared", c.op("request_submit", element="form-a")["result"], "submits", len(c.doc_events(t, {"submit"})))
    c.click_el("req"); key("End"); wl.type_("XYZ"); c.op("set_checked", element="cb", checked=True); c.op("set_selected_values", element="pick", values=["Apple"])
    P("before reset", val("req"), c.op("get_checked", element="cb")["result"], val("pick")); c.click_el("rst"); time.sleep(0.4)
    P("after reset", val("req"), c.op("get_checked", element="cb")["result"], val("pick")); c.click_el("req"); key("ctrl+z"); time.sleep(0.3); P("Ctrl+Z after reset", val("req"))
    ex(r'''
import os
os.makedirs(r"F:\ui4me\lab\files\multi", exist_ok=True)
for n in ("one.txt", "dva ž.txt"):
    open(os.path.join(r"F:\ui4me\lab\files\multi", n), "w", encoding="utf-8").write(n)''')

    def pick(el, text=None, cancel=False):
        c.click_el(el); time.sleep(2.5)
        dl = [(x["title"], x["owner"] == outer()["hwnd"]) for x in wl.wins() if x["class"] == "#32770"]
        if cancel:
            key("Escape")
        else:
            wl.clip_set(text); time.sleep(0.3); key("ctrl+a", "ctrl+v"); time.sleep(0.5); key("Return")
        time.sleep(2); return dl
    P("file1", pick("file1", r"F:\ui4me\lab\files\žluťoučký 日本.txt"), c.op("get_files", element="file1")["result"])
    P("file2", pick("file2", r'"F:\ui4me\lab\files\multi\one.txt" "F:\ui4me\lab\files\multi\dva ž.txt"'), c.op("get_files", element="file2")["result"])
    wl.wshot("final\\C05-files-selected.png"); P("form_data files", [x for x in c.op("get_form_data", element="form-a")["result"]["entries"] if x["kind"] == "file"])
    P("file1 cancel", pick("file1", cancel=True), c.op("get_files", element="file1")["result"])
    c.click_el("rst"); time.sleep(0.5); P("after reset", c.op("get_files", element="file1")["result"], c.op("get_files", element="file2")["result"]); wl.wshot("final\\C05-after-reset.png")

if not only or "C07" in only:
    if _n[0] == 0:
        restart()
    P("## C07")
    val = c.val
    c.op("scroll_into_view", element="s-c03", block="start"); time.sleep(1.5)
    at = c.screen
    ox, oy = at("ev-outer", 0.02); ix, iy = at("ev-inner"); wl.move(ox - 40, oy - 60); time.sleep(0.4); t = time.time()
    for x, y in ((ox + 5, oy), (ix, iy), (ox + 5, oy), (ox - 60, oy - 60)):
        wl.move(x, y); time.sleep(0.4)
    P("boundary", [(e["type"], e.get("element"), e.get("related_element")) for e in c.doc_events(t, {"mouseover", "mouseout", "mouseenter", "mouseleave"}) if e.get("element") in ("ev-outer", "ev-inner")])
    t = time.time(); wl.click(ix, iy, "right"); time.sleep(0.5); P("right", [(e["type"], e.get("button")) for e in c.doc_events(t, {"contextmenu", "auxclick", "mousedown", "mouseup", "click"})])
    t = time.time(); wl.click(ix, iy, "middle"); time.sleep(0.5); P("middle", [(e["type"], e.get("button")) for e in c.doc_events(t, {"auxclick", "click", "mousedown", "mouseup"})])
    t = time.time(); wl.click(*at("ctx-cancel"), "right"); time.sleep(0.5); P("ctx-cancel", [(e["type"], e.get("default_prevented")) for e in c.doc_events(t, {"contextmenu"})])
    t = time.time(); wl.dblclick(*at("dbl")); time.sleep(0.5); P("dbl", [(e["type"], e.get("detail")) for e in c.doc_events(t, {"click", "dblclick"})])
    c.op("set_value", element="t2", value=""); c.click_el("t2"); t = time.time(); wl.type_("ab"); time.sleep(0.4)
    P("typing events", [(e["type"], e.get("input_type"), e.get("data")) for e in c.doc_events(t, {"beforeinput", "input", "selectionchange"})])
    c.op("scroll_into_view", element="s-c03", block="start"); time.sleep(1.5); c.op("set_value", element="t2", value="")
    x, y = at("block-python"); t = time.time(); wl.click(x, y); tb = time.time(); time.sleep(0.6)
    wl.click(*at("t2")); wl.type_("typed while blocked"); time.sleep(0.3); v = val("t2")
    wl.move(*at("ev-outer"))
    for _ in range(3):
        wl.xdo("click", 5); time.sleep(0.1)
    time.sleep(0.4)
    P("%.1f s into block: value %r, t2 rect y %.0f, block log %s" % (time.time() - tb, v, c.rect("t2")["y"], [e["data"] for e in c.events(t, "python-block")]))
    time.sleep(9); P("block log", [(k, round(vv - tb, 2)) for b in [e["data"] for e in c.events(t, "python-block")] for k, vv in b.items()])

if not only or "C08" in only:
    if _n[0] == 0:
        restart()
    P("## C08")
    P(uia(f'''
e = uia.window(title="{C}")
out = {{"app/window name": e.CurrentName}}
nodes = uia.dump(e, depth=30, max_nodes=900)
grid = [d for d in nodes if d["type"] in ("Table", "DataGrid") and "Inventory" in (d["name"] or "")]
out["grid"] = grid[:1]
g = uia.find(e, name=grid[0]["name"]) if grid else None
if g:
    gp = uia.pattern(g, "Grid"); out["Grid pattern"] = [gp.CurrentRowCount, gp.CurrentColumnCount] if gp else None
    cells = []
    for d in uia.dump(g, depth=4):
        if d["type"] in ("DataItem", "HeaderItem", "Header") and d["name"]:
            cells.append((d["type"], d["name"]))
    out["cells"] = cells[:16]
    try:
        item = gp.GetItem(1, 1); gi = item.GetCurrentPattern(uia.U.UIA_GridItemPatternId)
        gi = gi.QueryInterface(uia.U.IUIAutomationGridItemPattern) if gi else None
        out["GetItem(1,1)"] = [item.CurrentName, gi.CurrentRow if gi else None, gi.CurrentColumn if gi else None]
    except Exception as err:
        out["GetItem(1,1)"] = repr(err)
rel = [(d["type"], d["name"], {{k: d[k] for k in ("labeled_by", "controller_for", "described_by", "flows_to") if k in d}}) for d in nodes if any(k in d for k in ("labeled_by", "controller_for", "described_by", "flows_to"))]
out["relations"] = rel
tree = [d for d in nodes if d["type"] == "Tree" and d["name"] == "Folders"]
if tree:
    t = uia.find(e, name="Folders", control="Tree")
    out["tree"] = [(d["name"][:12], d.get("level"), d.get("pos"), d.get("setsize"), d.get("selected"), d.get("expand")) for d in uia.dump(t, depth=8) if d["type"] == "TreeItem"]
live = [d for d in nodes if d["id"] == "live"]
out["live"] = live
result = out'''))
    P("busy:", uia(f'''
e = uia.window(title="{C}")
result = None'''))
    c.op("set_attribute", element="live", name="aria-busy", value="true"); time.sleep(0.5)
    P("aria-busy true -> UIA ItemStatus/IsBusy", uia(f'''
e = uia.window(title="{C}")
l = uia.find(e, automation_id="live")
result = [l.GetCurrentPropertyValue(uia.U.UIA_ItemStatusPropertyId), l.GetCurrentPropertyValue(uia.U.UIA_AriaPropertiesPropertyId)]'''))
    c.op("set_attribute", element="live", name="aria-busy", value="false")
    P("live region events:", uia(f'''
e = uia.window(title="{C}")
uia.listen(e, ("LiveRegionChanged",))
result = None'''))
    t0 = time.time()
    c.op("set_text", element="live", text="Live update 1"); time.sleep(1.0)
    c.op("set_attribute", element="live", name="aria-busy", value="true"); c.op("set_text", element="live", text="Busy update (suppressed?)"); time.sleep(1.0)
    c.op("set_attribute", element="live", name="aria-busy", value="false"); time.sleep(1.0)
    P("  events", uia(f"result = [(round(x['t'] - {t0}, 2), x['event'], x.get('name')) for x in uia.events({t0})]; uia.unlisten()"))
    c.op("scroll_into_view", element="s-c08", block="start"); time.sleep(1)
    for tgt, lbl in (("far", "paragraph (id far)"), (None, "static-text leaf 'Far away'")):
        c.op("scroll_to", element="scroller", x=0, y=0); time.sleep(0.5); c.op("set_control_selection", element="t1", start=1, end=3); s0 = c.sel("t1")
        r = uia(f'''
e = uia.window(title="{C}")
el = uia.find(e, automation_id="{tgt}") if {tgt!r} else [x for x in [e.FindFirst(uia.U.TreeScope_Descendants, uia.A().CreatePropertyCondition(uia.U.UIA_ControlTypePropertyId, uia.U.UIA_TextControlTypeId))]][0]
if not {tgt!r}:
    a = uia.A(); arr = e.FindAll(uia.U.TreeScope_Descendants, a.CreatePropertyCondition(uia.U.UIA_ControlTypePropertyId, uia.U.UIA_TextControlTypeId))
    el = [arr.GetElement(i) for i in range(arr.Length) if "Far away" in (arr.GetElement(i).CurrentName or "")][-1]
si = uia.pattern(el, "ScrollItem")
if si:
    si.ScrollIntoView(); time.sleep(0.8); result = ["ScrollIntoView ok", el.CurrentName[:30]]
else:
    result = ["no ScrollItem pattern", el.CurrentName[:30]]''')
        P(f"ScrollIntoView({lbl})", r, "scroller y", c.op("get_scroll", element="scroller")["result"]["y"], "selection unchanged", c.sel("t1")["start"] == s0["start"])
    c.op("set_control_selection", element="ta", start=2, end=5); s0 = c.sel("ta"); c.op("scroll_into_view", element="s-c09", block="end"); time.sleep(0.8)
    y0 = c.rect("ta")["y"]
    r = uia(f'''
e = uia.window(title="{C}")
a = uia.A(); arr = e.FindAll(uia.U.TreeScope_Descendants, a.CreatePropertyCondition(uia.U.UIA_ControlTypePropertyId, uia.U.UIA_EditControlTypeId))
ta = [arr.GetElement(i) for i in range(arr.Length) if "line one" in (uia.document_text(arr.GetElement(i)) if uia.pattern(arr.GetElement(i), "Text") else "")][0]
rg = uia.pattern(ta, "Text").DocumentRange.Clone()
rg.MoveEndpointByUnit(uia.U.TextPatternRangeEndpoint_Start, uia.U.TextUnit_Character, 9)
rg.MoveEndpointByUnit(uia.U.TextPatternRangeEndpoint_End, uia.U.TextUnit_Character, 12 - len(rg.GetText(-1)) - 9 if False else -(len(uia.document_text(ta)) - 12))
rg.ScrollIntoView(True); time.sleep(0.8)
result = rg.GetText(-1)''')
    P("text-range ScrollIntoView", repr(r), "y", y0, "->", c.rect("ta")["y"], "selection unchanged", c.sel("ta")["start"] == s0["start"])
    P("combo select item 2:", uia(f'''
e = uia.window(title="{C}")
cb = uia.find(e, name="Fruit", control="ComboBox")
items = [d["name"] for d in uia.dump(cb, depth=2) if d["type"] == "ListItem"]
uia.pattern(uia.find(cb, name=items[2]), "SelectionItem").Select(); time.sleep(0.5)
result = [items, uia.pattern(cb, "Value").CurrentValue]'''), c.val("combo"))
    P("stale element after replacement:", uia(f'''
e = uia.window(title="{C}")
uia._stale = uia.find(e, name="Fruit", control="ComboBox")
result = uia._stale.CurrentName'''))
    c.req({"kind": "load", "page": "a.html"}); time.sleep(1)
    P("  ->", uia('''
try:
    result = ["still alive?", uia._stale.CurrentName, uia._stale.CurrentBoundingRectangle.left]
except Exception as err:
    result = ["rejected", repr(err)[:120]]'''))
    c.req({"kind": "load", "page": "fixture.html"}); time.sleep(1)

if not only or "C03" in only:
    restart()  # startup callback registrations belong to the first document
    P("## C03")
    q = c.op("query_nodes", selector="ul.anon > li")["result"]; refs = [n["node"] for n in q["nodes"]]; P("query_nodes", q)
    P("set_text beta", c.op("node_batch", ops=[{"op": "set_text", "node": refs[1], "text": "BETA (mutated)"}]).get("ok"))
    P("valid+stale batch", c.op("node_batch", ops=[{"op": "set_text", "node": refs[0], "text": "SHOULD ROLL BACK"}, {"op": "set_text", "node": "node:9:99999:1", "text": "x"}]).get("error"), "| first", c.op("query_nodes", selector="ul.anon > li")["result"]["nodes"][0])
    wl.wshot("final\\C03-after-mutation.png")
    P("remove gamma", c.op("node_batch", ops=[{"op": "remove_element", "node": refs[2]}]).get("ok"), "| insert replacement", c.op("node_batch", ops=[{"op": "insert_html", "node": refs[1], "position": "afterend", "html": '<li id="repl">replacement</li>'}]).get("ok"))
    r = c.op("node_batch", ops=[{"op": "set_text", "node": refs[2], "text": "stale"}]); P("reuse removed handle", r.get("code"), r.get("error"), "| replacement", c.op("get_text", element="repl")["result"])
    c.op("scroll_into_view", element="s-c03", block="start"); time.sleep(1)
    t = time.time(); c.click_el("nested-rpc"); time.sleep(1); P("nested RPC", c.op("get_text", element="nested-out")["result"])
    t = time.time(); c.click_el("self-remove"); time.sleep(2.5)
    P("self-remove", [e["data"] for e in c.events(t, "self-remove")], "clicks", len([e for e in c.doc_events(t, {"click"}) if e.get("element") == "self-remove"]), "recreated", c.op("get_text", element="self-remove")["result"])
    wl.wshot("final\\C03-after-removal.png")
    ids = c.req({"kind": "ids"})["result"]; old = ids["main"]["document_id"]; keep = refs[1]
    c.req({"kind": "load", "page": "a.html"})
    r = c.req({"kind": "rawdoc", "params": {"window_id": ids["main"]["window_id"], "document_id": old, "command": {"op": "get_text", "element": "nested-out"}}}); P("old document id", r.get("code"), r.get("error"))
    r = c.op("node_batch", ops=[{"op": "set_text", "node": keep, "text": "x"}]); P("old node on new document", r.get("code"), r.get("error"))
    c.req({"kind": "load", "page": "fixture.html"}); time.sleep(1)

if not only or "C09" in only:
    if _n[0] == 0:
        restart()
    P("## C09")

    def png(w, h, rgba):
        raw = b"".join(b"\0" + bytes(rgba) * w for _ in range(h))
        ch = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
        return b"\x89PNG\r\n\x1a\n" + ch(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + ch(b"IDAT", zlib.compress(raw)) + ch(b"IEND", b"")
    ent = lambda p, d, m="image/png": {"path": p, "data_url": "data:%s;base64,%s" % (m, base64.b64encode(d).decode())}
    reg = lambda n, e: c.op("register_resource_bundle", name=n, entries=e)
    c.op("scroll_into_view", element="s-c09", block="end"); time.sleep(0.8)

    def color():
        x, y = c.screen("res-img"); return tuple(ex(f"result = list(w.pixel({int(x)}, {int(y)}))"))[:3]
    P("red", reg("bundle", [ent("a.png", png(48, 48, (255, 0, 0, 255)))]).get("ok"), color()); wl.wshot("final\\C09-red.png")
    P("blue", reg("bundle", [ent("a.png", png(48, 48, (0, 0, 255, 255)))])["result"], color()); wl.wshot("final\\C09-blue.png")
    for lbl, es in (("../x", [ent("../x.png", b"x")]), ("/x", [ent("/x.png", b"x")]), ("a\\b", [ent("a\\b.png", b"x")]), ("257", [ent(f"e{i}.png", b"x") for i in range(257)]), (">1MiB", [ent("b.bin", b"\0" * (1024 * 1024 + 1), "application/octet-stream")])):
        r = reg("bundle", es); P("invalid", lbl, r.get("code"), (r.get("error") or "")[:70])
    time.sleep(0.6); P("old bundle still shown", color())
    r = reg("other", [ent("C:/x.png", png(4, 4, (0, 255, 0, 255)))]); P("virtual path segment 'C:' (in-memory bundle, no filesystem)", r.get("ok"), r.get("error")); c.op("unregister_resource_bundle", name="other")
    ok = 0; last = None
    for i in range(20):
        r = reg(f"agg{i}", [ent("b.bin", bytes([i]) * (1000 * 1024), "application/octet-stream")])
        if r.get("ok"):
            ok += 1
        else:
            last = r.get("error"); break
    P("aggregate accepted", ok, last)
    for i in range(ok):
        c.op("unregister_resource_bundle", name=f"agg{i}")
    n = 0; last = None
    for i in range(70):
        r = reg(f"n{i}", [ent("t.txt", b"x", "text/plain")])
        if r.get("ok"):
            n += 1
        else:
            last = r.get("error"); break
    P("count accepted", n, last)
    for i in range(n):
        c.op("unregister_resource_bundle", name=f"n{i}")
    P("stats", json.dumps(c.op("get_resource_stats")["result"]))
    P("purge displayed", c.op("purge_unused_resources")["result"]["purged"], color())
    c.op("remove_element", element="res-img"); time.sleep(0.5); P("purge after removal", json.dumps(c.op("purge_unused_resources")["result"]))
    P("unregister", c.op("unregister_resource_bundle", name="bundle")["result"], json.dumps(c.op("get_resource_stats")["result"]["bundles"]))
    reg("broken", [ent("bad.png", b"\x89PNG\r\n\x1a\nGARBAGE")]); c.op("insert_html", element="s-c09", position="beforeend", html='<img id="badimg" src="ui4me-res://broken/bad.png">'); time.sleep(1)
    P("decode failure", sorted(set(re.findall(r'"code": "([A-Z_]+)"', json.dumps(c.op("get_diagnostics")["result"])))), c.op("has_element", element="badimg")["result"])

if not only or "C12" in only:
    if _n[0] == 0:
        restart()
    P("## C12 animation")
    hp = outer()["pid"]; wl.click(1550, 950); time.sleep(2)

    def m(tag, secs=20):
        a = ex(f"result = w.mem({hp})"); t0 = time.time(); time.sleep(secs); b = ex(f"result = w.mem({hp})")
        P(f"{tag}: host {100 * (b['cpu'] - a['cpu']) / (time.time() - t0):.2f}% private {b['private'] / 2**20:.1f} MiB handles {b['handles']}")
    m("baseline"); s = c.op("add_stylesheet", css="@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}")["result"]["sheet"]
    c.op("add_class", element="dbl", name="spin"); m("animation"); c.op("remove_stylesheet", sheet=s); m("sheet removed")
    s = c.op("add_stylesheet", css="@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}")["result"]["sheet"]; m("re-added", 10)
    c.op("replace_stylesheet", sheet=s, css=".spin{color:red}"); m("replaced static")
    c.op("replace_stylesheet", sheet=s, css="@keyframes spin{to{transform:rotate(360deg)}} .spin{animation:spin 2s linear infinite}"); time.sleep(3); c.op("remove_class", element="dbl", name="spin"); m("class removed")
P("DONE")
