#!/usr/bin/env python3
"""Type/lint check the actual Windows Rust sources from a non-Windows host.

This deliberately omits build.rs/resource compilation and cannot qualify a
Windows executable. Native Windows verification must use the real workspace.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target/windows-source-check")
    parser.add_argument("--output", type=Path, default=ROOT / "verification/windows-source-check.txt")
    args = parser.parse_args()
    if os.name == "nt":
        parser.error("On Windows, run the actual workspace's tools/verify.py and native acceptance.")
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    crate = ROOT / "crates/ui4me-host"
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]
    source = (crate / "Cargo.toml").read_text()
    source = re.sub(r'^build\s*=\s*".*"\s*$', "", source, flags=re.M)
    for key in ("version", "edition", "rust-version", "license"):
        source = re.sub(r"(?m)^" + re.escape(key) + r"\.workspace\s*=\s*true\s*$",
                        lambda _: key + " = " + json.dumps(workspace[key]), source)
    source = re.sub(r"(?ms)^\[lib\]\n.*?(?=^\[|\Z)",
                    lambda _: '[lib]\nname = "ui4me_host"\npath = '
                    + json.dumps(str(crate / "src/lib.rs")) + "\n\n", source)
    source = source.replace('path = "src/main.rs"',
                            "path = " + json.dumps(str(crate / "src/main.rs")))
    for folder, kind in (("tests", "test"), ("examples", "example")):
        for path in sorted((crate / folder).glob("*.rs")):
            source += f"\n[[{kind}]]\nname = {json.dumps(path.stem)}\npath = {json.dumps(str(path))}\n"
    # Explicit workspace prevents any accidental parent workspace inheritance.
    source += "\n[workspace]\n"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ui4me-windows-source-") as temporary:
        directory = Path(temporary)
        (directory / "Cargo.toml").write_text(source)
        shutil.copy2(ROOT / "Cargo.lock", directory / "Cargo.lock")
        command = ["cargo", "clippy", "--locked", "--target", "x86_64-pc-windows-msvc",
                   "--all-targets", "-j", str(args.jobs)]
        if args.offline:
            command.append("--offline")
        command += ["--", "-D", "warnings"]
        env = os.environ.copy()
        env["RUSTUP_TOOLCHAIN"] = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        env["CARGO_TARGET_DIR"] = str(args.target_dir.resolve())
        result = subprocess.run(command, cwd=directory, env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    notice = (
        "Windows Rust source/type/lint check only.\n"
        "Actual repository lib, executable, unit/integration-test and example sources; "
        "same dependency features and lockfile.\n"
        "NOT VERIFIED: build.rs, Windows resources, executable linking, native execution, "
        "RDP, named-pipe security or Job Object runtime behavior.\n\n"
    )
    args.output.write_text(notice + "$ " + " ".join(command) + "\n\n" + result.stdout
                           + f"\nExit code: {result.returncode}\nResult: "
                           + ("PASS" if result.returncode == 0 else "FAIL") + "\n")
    print(result.stdout, end="")
    print(notice + "Report: " + str(args.output))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
