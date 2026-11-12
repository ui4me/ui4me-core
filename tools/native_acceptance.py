#!/usr/bin/env python3
"""Create honest native acceptance evidence and run the automated native subset.

Human or computer-use observations can be recorded with --record. A record must
reference an existing evidence file. Unexecuted checks stay BLOCKED.
"""
import argparse
from datetime import datetime, timezone
import getpass
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
CHECKS = {
    "N01": "Cold launch, hosted app identity, icon and taskbar grouping",
    "N02": "Simultaneous second launch forwards to exactly one backend in this desktop session",
    "N03": "Two native windows: ownership, resize, minimize, maximize and close/save/cancel",
    "N04": "Native clipboard exchange with another application; preserve its original content",
    "N05": "Native open/save/folder dialogs: select, cancel, parent-close and quit races",
    "N06": "Real keyboard layout and IME composition, Unicode, caret and surrounding text",
    "N07": "Accessibility tree, focus and at least one action through native accessibility tooling",
    "N08": "Scale/display change and surface recreation; record actual display configuration",
    "N09": "Backend freeze/crash while host-local painting, input and failure UI remain usable",
    "N10": "Normal quit and forced host kill clean managed child and grandchild processes",
    "N11": "An externally opened application survives host exit",
    "N12": "Installed normal-user launch outside the checkout and build directory",
    "N13": "File/text drag-and-drop in the actual native target session",
    "N14": "Session isolation; Windows RDP disconnect/reconnect where applicable",
    "N15": "Startup, idle CPU, host/backend memory and repeated window cycles measured",
}


def interactive_context():
    detail = {"os": platform.system(), "release": platform.release(),
              "platform": platform.platform(), "architecture": platform.machine(),
              "machine": platform.node(),
              "username": getpass.getuser(),
              "desktop": os.environ.get("XDG_CURRENT_DESKTOP"),
              "session_type": os.environ.get("XDG_SESSION_TYPE"),
              "wayland_display": os.environ.get("WAYLAND_DISPLAY"),
              "session_name": os.environ.get("SESSIONNAME")}
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        user = ctypes.WinDLL("user32", use_last_error=True)
        k.ProcessIdToSessionId.argtypes = [wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
        k.ProcessIdToSessionId.restype = wintypes.BOOL
        user.GetProcessWindowStation.restype = wintypes.HANDLE
        user.GetUserObjectInformationW.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                  wintypes.LPVOID, wintypes.DWORD,
                                                  ctypes.POINTER(wintypes.DWORD)]
        user.GetUserObjectInformationW.restype = wintypes.BOOL
        class USEROBJECTFLAGS(ctypes.Structure):
            _fields_ = [("inherit", wintypes.BOOL), ("reserved", wintypes.BOOL), ("flags", wintypes.DWORD)]
        session = wintypes.DWORD()
        k.ProcessIdToSessionId(os.getpid(), ctypes.byref(session))
        info, needed = USEROBJECTFLAGS(), wintypes.DWORD()
        ok = user.GetUserObjectInformationW(user.GetProcessWindowStation(), 1,
                                           ctypes.byref(info), ctypes.sizeof(info), ctypes.byref(needed))
        detail["session_id"] = session.value
        detail["visible_window_station"] = bool(ok and info.flags & 1)
        shell = ctypes.WinDLL("shell32")
        shell.IsUserAnAdmin.restype = wintypes.BOOL
        detail["elevated"] = bool(shell.IsUserAnAdmin())
        return detail["visible_window_station"] and session.value != 0, detail
    display = os.environ.get("WAYLAND_DISPLAY")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    socket_path = Path(display) if display and Path(display).is_absolute() else Path(runtime) / display if display and runtime else None
    try:
        usable_socket = bool(socket_path and stat.S_ISSOCK(socket_path.stat().st_mode))
    except OSError:
        usable_socket = False
    detail["wayland_socket_exists"] = usable_socket
    detail["uid"] = os.geteuid()
    detail["elevated"] = os.geteuid() == 0
    return usable_socket, detail


