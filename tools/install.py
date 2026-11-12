#!/usr/bin/env python3
"""Install the locked host and an optional standalone HTML/CSS reference app."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
from run_example import ROOT, manifest


def copy_reference(destination: Path, *, probe: bool = False) -> Path:
    """Copy only runtime example files; never overwrite an existing application."""
    if destination.exists() and (not destination.is_dir() or any(destination.iterdir())):
        raise ValueError("The reference destination must be new or empty")
    destination.mkdir(parents=True, exist_ok=True)
    names = ("client.py", "demo.py") if probe else ("client.py", "application.py", "renderer_demo.py")
    for name in names:
        shutil.copy2(ROOT / "examples/python" / name, destination / name)
    if not probe:
        shutil.copytree(ROOT / "examples/python/ui", destination / "ui",
                        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    shutil.copy2(ROOT / "crates/ui4me-host/assets/ui4me.png", destination / "ui4me.png")
    app = destination / "app.toml"
    app.write_text(manifest([], app_id="org.ui4me.HostReference" if probe else "org.ui4me.RendererReference",
        backend_path=destination / ("demo.py" if probe else "renderer_demo.py"),
        icon_path="{app_dir}/ui4me.png",
        app_name="UI4ME Host Reference" if probe else "UI4ME Desktop Workspace",
        startup_timeout_ms=15000 if probe else 30000), encoding="utf-8")
    return app


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, help="Cargo installation root (optional)")
    parser.add_argument("--debug", action="store_true", help="Install an unoptimized developer build")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--example-dir", type=Path,
                        help="Copy a standalone reference app into this new or empty permanent directory")
    parser.add_argument("--probe", action="store_true", help="Copy the v0.1 host probe app instead of the renderer app")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    example = args.example_dir.expanduser().resolve() if args.example_dir else None
    if example and example.exists() and (not example.is_dir() or any(example.iterdir())):
        parser.error("--example-dir must name a new or empty directory; existing application files are never overwritten")
    command = ["cargo", "install", "--locked", "--jobs", str(args.jobs), "--path", str(ROOT / "crates/ui4me-host")]
    if args.root:
        command += ["--root", str(args.root.resolve())]
    if args.debug:
        command.append("--debug")
    result = subprocess.run(command, cwd=ROOT, check=False)
    if result.returncode == 0 and example:
        app = copy_reference(example, probe=args.probe)
        print(json.dumps({"reference_app": str(app), "launch": ["ui4me-host", "run", "--app", str(app)],
            "note": "The manifest pins this Python interpreter. Keep it installed at the registered location."}, indent=2))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
