"""C01-C12 native observation fixture for UI4ME Core 1.0 (test-only application).

Launched by the installed host like any application. It opens fixture.html and
serves a local control socket so an operator can issue real protocol calls while
real input is delivered to the native window. Every notification is logged.
Not part of the product; it uses only the supplied application facade.
"""
from __future__ import annotations

import faulthandler
import json
import os
import socket
import sys
import threading
import time
import traceback
from pathlib import Path

from application import Application
from client import RpcError

HERE = Path(__file__).resolve().parent
STATE = Path(os.environ.get("CLAB_STATE") or (Path(os.environ.get("LOCALAPPDATA", Path.home())) / "ui4me-wlab-state"))
STATE.mkdir(parents=True, exist_ok=True)
LOG = STATE / "events.jsonl"
faulthandler.enable(open(STATE / "fault.txt", "w"))
PIPE = r"\\.\pipe\ui4me-wlab"
KEYFILE = STATE / "ctl.key"
log_lock = threading.Lock()


def log(kind, data):
    with log_lock, LOG.open("a", encoding="utf-8") as f:
        f.write(json.dumps({"t": time.time(), "kind": kind, "data": data}, ensure_ascii=False) + "\n")


def main():
    with Application.from_environment() as app:
        rpc = app.rpc
        for method in ("document.event", "window.event", "document.error", "desktop.preferences_changed"):
            original = rpc._notifications.get(method)
            def wrapped(params, _m=method, _o=original):
                log(_m, params)
                if _o:
                    _o(params)
            rpc._notifications[method] = wrapped
        windows = {}
        width = int(os.environ.get("CLAB_W", "1000"))
        height = int(os.environ.get("CLAB_H", "760"))
        main_window = app.open_file(HERE / os.environ.get("CLAB_PAGE", "fixture.html"),
                                    title="UI4ME C-lab", width=width, height=height,
                                    min_width=480, min_height=360, max_width=1400, max_height=1000)
        windows["main"] = main_window
        doc = main_window.document
        log("step", "window opened")
        # Pre-registered default-action policies (never decided late in Python).
        for element, event in (("cancel-field", "beforeinput"), ("ctx-cancel", "contextmenu"), ("form-a", "submit")):
            try:
                doc.prevent_default(element, event)
            except Exception as error:
                log("step", f"prevent_default {element} {event} failed: {type(error).__name__}: {error}")
        log("step", "policies set")
        lab_state = {}
        hold = threading.Event()
        hold.set()

        def blocker(event):
            # Optional: block the ordered application worker (C07 responsiveness).
            if event.get("element") == "block-python":
                log("python-block", {"begin": time.time()})
                time.sleep(float(os.environ.get("CLAB_BLOCK", "8")))
                log("python-block", {"end": time.time()})
        doc.on("click", blocker, element="block-python")

        def self_remove(event):
            # Callback removes its own target and performs another RPC.
            try:
                doc.remove("self-remove")
                doc.insert_html("self-remove-host", '<button id="self-remove">Recreated</button>')
                log("self-remove", {"ok": True})
            except RpcError as e:
                log("self-remove", {"error": str(e)})
        doc.on("click", self_remove, element="self-remove")

        def nested(event):
            doc.set_text("nested-out", "nested RPC at %.3f; focus=%s" % (time.time(), json.dumps(doc.focused_element())))
        doc.on("click", nested, element="nested-rpc")

        def serve():
            import secrets
            from multiprocessing.connection import Listener
            key = secrets.token_hex(16)
            KEYFILE.write_text(key)
            server = Listener(PIPE, family="AF_PIPE", authkey=key.encode())
            while not rpc.shutdown_requested.is_set():
                connection = server.accept()
                threading.Thread(target=handle, args=(connection,), daemon=True).start()

        def handle(connection):
            with connection:
                while True:
                    try:
                        request = connection.recv()
                    except (EOFError, OSError):
                        return
                    try:
                        result = dispatch(request)
                        reply = {"ok": True, "result": result}
                    except RpcError as e:
                        reply = {"ok": False, "code": e.code, "error": str(e), "data": getattr(e, "data", None)}
                    except Exception as e:  # report, never hide
                        reply = {"ok": False, "error": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()}
                    if request.get("kind") != "exec" or not request.get("quiet"):
                        log("ctl", {"request": request, "reply": reply})
                    connection.send(json.loads(json.dumps(reply, ensure_ascii=False, default=repr)))

        def wid(name):
            return windows[name].id if name in windows else name

        def dispatch(request):
            kind = request.get("kind", "call")
            if kind == "call":
                params = dict(request.get("params") or {})
                if "window" in params:
                    params["window_id"] = wid(params.pop("window"))
                return rpc.call(request["method"], params, timeout=request.get("timeout", 30))
            if kind == "doc":
                window = windows[request.get("window", "main")]
                return rpc.call("document.command", {"window_id": window.id,
                    "document_id": window.document.id, "command": request["command"]},
                    timeout=request.get("timeout", 30))
            if kind == "rawdoc":  # explicit (possibly stale) identities
                return rpc.call("document.command", request["params"])
            if kind == "ids":
                return {name: {"window_id": w.id, "document_id": w.document.id if w.document else None}
                        for name, w in windows.items()}
            if kind == "open":
                w = app.open_file(HERE / request["page"], title=request.get("title", "UI4ME C-lab 2"),
                                  width=request.get("width", 600), height=request.get("height", 400))
                windows[request["name"]] = w
                return {"window_id": w.id, "document_id": w.document.id}
            if kind == "load":
                w = windows[request.get("window", "main")]
                d = w.load_file(HERE / request["page"])
                return {"document_id": d.id}
            if kind == "picker":
                w = windows[request.get("window", "main")]
                future = rpc.call_async("dialog." + request["dialog"],
                                        dict(request.get("params") or {}, window_id=w.id))
                def done(f):
                    try:
                        log("picker", {"result": f.result()})
                    except Exception as e:
                        log("picker", {"error": str(e)})
                future.add_done_callback(done)
                return {"started": True}
            if kind == "exec":  # lab-only: run code in this backend (app/rpc/windows/log in scope)
                scope = {"app": app, "rpc": rpc, "windows": windows, "log": log, "HERE": HERE,
                         "time": time, "json": json, "os": os, "threading": threading,
                         "state": lab_state, "result": None}
                exec(request["code"], scope)
                return scope.get("result")
            if kind == "python-hold":
                return {"held": True}
            raise ValueError("unknown kind")

        threading.Thread(target=serve, daemon=True, name="clab-ctl").start()
        log("started", {"pid": os.getpid(), "ids": dispatch({"kind": "ids"})})
        app.run()
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit:
        raise
    except BaseException:
        (STATE / "crash.txt").write_text(traceback.format_exc())
        raise
