"""Native probe app for workstream N (F-07, F-08, F-17, F-17b) on a real KDE Wayland session.

Usage:  python3 probe_app.py <scenario> <outdir>

Scenarios: focus (F-08, F-17), keys (F-17b shortcuts + click press/release), geom (F-07).
Every document event of interest is appended to <outdir>/events.jsonl. The driver sends
commands by appending JSON lines to <outdir>/cmd.jsonl ({"id": n, "op": ..., args});
replies go to <outdir>/results.jsonl. This lets the driver mix injected input (ydotool),
AT-SPI calls and backend API calls in one timeline.
"""
from __future__ import annotations

import json
import sys
import threading
import time
from pathlib import Path

from ui4me_core import Application

HERE = Path(__file__).resolve().parent
SCENARIO, OUT = sys.argv[1], Path(sys.argv[2])
OUT.mkdir(parents=True, exist_ok=True)
EVENTS = OUT / "events.jsonl"
RESULTS = OUT / "results.jsonl"
CMDS = OUT / "cmd.jsonl"
for f in (EVENTS, RESULTS, CMDS):
    f.write_text("")
lock = threading.Lock()


def log(path: Path, record: dict) -> None:
    record = {"t": round(time.time(), 3), **record}
    with lock, path.open("a") as fh:
        fh.write(json.dumps(record, ensure_ascii=False) + "\n")


KEEP = ("type", "element", "key", "code", "ctrl", "shift", "alt", "meta", "href", "url",
        "x", "y", "button", "value", "selection")
EVENT_TYPES = ("focus", "click", "keydown", "link_activated", "copy", "cut", "paste",
               "input", "pointerdown", "pointerup", "select", "compositionstart",
               "compositionupdate", "compositionend")

with Application.start(name="UI4ME N probe", app_id="org.ui4me.NProbe") as app:
    window = app.open_file(HERE / f"{SCENARIO}.html", title=f"N-probe {SCENARIO}",
                           width=800, height=500)
    doc = window.document

    def on_event(event):
        log(EVENTS, {k: event[k] for k in KEEP if k in event})

    for kind in EVENT_TYPES:
        doc.on(kind, on_event)
    if SCENARIO == "keys":
        # The documented pattern for an app-defined Ctrl+S shortcut (python-api.md):
        doc.prevent_default("editor", "keydown", key="s", ctrl=True)

    def run(cmd: dict):
        op = cmd.pop("op")
        if op == "focus":
            doc.focus(cmd["element"])
            time.sleep(0.4)
            return {"focused": doc.focused_element(), "scroll": doc.scroll()}
        if op == "state":
            out = {"focused": doc.focused_element(), "scroll": doc.scroll(),
                   "window": window.state()}
            for e in cmd.get("rects", []):
                out[f"rect:{e}"] = doc.command("get_rect", element=e)
            for e in cmd.get("selections", []):
                out[f"sel:{e}"] = doc.control_selection(e)
                out[f"value:{e}"] = doc.get_value(e)
            return out
        if op == "scroll_to":
            doc.scroll_to(x=cmd.get("x", 0), y=cmd.get("y", 0), behavior="instant")
            return doc.scroll()
        if op == "set_value":
            doc.set_value(cmd["element"], cmd["value"])
            return doc.get_value(cmd["element"])
        if op == "select":
            doc.set_control_selection(cmd["element"], cmd["start"], cmd["end"])
            return doc.control_selection(cmd["element"])
        if op == "prevent":
            doc.prevent_default(cmd["element"], cmd["event"], key=cmd.get("key"),
                                ctrl=cmd.get("ctrl"), prevent=cmd.get("prevent", True))
            return True
        if op == "clipboard":
            return app.clipboard_text()
        if op == "command":
            return doc.command(cmd.pop("name"), **cmd)
        if op == "quit":
            app.request_quit()
            return "bye"
        raise ValueError(op)

    def control():
        seen = 0
        while not app.rpc.closed:
            lines = CMDS.read_text().splitlines()
            for line in lines[seen:]:
                cmd = json.loads(line)
                cid = cmd.pop("id")
                try:
                    log(RESULTS, {"id": cid, "ok": run(cmd)})
                except Exception as exc:  # report, keep serving
                    log(RESULTS, {"id": cid, "error": repr(exc)})
            seen = len(lines)
            time.sleep(0.1)

    threading.Thread(target=control, daemon=True).start()
    log(RESULTS, {"id": 0, "ok": "ready"})
    app.run()
