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
    """False for a Python console-script launcher, True for anything else
    that can run the host.

    POSIX: any executable (ELF, Mach-O, or a distro/Nix ``#!/bin/sh exec ...``
    wrapper) except a script run by a Python interpreter (including the
    ``#!/bin/sh`` trampoline pip/uv write for long interpreter paths), one
    that runs ``ui4me_core.launcher``, or the script running now (BIND-04).
    Windows: a PE without a zip archive appended (pip/uv launchers are PEs).
    """
    try:
        with path.open("rb") as file:
            if os.name != "nt":
                return not _is_python_script(file.read(_SCRIPT_HEAD), path)
            magic = file.read(4)
            if magic[:2] != b"MZ":
                return False
            size = file.seek(0, os.SEEK_END)
            file.seek(max(0, size - 65_557))  # The zip end record is in the last 64 KiB + 22 bytes.
            return b"PK\x05\x06" not in file.read()
    except OSError:
        return False


_SCRIPT_HEAD = 4096
_PYTHON_NAMES = (b"python", b"pypy")


def _is_python_script(head: bytes, path: Path) -> bool:
    if b"ui4me_core.launcher" in head:
        return True
    try:
        if sys.argv and sys.argv[0] and os.path.samefile(path, sys.argv[0]):
            return True
    except OSError:
        pass
    if not head.startswith(b"#!"):
        return False  # A native executable.
    lines = head.split(b"\n")
    words = lines[0][2:].split()
    if words and os.path.basename(words[0]) == b"env":
        words = [word for word in words[1:] if not word.startswith(b"-") and b"=" not in word]
    if words and os.path.basename(words[0]).startswith(_PYTHON_NAMES):
        return True
    # pip/uv for an interpreter path that is long or contains spaces:
    # "#!/bin/sh", then "'''exec' "/path/to/python" "$0" "$@"".
    return len(lines) > 1 and lines[1].startswith(b"'''exec' ")


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
