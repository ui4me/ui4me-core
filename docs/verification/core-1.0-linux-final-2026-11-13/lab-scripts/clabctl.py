"""Client for the completion lab's control socket and event log."""
import json, socket, time
from pathlib import Path

STATE = Path.home() / ".local/state/ui4me-clab"


def req(obj, timeout=60):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(str(STATE / "ctl.sock"))
    f = s.makefile("rwb")
    f.write((json.dumps(obj, ensure_ascii=False) + "\n").encode())
    f.flush()
    r = json.loads(f.readline())
    s.close()
    return r


def call(method, **params):
    return req({"kind": "call", "method": method, "params": params})


def doc(command, window="main", **kw):
    return req({"kind": "doc", "command": command, "window": window, **kw})


def events(since=0.0, kind=None):
    out = []
    p = STATE / "events.jsonl"
    if not p.exists():
        return out
    for line in p.open(encoding="utf-8"):
        e = json.loads(line)
        if e["t"] >= since and (kind is None or e["kind"] == kind):
            out.append(e)
    return out


def doc_events(since=0.0, types=None):
    out = []
    for e in events(since, "document.event"):
        d = e["data"]
        ev = d.get("event", d)
        if types is None or ev.get("type") in types:
            out.append(ev)
    return out


def op(_op, window="main", **args):
    return req({"kind": "doc", "window": window, "command": {"op": _op, **args}})


def click_el(lab, element, caption="C-lab", button="left"):
    r = op("get_rect", element=element)["result"]["rect"]
    w = lab.window(caption)
    lab.click(w["cx"] + r["x"] + min(r["width"] - 4, r["width"] / 2), w["cy"] + r["y"] + r["height"] / 2, name=button)
    import time as _t
    _t.sleep(0.4)
