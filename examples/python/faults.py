"""Controlled fault fixtures. Launch through tools/run_faults.py.

These behaviors are deliberately unsuitable for production applications. Worker
descendants never create an independent process group or escape the host's job.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import traceback

from ui4me_core import RpcClient

MODES = ("veto-close", "veto-quit", "freeze-close", "freeze-quit",
         "crash-after-ready", "log-flood", "managed-tree")


def process_identity(pid=None):
    """PID plus OS creation stamp, so a reused PID is not mistaken for our child."""
    pid = os.getpid() if pid is None else pid
    if os.name != "nt":
        try:
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            if fields[0] == "Z":
                return None
            return {"pid": pid, "creation_stamp": fields[19]}
        except (OSError, IndexError):
            return None
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.GetProcessTimes.restype = wintypes.BOOL
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.OpenProcess(0x1000 | 0x100000, False, pid)
    if not handle:
        return None
    try:
        if kernel.WaitForSingleObject(handle, 0) != 258:
            return None
        birth, exit_time, system, user = (wintypes.FILETIME() for _ in range(4))
        if not kernel.GetProcessTimes(handle, *(ctypes.byref(v) for v in (birth, exit_time, system, user))):
            return None
        return {"pid": pid, "creation_stamp": str((birth.dwHighDateTime << 32) | birth.dwLowDateTime)}
    finally:
        kernel.CloseHandle(handle)


def tree_process(args):
    kind = "leaf" if args.tree_leaf else "worker"
    record = {"run_id": args.run_id, "kind": kind, **process_identity()}
    if kind == "worker":
        child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                                  "--tree-leaf", "--evidence", str(args.evidence), "--run-id", args.run_id])
        record["child_pid"] = child.pid
    destination = args.evidence.parent / ("tree-" + kind + ".json")
    temporary = destination.with_suffix(".tmp")
    temporary.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    temporary.replace(destination)
    while True:
        time.sleep(60)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=MODES)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--duration", type=float, default=5,
                        help="Callback hold / managed-tree lifetime; 0 holds until the host closes")
    parser.add_argument("--tree-worker", action="store_true")
    parser.add_argument("--tree-leaf", action="store_true")
    args = parser.parse_args()
    args.evidence.parent.mkdir(parents=True, exist_ok=True)
    if args.tree_worker or args.tree_leaf:
        tree_process(args)
        return 0
    if not args.mode:
        parser.error("--mode is required for the backend")
    evidence_lock = threading.Lock()
    def record(event, **detail):
        row = {"time_utc": datetime.now(timezone.utc).isoformat(), "run_id": args.run_id,
               "event": event, "pid": os.getpid(), **detail}
        line = json.dumps(row, ensure_ascii=False)
        with evidence_lock, args.evidence.open("a", encoding="utf-8") as file:
            file.write(line + "\n")
        print(line, flush=True)

    client = None
    try:
        client = RpcClient.from_environment()
        main_id = client.call("window.create", {"title": "UI4ME fault: " + args.mode,
            "width": 880, "height": 500, "visible": True})["window_id"]
        target_id = client.call("window.create", {"title": "UI4ME fault target",
            "width": 500, "height": 340, "visible": True})["window_id"]
        client.call("probe.set_text", {"window_id": main_id, "text":
            "Controlled fault: " + args.mode + "\n"
            "Observe host-local text input, painting, accessibility and window behavior.\n"
            "Ctrl+Shift+Q is the host's explicit emergency quit.\n"
            "A withheld close/quit decision must never silently become permission."})
        client.call("app.ready")
        diagnostics = client.call("host.diagnostics")
        host_identity = process_identity(diagnostics["pid"])
        if host_identity is None:
            raise RuntimeError("Cannot establish the real host's OS process creation identity")
        record("ready", main_window_id=main_id, target_window_id=target_id,
               mode=args.mode, backend=process_identity(), host=host_identity,
               containment=diagnostics.get("containment"))

        if args.mode == "crash-after-ready":
            time.sleep(max(0.1, args.duration))
            record("intentional_crash", exit_code=23)
            os._exit(23)

        if args.mode in ("veto-close", "veto-quit", "freeze-close", "freeze-quit"):
            is_close = args.mode.endswith("close")
            freeze = args.mode.startswith("freeze")
            received, release, returned = threading.Event(), threading.Event(), threading.Event()
            first_decision = threading.Lock()
            handled = False
            def decide(params):
                nonlocal handled
                with first_decision:
                    first, handled = not handled, True
                if not first:
                    return {"allow": True}
                record("callback_blocked" if freeze else "callback_veto", request=params)
                received.set()
                if freeze:
                    release.wait()
                returned.set()
                return {"allow": False}
            client.on_request("window.before_close" if is_close else "app.before_quit", decide)
            method = "window.request_close" if is_close else "app.request_quit"
            params = {"window_id": target_id} if is_close else {}
            client.call(method, params)
            if not received.wait(5):
                raise TimeoutError("Host never sent the close/quit decision request")
            count = 0
            deadline = time.monotonic() + max(0.3, args.duration)
            while time.monotonic() < deadline:
                client.call("host.capabilities", timeout=3)
                count += 1
                time.sleep(0.1)
            record("host_control_responsive", calls=count, decision_held=freeze)
            if freeze and args.duration == 0:
                record("awaiting_native_emergency_quit")
                while not client.closed and not client.shutdown_requested.wait(0.2):
                    pass
                return 0
            release.set()
            if not returned.wait(3):
                raise TimeoutError("Backend callback did not return its veto")
            time.sleep(0.2)
            client.call("window.set", {"window_id": target_id, "title": "Still alive after explicit veto"})
            record("window_alive_after_veto")

        elif args.mode == "log-flood":
            chunk = ("controlled backend log — 日本語\n" * 100).encode("utf-8")
            write_errors = []
            def flood(fd):
                try:
                    for _ in range(max(1, (4 * 1024 * 1024) // len(chunk))):
                        remaining = memoryview(chunk)
                        while remaining:
                            remaining = remaining[os.write(fd, remaining):]
                except Exception as error:
                    write_errors.append(str(error))
            writers = [threading.Thread(target=flood, args=(fd,), daemon=True) for fd in (1, 2)]
            for writer in writers:
                writer.start()
            for _ in range(24):
                client.call("host.capabilities", timeout=5)
            for writer in writers:
                writer.join(15)
            if write_errors or any(writer.is_alive() for writer in writers):
                raise RuntimeError("Log flood blocked or failed: " + repr(write_errors))
            record("bounded_log_flood_completed", bytes_per_stream_approximately=4 * 1024 * 1024,
                   concurrent_control_calls=24)

        elif args.mode == "managed-tree":
            child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                                      "--tree-worker", "--evidence", str(args.evidence), "--run-id", args.run_id])
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if all((args.evidence.parent / f"tree-{kind}.json").is_file() for kind in ("worker", "leaf")):
                    break
                time.sleep(0.05)
            else:
                raise TimeoutError("Managed child/grandchild did not publish their process identities")
            record("managed_tree_ready", child_pid=child.pid)
            if args.duration == 0:
                while not client.closed and not client.shutdown_requested.wait(0.2):
                    pass
                record("fixture_completed")
                return 0
            time.sleep(args.duration)

        client.call("app.request_quit")
        deadline = time.monotonic() + 10
        while not client.closed and not client.shutdown_requested.is_set() and time.monotonic() < deadline:
            time.sleep(0.05)
        if not client.closed and not client.shutdown_requested.is_set():
            raise TimeoutError("Host did not begin shutdown after the accepted quit")
        record("fixture_completed")
        return 0
    except Exception as error:
        record("fixture_failed", error=f"{type(error).__name__}: {error}")
        traceback.print_exc()
        return 1
    finally:
        if client:
            try:
                client.flush()
            finally:
                client.close()


if __name__ == "__main__":
    raise SystemExit(main())
