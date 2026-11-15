"""Locate the installed UI4ME native host and forward its command-line API."""
from __future__ import annotations
import importlib.resources
import os
from pathlib import Path
import shutil
import subprocess
import sys


def host_binary() -> Path:
    """Return the packaged native host. Development installs may use UI4ME_HOST_BINARY."""
    override = os.environ.get("UI4ME_HOST_BINARY")
    if override:
        binary = Path(override).expanduser().resolve()
        if not binary.is_file():
            raise FileNotFoundError(f"UI4ME_HOST_BINARY does not exist: {binary}")
        return binary
    name = "ui4me-host.exe" if os.name == "nt" else "ui4me-host"
    entry = importlib.resources.files("ui4me_core").joinpath("_bin", name)
    binary = Path(os.fspath(entry))
    if binary.is_file():
        return binary
    for folder in os.environ.get("PATH", "").split(os.pathsep):
        found = shutil.which(name, path=folder) if folder else None
        # The ui4me-host console script of this package is on PATH too; never
        # resolve to it (that would run this wrapper again, forever).
        if found and _is_native_host(Path(found)):
            return Path(found)
    raise FileNotFoundError(
        "UI4ME native host is missing. Install the platform wheel or set UI4ME_HOST_BINARY."
        " (A 'ui4me-host' on PATH that is only the Python wrapper script is not used.)"
    )


def _is_native_host(path: Path) -> bool:
    """True for a native executable, False for a script or a pip/uv
    console-script launcher (a small .exe with a zip archive appended)."""
    try:
        with path.open("rb") as file:
            magic = file.read(4)
            if os.name != "nt":
                return magic == b"\x7fELF"
            if magic[:2] != b"MZ":
                return False
            size = file.seek(0, os.SEEK_END)
            file.seek(max(0, size - 65_557))  # The zip end record is in the last 64 KiB + 22 bytes.
            return b"PK\x05\x06" not in file.read()
    except OSError:
        return False


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print("Usage: ui4me-host [run --app APP.toml | doctor | render ... | --version]", file=sys.stderr)
        return 2
    try:
        binary = host_binary()
    except (OSError, FileNotFoundError) as error:
        print(str(error), file=sys.stderr)
        return 1
    if os.name == "posix":
        # Replace this Python wrapper with the native host so signals from a
        # process manager (SIGTERM, SIGHUP) reach the host itself instead of
        # orphaning it, and the host's exit status is returned unchanged.
        sys.stdout.flush()
        sys.stderr.flush()
        os.execv(str(binary), [str(binary), *args])
    return subprocess.call([str(binary), *args])


if __name__ == "__main__":
    raise SystemExit(main())
