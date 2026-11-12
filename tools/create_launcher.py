#!/usr/bin/env python3
"""Register or remove an application launcher for the current desktop user.

Registration points to an installed host and a permanent application manifest.
It does not copy application files or install anything system-wide.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
APP_ID = re.compile(r"[A-Za-z][A-Za-z0-9_-]*(?:\.[A-Za-z][A-Za-z0-9_-]*)+\Z")


def validate_id(value):
    if not isinstance(value, str) or len(value.encode("utf-8")) > 128 or not APP_ID.fullmatch(value):
        raise ValueError("Application ID must contain dotted ASCII components starting with letters")
    return value


def xdg_path(variable, fallback):
    path = Path(os.environ.get(variable) or fallback).expanduser()
    if not path.is_absolute():
        raise ValueError(f"{variable} must be an absolute path")
    return path


def state_directory():
    if os.name == "nt":
        base = Path(os.environ["LOCALAPPDATA"])
    else:
        base = xdg_path("XDG_STATE_HOME", str(Path.home() / ".local/state"))
    return base / "ui4me" / "launchers"


def desktop_value(value):
    return str(value).replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def desktop_exec_arg(value):
    value = str(value).replace("%", "%%")
    # Desktop Entry string escaping is applied after Exec argument quoting.
    for character in ("\\", '"', "$", "`"):
        value = value.replace(character, "\\" + character)
    return '"' + value + '"'


def desktop_entry(app_id, name, host, app, icon=None):
    command = " ".join(desktop_exec_arg(v) for v in (host, "run", "--app", app))
    lines = ["[Desktop Entry]", "Type=Application", "Version=1.0",
             "Name=" + desktop_value(name), "Exec=" + desktop_value(command),
             "TryExec=" + desktop_value(host),
             "Terminal=false", "StartupNotify=true", "StartupWMClass=" + app_id,
             "Categories=Development;", "X-UI4ME-Managed=true", "X-UI4ME-AppId=" + app_id]
    if icon:
        lines.append("Icon=" + desktop_value(icon))
    return "\n".join(lines) + "\n"


def run_windows_helper(payload):
    helper = ROOT / "tools/windows/register_launcher.ps1"
    with tempfile.TemporaryDirectory(prefix="ui4me-launcher-") as folder:
        source = Path(folder) / "request.json"
        source.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
        result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive",
                                 "-ExecutionPolicy", "Bypass", "-File", str(helper),
                                 "-InputJson", str(source)], capture_output=True, text=True,
                                encoding="utf-8", errors="replace", check=False)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "Shortcut registration failed")
    return json.loads(result.stdout.strip().lstrip("\ufeff"))


def launcher_path(app_id):
    if os.name == "nt":
        return Path(run_windows_helper({"action": "location", "app_id": app_id})["path"])
    return xdg_path("XDG_DATA_HOME", str(Path.home() / ".local/share")) / "applications" / (app_id + ".desktop")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_ownership(record_path, destination, app_id):
    if not record_path.is_file():
        raise ValueError(f"Refusing to replace/remove an unregistered launcher: {destination}")
    record = json.loads(record_path.read_text(encoding="utf-8"))
    if (record.get("app_id") != app_id or Path(record.get("launcher", "")).resolve() != destination.resolve()
            or (destination.exists() and record.get("sha256") != digest(destination))):
        raise ValueError("Launcher no longer matches this tool's ownership record; inspect it before modifying it")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    install = commands.add_parser("install")
    install.add_argument("--app", type=Path, required=True)
    install.add_argument("--host", type=Path, help="Installed ui4me-host executable; otherwise search PATH")
    install.add_argument("--icon", type=Path, help="Launcher icon; Windows accepts .ico/.exe, Linux also accepts .png/.svg")
    uninstall = commands.add_parser("uninstall")
    uninstall.add_argument("--app-id", required=True)
    args = parser.parse_args()
    if sys.platform not in ("linux", "win32"):
        parser.error("Per-user launcher registration currently supports Linux and Windows")
    try:
        if args.action == "install":
            app = args.app.expanduser().resolve(strict=True)
            config = tomllib.loads(app.read_text(encoding="utf-8"))
            app_id = validate_id(config["app"]["id"])
            name = config["app"].get("name", app_id)
            if not isinstance(name, str) or not name.strip() or any(c in name for c in "\x00\n\r"):
                raise ValueError("Application name must be a nonempty single line")
            host_arg = str(args.host.expanduser()) if args.host else shutil.which("ui4me-host")
            if not host_arg:
                raise ValueError("Install the host or provide --host /absolute/path/ui4me-host")
            host = Path(host_arg).resolve(strict=True)
            if not host.is_file():
                raise ValueError("The host must be an executable file")
            if os.name != "nt" and (not os.access(host, os.X_OK) or "%" in str(host)):
                raise ValueError("The Linux host must be executable and its path must not contain % (desktop launcher compatibility)")
            icon = args.icon
            if icon is None and config["app"].get("icon"):
                icon = Path(config["app"]["icon"].replace("{app_dir}", str(app.parent)))
                if not icon.is_absolute():
                    icon = app.parent / icon
            if icon:
                icon = icon.expanduser().resolve(strict=True)
                if os.name == "nt" and icon.suffix.lower() not in (".ico", ".exe"):
                    # Native window PNG icons and Windows shortcut icon resources
                    # have different formats. Use the host icon unless overridden.
                    if args.icon:
                        raise ValueError("A Windows shortcut icon must be .ico or .exe")
                    icon = None
        else:
            app_id = validate_id(args.app_id)
        destination = launcher_path(app_id)
        record_path = state_directory() / (app_id + ".json")
        if destination.exists() or args.action == "uninstall":
            load_ownership(record_path, destination, app_id)
        if args.action == "uninstall":
            destination.unlink(missing_ok=True)
            record_path.unlink(missing_ok=True)
            print(json.dumps({"status": "removed", "app_id": app_id, "launcher": str(destination)}))
            return 0
        destination.parent.mkdir(parents=True, exist_ok=True)
        record_path.parent.mkdir(parents=True, exist_ok=True)
        fd, temporary_name = tempfile.mkstemp(prefix=".ui4me-", suffix=destination.suffix, dir=destination.parent)
        os.close(fd)
        temporary = Path(temporary_name)
        try:
            if os.name == "nt":
                run_windows_helper({"action": "install", "app_id": app_id, "path": str(temporary),
                    "host": str(host), "arguments": subprocess.list2cmdline(["run", "--app", str(app)]),
                    "cwd": str(app.parent), "name": name, "icon": str(icon) if icon else str(host)})
            else:
                temporary.write_text(desktop_entry(app_id, name, host, app, icon), encoding="utf-8")
                temporary.chmod(0o644)
            checksum = digest(temporary)
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)
        record = {"schema": 1, "app_id": app_id, "launcher": str(destination), "host": str(host),
                  "manifest": str(app), "sha256": checksum, "installed_utc": datetime.now(timezone.utc).isoformat()}
        fd, temporary_name = tempfile.mkstemp(prefix=".ui4me-", suffix=".json", dir=record_path.parent)
        with os.fdopen(fd, "w", encoding="utf-8") as file:
            json.dump(record, file, ensure_ascii=False, indent=2)
            file.write("\n")
        Path(temporary_name).replace(record_path)
        print(json.dumps({"status": "installed", "app_id": app_id, "launcher": str(destination),
                          "note": "Keep the host, manifest, backend and icon at their registered paths."}, ensure_ascii=False))
        return 0
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f"Launcher registration failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