def source_identity():
    """Stable identity of executable inputs; exclude reports and build outputs."""
    digest = hashlib.sha256()
    excluded = {"target", ".git", "verification", "__pycache__", ".venv", ".local", "dist"}
    paths = []
    for directory, folders, files in os.walk(ROOT):
        folders[:] = [name for name in folders if name not in excluded and not (Path(directory) == ROOT and name == "docs")]
        for name in files:
            path = Path(directory) / name
            if path.suffix not in {".pyc", ".log"}:
                paths.append(path)
    for path in sorted(paths):
        digest.update(path.relative_to(ROOT).as_posix().encode("utf-8") + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def main(checks=None, suite="host"):
    checks = CHECKS if checks is None else checks
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "verification" / ("native" if suite == "host" else "renderer-native"))
    parser.add_argument("--automated", action="store_true")
    parser.add_argument("--record", choices=checks)
    parser.add_argument("--status", choices=("PASS", "FAIL", "BLOCKED"))
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--note", default="")
    parser.add_argument("--host")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    path = args.output / "acceptance.json"
    current_source = source_identity()
    if path.exists():
        report = json.loads(path.read_text(encoding="utf-8"))
        if set(report.get("checks", {})) != set(checks) or report.get("source_sha256") != current_source:
            parser.error("This ledger belongs to another suite/source tree; choose a fresh --output for this integration milestone")
    else:
        report = {"schema": 2, "suite": suite, "source_sha256": current_source, "qualification": "UNVERIFIED",
                  "checks": {key: {"requirement": text, "status": "BLOCKED", "reason": "Not executed"}
                             for key, text in checks.items()}}
    interactive, context = interactive_context()
    environment = {key: context[key] for key in ("os", "release", "architecture", "machine", "username")}
    if report.get("environment") and report["environment"] != environment:
        parser.error("This evidence directory belongs to a different target machine/OS; choose a separate --output")
    report["environment"] = environment
    report["last_context"] = context
    report["updated_utc"] = datetime.now(timezone.utc).isoformat()
    rc = 0
    if args.record:
        if not args.status:
            parser.error("--record requires --status")
        if args.status != "BLOCKED" and (not args.evidence or not args.evidence.is_file()):
            parser.error("A PASS/FAIL observation requires an existing --evidence file")
        if args.status == "PASS" and not interactive:
            parser.error("Native PASS records must be captured inside the interactive target session")
        report["checks"][args.record] = {"requirement": checks[args.record], "status": args.status,
            "evidence": str(args.evidence.resolve()) if args.evidence else None,
            "evidence_sha256": hashlib.sha256(args.evidence.read_bytes()).hexdigest() if args.evidence and args.evidence.is_file() else None,
            "note": args.note, "context": context, "recorded_utc": report["updated_utc"]}
    if args.automated:
        if not interactive:
            report["automated_subset"] = {"status": "BLOCKED", "reason": "No interactive native target session"}
            rc = 2
        else:
            runner = "run_example.py" if suite == "host" else "run_renderer.py"
            app_id = "org.ui4me.HostNativeVerification" if suite == "host" else "org.ui4me.RendererNativeVerification"
            from run_example import locate_host
            try:
                selected_host = Path(locate_host(args.host)).resolve()
                with selected_host.open("rb") as executable:
                    binary_hash = hashlib.file_digest(executable, "sha256").hexdigest()
                report["tested_host"] = {"path": str(selected_host), "sha256": binary_hash,
                                         "size_bytes": selected_host.stat().st_size}
            except OSError as error:
                parser.error(f"Cannot identify the native host executable: {error}")
            command = [sys.executable, str(ROOT / "tools" / runner), "--native-test",
                       "--timeout", "90", "--evidence", str((args.output / "backend.json").resolve()),
                       "--app-id", app_id, "--host", str(selected_host)]
            with (args.output / "automated.log").open("wb") as log:
                completed = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=False)
            report["automated_subset"] = {"status": "PASS" if completed.returncode == 0 else "FAIL",
                "exit_code": completed.returncode, "log": "automated.log",
                "scope": ("Window creation, protocol, clipboard self-round-trip and quit only" if suite == "host"
                          else "Local HTML/CSS documents, DOM commands, callback-to-host RPC, replacement and quit only")
                          + "; no inferred desktop qualification"}
            rc = completed.returncode
    statuses = [row["status"] for row in report["checks"].values()]
    statuses.append(report.get("automated_subset", {}).get("status", "BLOCKED"))
    report["qualification"] = "PASS" if all(s == "PASS" for s in statuses) else "FAIL" if "FAIL" in statuses else "BLOCKED"
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return rc or {"PASS": 0, "FAIL": 1, "BLOCKED": 2}[report["qualification"]]


if __name__ == "__main__":
    raise SystemExit(main())
