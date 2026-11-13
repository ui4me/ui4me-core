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
    fallback = shutil.which(name)
    if fallback:
        return Path(fallback)
    raise FileNotFoundError(
        "UI4ME native host is missing. Install the platform wheel or set UI4ME_HOST_BINARY."
    )


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
    return subprocess.call([str(binary), *args])


if __name__ == "__main__":
    raise SystemExit(main())
