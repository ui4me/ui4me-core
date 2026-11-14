#!/usr/bin/env python3
"""Run a supplied backend fault fixture and retain observations/process evidence.

The authenticated host reports its real PID/creation stamp; --kill-host targets
that process, which can differ from the Linux systemd bootstrap launcher's PID.
This tool does not convert native GUI observation requirements into automatic PASS.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import uuid

from run_example import ROOT, checkout_environment, locate_host, manifest

sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(ROOT / "examples/python"))
from faults import MODES, process_identity


def events(path, run_id):
    if not path.is_file():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            row = json.loads(line)
            if row.get("run_id") == run_id:
                rows.append(row)
        except ValueError:
            pass  # An append can be in progress while monitoring the fixture.
    return rows


def terminate_owned(identity):
    pid = identity["pid"]
    if pid <= 0 or pid == os.getpid() or process_identity(pid) != identity:
        return False
    if os.name != "nt":
        if hasattr(os, "pidfd_open") and hasattr(signal, "pidfd_send_signal"):
            try:
                process_fd = os.pidfd_open(pid)
            except ProcessLookupError:
                return False
            try:
                if process_identity(pid) != identity:
                    return False
                try:
                    signal.pidfd_send_signal(process_fd, signal.SIGKILL)
                    return True
                except ProcessLookupError:
                    return False
            finally:
                os.close(process_fd)
        # Never fall back to an unchecked PID kill during evidence cleanup.
        return False
    import ctypes
    from ctypes import wintypes
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    k.OpenProcess.restype = wintypes.HANDLE
    k.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
    k.TerminateProcess.restype = wintypes.BOOL
    k.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    k.GetProcessTimes.restype = wintypes.BOOL
    k.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = k.OpenProcess(1 | 0x1000, False, pid)
    if not handle:
        return False
    try:
        times = [wintypes.FILETIME() for _ in range(4)]
        if not k.GetProcessTimes(handle, *(ctypes.byref(value) for value in times)):
            return False
        if str((times[0].dwHighDateTime << 32) | times[0].dwLowDateTime) != identity["creation_stamp"]:
            return False
        return bool(k.TerminateProcess(handle, 125))
    finally:
        k.CloseHandle(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--host")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--allow-unsupervised", action="store_true")
    parser.add_argument("--duration", type=float, default=5,
                        help="Seconds to hold a decision/tree; 0 waits for native emergency quit")
    parser.add_argument("--timeout", type=float, help="Deadline; defaults to 45 seconds in headless mode")
    parser.add_argument("--kill-host", action="store_true", help="Only for managed-tree: kill this host after the tree is ready")
    parser.add_argument("--unread-host-output", action="store_true",
                        help="Only for log-flood: leave host stdout/stderr unread until it exits")
    parser.add_argument("--kill-delay", type=float, default=2)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.duration < 0 or args.kill_delay < 0:
        parser.error("Durations cannot be negative")
    if args.kill_host and args.mode != "managed-tree":
        parser.error("--kill-host is restricted to the managed-tree fixture")
    if args.unread_host_output and args.mode != "log-flood":
        parser.error("--unread-host-output is restricted to the bounded log-flood fixture")
    run_id = uuid.uuid4().hex
    output = (args.output or ROOT / "verification/faults" / (args.mode + "-" + run_id[:8])).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("Choose an empty evidence directory so earlier runs cannot be mistaken for this one")
    evidence = output / "backend-events.jsonl"
    try:
        host = locate_host(args.host)
    except OSError as error:
        parser.exit(2, str(error) + "\n")
    backend_args = ["--mode", args.mode, "--evidence", str(evidence), "--run-id", run_id,
                    "--duration", str(0 if args.kill_host else args.duration)]
    env = checkout_environment()
    if args.headless:
        env["UI4ME_TEST_MODE"] = "1"
    timeout = args.timeout if args.timeout is not None else 45 if args.headless or args.kill_host else None
    report = {"schema": 1, "run_id": run_id, "mode": args.mode, "headless": args.headless,
              "started_utc": datetime.now(timezone.utc).isoformat(), "status": "BLOCKED",
              "host_output_was_unread": args.unread_host_output,
              "native_desktop_qualified": False, "output": str(output)}
    timed_out, intentional_kill, interrupted, ready_at = False, False, False, None
    actual_host, kill_attempted, kill_failed = None, False, False
    with tempfile.TemporaryDirectory(prefix="ui4me-fault-") as folder:
        source = Path(folder) / "app.toml"
        app_id = "org.ui4me.Fault" + args.mode.replace("-", "").title() + ".Run" + run_id
        source.write_text(manifest(backend_args, app_id, ROOT / "examples/python/faults.py"), encoding="utf-8")
        command = [host, "run", "--app", str(source)]
        if args.headless:
            command.append("--headless")
        if args.allow_unsupervised:
            command.append("--allow-unsupervised")
        report["command"] = command
        print("Launching controlled fault:", args.mode, "Evidence:", output, flush=True)
        with (output / "host-backend.log").open("wb") as log:
            process = subprocess.Popen(command, cwd=ROOT, env=env,
                                       stdout=subprocess.PIPE if args.unread_host_output else log,
                                       stderr=subprocess.STDOUT)
            report["launcher_pid"] = process.pid
            started, last_progress = time.monotonic(), 0
            try:
                while process.poll() is None:
                    now = time.monotonic()
                    observed = events(evidence, run_id)
                    seen = {row["event"] for row in observed}
                    if actual_host is None:
                        ready = next((row for row in observed if row["event"] == "ready"), None)
                        if ready:
                            actual_host = ready["host"]
                            report["actual_host"] = actual_host
                            report["containment"] = ready.get("containment")
                    if args.kill_host and "managed_tree_ready" in seen and ready_at is None:
                        ready_at = now
                    if ready_at is not None and not kill_attempted and now - ready_at >= args.kill_delay:
                        kill_attempted = True
                        intentional_kill = bool(actual_host and terminate_owned(actual_host))
                        kill_failed = not intentional_kill
                    if timeout is not None and now - started > timeout:
                        if actual_host:
                            terminate_owned(actual_host)
                        process.kill()
                        timed_out = True
                        break
                    if now - last_progress >= 5:
                        print(f"Launcher PID {process.pid}; host {actual_host['pid'] if actual_host else 'pending'}; elapsed {now-started:.1f}s; events {', '.join(sorted(seen)) or 'pending'}", flush=True)
                        last_progress = now
                    time.sleep(0.1)
            except KeyboardInterrupt:
                interrupted = True
            finally:
                if process.poll() is None:
                    if actual_host:
                        terminate_owned(actual_host)
                    process.kill()
            report["host_exit_code"] = process.wait(timeout=5)
            if args.unread_host_output:
                try:
                    captured, _ = process.communicate(timeout=3)
                    log.write(captured or b"")
                except subprocess.TimeoutExpired as error:
                    log.write(error.output or b"")
                    report["output_pipe_did_not_close"] = True
                    timed_out = True
                    process.stdout.close()
            report["seconds"] = round(time.monotonic() - started, 3)
    rows = events(evidence, run_id)
    seen = {row["event"] for row in rows}
    if actual_host is None:
        ready = next((row for row in rows if row["event"] == "ready"), None)
        if ready:
            actual_host = ready["host"]
            report["actual_host"] = actual_host
            report["containment"] = ready.get("containment")
    host_survived = bool(actual_host and process_identity(actual_host["pid"]) == actual_host)
    if host_survived:
        report["surviving_actual_host_cleanup_attempted"] = terminate_owned(actual_host)
    report.update(events=rows, timed_out=timed_out, interrupted=interrupted,
                  intentional_host_kill=intentional_kill, host_kill_failed=kill_failed,
                  actual_host_survived_launcher=host_survived)
    report["managed_descendants"] = []
    for kind in ("worker", "leaf"):
        path = output / ("tree-" + kind + ".json")
        if not path.is_file():
            continue
        record = json.loads(path.read_text(encoding="utf-8"))
        if record.get("run_id") != run_id:
            continue
        identity = {key: record[key] for key in ("pid", "creation_stamp")}
        deadline = time.monotonic() + 3
        while process_identity(identity["pid"]) == identity and time.monotonic() < deadline:
            time.sleep(0.1)
        alive = process_identity(identity["pid"]) == identity
        result = {"kind": kind, **identity, "alive_after_host_exit": alive}
        if alive:
            result["cleanup_attempted"] = terminate_owned(identity)
        report["managed_descendants"].append(result)
    failed = ("fixture_failed" in seen or timed_out or interrupted or kill_failed or host_survived
              or any(row["alive_after_host_exit"] for row in report["managed_descendants"]))
    if args.mode == "managed-tree" and len(report["managed_descendants"]) != 2:
        failed = True
    if args.kill_host:
        complete = intentional_kill and "managed_tree_ready" in seen
    elif args.mode == "crash-after-ready":
        complete = "intentional_crash" in seen and report["host_exit_code"] != 0
    elif args.duration == 0 and args.mode.startswith("freeze"):
        complete = False
        report["note"] = "Indefinite decision fixture requires a native observation record; automatic qualification remains blocked."
    else:
        complete = "fixture_completed" in seen and report["host_exit_code"] == 0
    report["status"] = "FAIL" if failed else "PASS" if complete else "BLOCKED"
    report["scope"] = "Protocol/fault/process observations only. Native painting, input, failure UI and emergency-quit behavior need separate observation."
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["status"] == "PASS" else 1 if report["status"] == "FAIL" else 2


if __name__ == "__main__":
    raise SystemExit(main())
