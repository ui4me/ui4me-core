"""RC2 lab backend: frameless window, event log and a local control socket.

The control socket (`$XDG_RUNTIME_DIR/ui4me-rc2-lab.sock`) accepts one JSON
line {"exec": "<python>"} and replies with JSON {"ok":..., "result":...}. The
code runs in this backend with `app`, `window`, `doc`, `state` and `log` in
scope; the variable `result` is returned.
"""
from __future__ import annotations
import json, os, socket, threading, time, traceback
from pathlib import Path
from application import Application

HERE = Path(__file__).resolve().parent
LOG = Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / "ui4me-rc2-lab-events.jsonl"
SOCK = Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / "ui4me-rc2-lab.sock"
state = {"veto": False, "close_requests": 0}


def log(kind, payload):
    with LOG.open("a") as f:
        f.write(json.dumps({"t": time.time(), "kind": kind, **payload}) + "\n")


def serve(env):
    if SOCK.exists():
        SOCK.unlink()
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(str(SOCK)); os.chmod(SOCK, 0o600); srv.listen(2)
    while True:
        conn, _ = srv.accept()
        with conn, conn.makefile("rw") as f:
            for line in f:
                req = json.loads(line)
                scope = dict(env); scope["result"] = None
                try:
                    exec(req["exec"], scope)
                    reply = {"ok": True, "result": scope.get("result")}
                except Exception as error:
                    reply = {"ok": False, "error": repr(error), "trace": traceback.format_exc()}
                f.write(json.dumps(reply, default=repr) + "\n"); f.flush()


def main():
    LOG.write_text("")
    with Application.from_environment() as app:
        window = app.open_file(HERE / "ui" / "lab.html", title="UI4ME RC2 lab", width=900,
                               height=640, decorations=False, min_width=320, min_height=240)
        doc = window.document
        env = {"app": app, "window": window, "doc": doc, "state": state, "log": log, "HERE": HERE,
               "time": time, "json": json}

        def wire(d):
            for ev in ("pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick",
                       "auxclick", "contextmenu"):
                d.on(ev, lambda e, ev=ev: log("event", {"type": ev, "e": e}))
            d.on("click", lambda _: window.set(minimized=True), element="min")
            d.on("click", lambda _: window.set(maximized=not bool((window.state().get("observed") or {}).get("maximized"))), element="max")
            d.on("click", lambda _: window.request_close(), element="cls")
        env["wire"] = wire
        wire(doc)

        def before_close():
            state["close_requests"] += 1
            log("before_close", {"veto": state["veto"]})
            return not state["veto"]
        window.on_before_close(before_close)
        threading.Thread(target=serve, args=(env,), daemon=True).start()
        log("ready", {"pid": os.getpid()})
        app.run()
    log("exit", {})


if __name__ == "__main__":
    main()
