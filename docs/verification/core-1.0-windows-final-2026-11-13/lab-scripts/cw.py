"""Client for the Windows C-lab control pipe (through the in-session agent)."""
import json, time
import wl

C = "UI4ME C-lab"


def req(obj, timeout=60):
    return wl.ex(f"result = w.lab(json.loads({json.dumps(json.dumps(obj, ensure_ascii=False))}))")


def call(method, **params):
    return req({"kind": "call", "method": method, "params": params})


def doc(command, window="main", **kw):
    return req({"kind": "doc", "command": command, "window": window, **kw})


def op(_op, window="main", **args):
    return req({"kind": "doc", "window": window, "command": {"op": _op, **args}})


def code(src):
    r = req({"kind": "exec", "code": src, "quiet": True})
    if not r["ok"]:
        raise RuntimeError(r.get("error"))
    return r["result"]


def events(since=0.0, kind=None):
    return wl.ex(f"result = w.lab_events({since!r}, {kind!r})")


def doc_events(since=0.0, types=None):
    out = []
    for e in events(since, "document.event"):
        d = e["data"]
        ev = d.get("event", d)
        if types is None or ev.get("type") in types:
            out.append(ev)
    return out


def rect(element, window="main"):
    return op("get_rect", window=window, element=element)["result"]["rect"]


def screen(element, fx=0.5, fy=0.5, cap=C, window="main"):
    r = rect(element, window); w = wl.win(cap)
    return w["cx"] + r["x"] + r["width"] * fx, w["cy"] + r["y"] + r["height"] * fy


def click_el(element, button="left", cap=C, window="main"):
    r = rect(element, window); w = wl.win(cap)
    wl.click(w["cx"] + r["x"] + min(r["width"] - 4, r["width"] / 2), w["cy"] + r["y"] + r["height"] / 2, button)
    time.sleep(0.4)


def val(e):
    return op("get_value", element=e).get("result", {}).get("value")


def sel(e):
    return op("get_control_selection", element=e).get("result")
