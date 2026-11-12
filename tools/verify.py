#!/usr/bin/env python3
"""Run focused checks and retain their exact commands, exit codes and logs."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
STAGES = ("fmt", "check", "test", "python", "headless", "renderer", "faults", "native", "renderer-native", "completion-native", "browser")


def stop_process_tree(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        try:
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=10, check=False)
        except (OSError, subprocess.TimeoutExpired):
            pass
        if process.poll() is None:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", action="append", choices=STAGES)
    parser.add_argument("--output", type=Path, default=ROOT / "verification" / "latest")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--keep-going", action="store_true")
    parser.add_argument("--timeout", type=int, default=1200)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    stages = args.stage or ["fmt", "check", "test", "python", "headless", "renderer", "faults"]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = str(args.jobs)
    env["RUST_TEST_THREADS"] = str(args.jobs)
    fault_run = output / "faults" / uuid.uuid4().hex[:10]
    commands = {
        # `fmt --all` also traverses local patch crates' upstream workspaces;
        # those intentionally contain no upstream developer-only generators.
        # Format our five workspace packages, never rewrite vendored sources.
        "fmt": [["cargo", "fmt", "-p", "ui4me-host", "-p", "ui4me-renderer",
                 "-p", "ui4me-document", "-p", "ui4me-assets", "-p", "ui4me-engine-types",
                 "--", "--check"]],
        "check": [["cargo", "check", "--locked", "--workspace", "--all-targets"]],
        "test": [["cargo", "test", "--locked", "--workspace"]],
        "python": [[sys.executable, "-m", "unittest", "discover", "-s", "tools", "-p", "test_*.py", "-v"]],
        "headless": [["cargo", "build", "--locked", "-p", "ui4me-host"],
                     [sys.executable, "tools/run_example.py", "--headless", "--timeout", "40",
                      "--app-id", "org.ui4me.HostHeadlessVerification",
                      "--evidence", str(output / "backend-headless.json")]],
        "native": [[sys.executable, "tools/native_acceptance.py", "--automated",
                    "--output", str(output / "native")]],
        "renderer": [["cargo", "build", "--locked", "-p", "ui4me-host"],
                     [sys.executable, "tools/run_renderer.py", "--headless", "--timeout", "90",
                      "--app-id", "org.ui4me.RendererHeadlessVerification",
                      "--evidence", str(output / "renderer-headless.json")]],
        "renderer-native": [[sys.executable, "tools/renderer_acceptance.py", "--automated",
                             "--output", str(output / "renderer-native")]],
        "completion-native": [[sys.executable, "tools/completion_acceptance.py", "--automated",
                              "--output", str(output / "completion-native")]],
        "browser": [[sys.executable, "tools/browser_reference.py", "--output", str(output / "browser" / uuid.uuid4().hex[:10])]],
    }
    commands["faults"] = [["cargo", "build", "--locked", "-p", "ui4me-host"]] + [
        [sys.executable, "tools/run_faults.py", mode, "--headless", "--duration", "1",
         "--timeout", "45", "--output", str(fault_run / mode)]
        + (["--unread-host-output"] if mode == "log-flood" else [])
        for mode in ("veto-close", "veto-quit", "freeze-close", "freeze-quit",
                     "crash-after-ready", "log-flood", "managed-tree")]
    if os.name == "nt":
        commands["faults"].append([sys.executable, "tools/run_faults.py", "managed-tree",
            "--headless", "--kill-host", "--timeout", "30", "--output", str(fault_run / "forced-host-kill")])
    rows = []
    for stage in stages:
        print(f"\n=== {stage} ===", flush=True)
        started = time.monotonic()
        row = {"stage": stage, "status": "PASS", "commands": []}
        for index, command in enumerate(commands[stage]):
            process = None
            log = output / f"{stage}-{index}.log"
            print(" ".join(command), flush=True)
            try:
                with log.open("wb") as log_file:
                    process = subprocess.Popen(command, cwd=ROOT, env=env,
                                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                               start_new_session=os.name != "nt")
                    # Reader thread is unnecessary here: stream line output while
                    # a watchdog enforces a deadline without hiding CLI progress.
                    import threading
                    expired = threading.Event()
                    def expire(watched=process, alarm=expired):
                        alarm.set()
                        stop_process_tree(watched)
                    timer = threading.Timer(args.timeout, expire)
                    timer.start()
                    try:
                        for line in iter(process.stdout.readline, b""):
                            log_file.write(line)
                            sys.stdout.buffer.write(line)
                            sys.stdout.buffer.flush()
                        rc = process.wait()
                    finally:
                        timer.cancel()
                    if expired.is_set():
                        rc = 124
                row["commands"].append({"argv": command, "exit_code": rc, "log": str(log.relative_to(output))})
                if rc:
                    row["status"] = "BLOCKED" if rc == 2 and stage in ("native", "renderer-native", "completion-native", "browser") else "FAIL"
                    break
            except (OSError, KeyboardInterrupt) as error:
                if process is not None and process.poll() is None:
                    stop_process_tree(process)
                row["status"] = "BLOCKED" if isinstance(error, FileNotFoundError) else "INTERRUPTED"
                row["error"] = str(error)
                break
        row["seconds"] = round(time.monotonic() - started, 3)
        rows.append(row)
        report = {"schema": 1, "time_utc": datetime.now(timezone.utc).isoformat(),
                  "platform": platform.platform(), "machine": platform.machine(),
                  "python": sys.version, "stages": rows,
                  "native_desktop_qualified": False,
                  "note": "Review native acceptance evidence separately; build/headless success is not desktop qualification."}
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"{stage}: {row['status']}", flush=True)
        if row["status"] != "PASS" and not args.keep_going:
            break
    return 0 if len(rows) == len(stages) and all(r["status"] == "PASS" for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
