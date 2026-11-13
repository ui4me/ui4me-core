"""UI4ME Windows lab agent: runs inside ui4me_test's interactive RDP session.

Serves a named pipe (multiprocessing.connection, authkey from agent.key next to
this file). Each request is {"exec": code}; the code runs with the helper module
`w` (Win32 queries, screenshots, clipboard, process info) in scope and returns
the variable `result`. Test tooling only; not part of the product.
"""
import ctypes
import json
import os
import secrets
import sys
import threading
import time
import traceback
from multiprocessing.connection import Listener
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import w  # noqa: E402

ADDRESS = r"\\.\pipe\ui4me-lab-agent"
KEY = HERE / "agent.key"


def main():
    # Per-monitor DPI aware so rectangles are physical pixels (match RDP input).
    try:
        ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
    except Exception:
        pass
    key = secrets.token_hex(16)
    KEY.write_text(key)
    log = open(HERE / "agent.log", "a", buffering=1, encoding="utf-8")
    log.write(f"{time.ctime()} start pid={os.getpid()} session={w.session_id()}\n")
    listener = Listener(ADDRESS, family="AF_PIPE", authkey=key.encode())
    scope = {"w": w, "time": time, "json": json, "os": os, "Path": Path}

    def handle(conn):
        with conn:
            while True:
                try:
                    req = conn.recv()
                except (EOFError, OSError):
                    return
                local = dict(scope)
                local["result"] = None
                try:
                    exec(req["exec"], local)
                    reply = {"ok": True, "result": local.get("result")}
                except BaseException as error:  # report everything to the driver
                    reply = {"ok": False, "error": repr(error), "trace": traceback.format_exc()}
                try:
                    conn.send(json.loads(json.dumps(reply, default=repr)))
                except (OSError, EOFError):
                    return
                if req.get("quit"):
                    os._exit(0)

    while True:
        conn = listener.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
