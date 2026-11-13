#!/usr/bin/env python3
"""Run each UIA operation in a fresh process with an external hard deadline.

Run in ui4me_test's active RDP session, targeting the installed reference app.
Repeat against a native editor by providing its process ID and control ID/name;
--first-edit chooses its first Edit control. This tool does not
terminate the target application, alter its text, or change the machine setup.
The select mode changes its selection and --focus changes foreground focus.

--client chooses the managed System.Windows.Automation client (UiaTextProbe.cs)
and/or the native IUIAutomation COM client (uia_com_probe.py, needs comtypes).
Finding recorded for Core 1.0 (Windows Server 2022, .NET Framework 4.8): the
managed client's TextPatternRange.GetText on a range returned by GetSelection
raises AccessViolationException in UiaCoreApi.RawTextRange_GetText for every
provider tested (UI4ME/AccessKit, WordPad RichEdit, Chrome), in MTA and STA;
the native COM client completes every operation for all three. Treat a managed
selection-read failure as a client defect unless the COM client fails too.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import time

MODES = ("pattern", "document-text", "selection", "selection-text", "select", "select-read")


def stop_probe(process, stream):
    """Bound cleanup even when taskkill itself fails; never kill the target app."""
    errors = []
    try:
        result = subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdout=stream, stderr=subprocess.STDOUT, timeout=10, check=False)
        if result.returncode:
            errors.append(f"taskkill exited {result.returncode}")
    except (OSError, subprocess.TimeoutExpired) as error:
        errors.append(f"taskkill: {error}")
    if process.poll() is None:
        try:
            process.kill()
        except OSError as error:
            errors.append(f"kill: {error}")
    try:
        process.wait(timeout=10)
    except (OSError, subprocess.TimeoutExpired) as error:
        errors.append(f"reap: {error}")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, default=0)
    parser.add_argument("--title", default=None)
    selector = parser.add_mutually_exclusive_group()
    selector.add_argument("--automation-id", default=None)
    selector.add_argument("--control-name", default=None)
    selector.add_argument("--first-edit", action="store_true")
    parser.add_argument("--apartment", action="append", choices=("MTA", "STA"))
    parser.add_argument("--mode", action="append", choices=MODES)
    parser.add_argument("--client", action="append", choices=("managed", "com"),
                        help="managed System.Windows.Automation (default) and/or native IUIAutomation via comtypes")
    parser.add_argument("--focus", action="store_true")
    parser.add_argument("--timeout", type=int, default=15)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.exit(2, "BLOCKED: this diagnostic requires an interactive Windows desktop\n")
    if not 1 <= args.timeout <= 300:
        parser.error("--timeout must be 1..300")
    if args.title is None:
        args.title = "" if args.pid > 0 else "Desktop workspace"
    if args.pid < 0 or (args.pid == 0 and not args.title.strip()):
        parser.error("provide a positive --pid or a distinctive --title")
    if args.control_name is not None and not args.control_name.strip():
        parser.error("--control-name must not be empty; use --first-edit")
    if args.automation_id is None and args.control_name is None and not args.first_edit:
        args.automation_id = "name"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    clients = args.client or ["managed"]
    if "com" in clients and (args.pid <= 0 or args.control_name is not None):
        parser.error("--client com needs --pid and --automation-id or --first-edit")
    rows = []
    for client, apartment, mode in [(c, a, m) for c in clients
                                    for a in args.apartment or ["MTA", "STA"]
                                    for m in args.mode or MODES]:
        name = ("" if client == "managed" else "com-") + apartment.lower() + "-" + mode
        log = output / (name + ".log")
        log.unlink(missing_ok=True)
        command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
            "-" + apartment, "-File", str(Path(__file__).with_name("uia_text_probe.ps1")),
            "-TargetProcessId", str(args.pid),
            "-Mode", mode, "-Apartment", apartment, "-LogPath", str(log),
            "-TimeoutSeconds", str(args.timeout)]
        # Windows PowerShell 5.1 can drop empty -File arguments. Omit them
        # and use a switch for the intentionally unqualified Edit selector.
        if args.title.strip():
            command += ["-WindowTitle", args.title]
        if args.control_name is not None:
            command += ["-ControlName", args.control_name]
        elif args.first_edit or args.automation_id == "":
            command.append("-FirstEdit")
        elif args.automation_id is not None:
            command += ["-AutomationId", args.automation_id]
        if args.focus:
            command.append("-Focus")
        if client == "com":
            command = [sys.executable, str(Path(__file__).with_name("uia_com_probe.py")),
                       "--pid", str(args.pid), "--mode", mode, "--apartment", apartment,
                       "--log", str(log)]
            if args.first_edit or args.automation_id == "":
                pass  # first Edit control type
            elif args.automation_id is not None:
                command += ["--automation-id", args.automation_id]
        started = time.monotonic()
        errors = []
        with (output / (name + "-process.log")).open("wb") as stream:
            try:
                process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT)
            except OSError as error:
                errors.append(f"launch: {error}")
                rc = 125
            else:
                try:
                    rc = process.wait(timeout=args.timeout + 30)  # bounded compile/start overhead
                except subprocess.TimeoutExpired:
                    errors.extend(stop_probe(process, stream))
                    rc = 124
        text = log.read_text(encoding="utf-8", errors="replace") if log.exists() else ""
        status = "PASS" if rc == 0 and text.rstrip().endswith("PASS") else "TIMEOUT" if rc == 124 else "FAIL"
        row = {"client": client, "mode": mode, "apartment": apartment, "status": status, "exit_code": rc,
               "seconds": round(time.monotonic() - started, 3), "log": log.name,
               "last_marker": text.splitlines()[-1] if text else None, "argv": command,
               "errors": errors}
        rows.append(row)
        print(name, status, row["last_marker"], flush=True)
        (output / "report.json").write_text(json.dumps({"schema": 1,
            "time_utc": datetime.now(timezone.utc).isoformat(), "pid": args.pid,
            "title": args.title, "focus_requested": args.focus,
            "selection_note": "select changes target selection; prepare/reset the target before each independent reproduction",
            "native_desktop_qualified": False, "results": rows}, indent=2) + "\n", encoding="utf-8")
    return 0 if rows and all(row["status"] == "PASS" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
