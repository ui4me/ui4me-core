#!/usr/bin/env python3
"""Install the host executable from this source checkout using its lockfile."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
from run_example import manifest

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--root", type=Path, help="Cargo installation root (optional)")
parser.add_argument("--debug", action="store_true", help="Install an unoptimized developer build")
parser.add_argument("--jobs", type=int, default=2, help="Conservative Cargo build concurrency")
parser.add_argument("--example-dir", type=Path,
                    help="Also copy a standalone reference application into this empty permanent directory")
args = parser.parse_args()
if args.jobs < 1:
    parser.error("--jobs must be positive")
example = args.example_dir.expanduser().resolve() if args.example_dir else None
if example and example.exists() and (not example.is_dir() or any(example.iterdir())):
    parser.error("--example-dir must name a new or empty directory; existing application files are never overwritten")
command = ["cargo", "install", "--locked", "--jobs", str(args.jobs), "--path", str(root / "crates/ui4me-host")]
if args.root:
    command += ["--root", str(args.root.resolve())]
if args.debug:
    command.append("--debug")
result = subprocess.run(command, cwd=root, check=False)
if result.returncode == 0 and example:
    example.mkdir(parents=True, exist_ok=True)
    for name in ("client.py", "demo.py"):
        shutil.copy2(root / "examples/python" / name, example / name)
    shutil.copy2(root / "crates/ui4me-host/assets/ui4me.png", example / "ui4me.png")
    app = example / "app.toml"
    app.write_text(manifest([], backend_path=example / "demo.py", icon_path="{app_dir}/ui4me.png"), encoding="utf-8")
    print(json.dumps({"reference_app": str(app), "launch": ["ui4me-host", "run", "--app", str(app)],
                      "note": "The manifest pins this Python interpreter. Keep it installed at the registered location."}, indent=2))
raise SystemExit(result.returncode)
