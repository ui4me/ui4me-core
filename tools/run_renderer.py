#!/usr/bin/env python3
"""Launch the HTML/CSS application with this Python interpreter and verify its report."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from run_example import ROOT, locate_host, manifest

sys.path.insert(0, str(ROOT / "examples/python"))
from renderer_demo import REQUIRED_CHECKS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--native-test", action="store_true")
    parser.add_argument("--allow-unsupervised", action="store_true")
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--timeout", type=float)
    parser.add_argument("--app-id", default="org.ui4me.RendererReference")
    parser.add_argument("--hold-callback", action="store_true")
    parser.add_argument("--crash-after-ready", type=float)
    parser.add_argument("--updates", type=float)
    args = parser.parse_args()
    if args.headless and args.native_test:
        parser.error("--headless and --native-test are mutually exclusive")
    automated = args.headless or args.native_test
    if automated and (args.hold_callback or args.crash_after_ready is not None or args.updates is not None):
        parser.error("Fault observation flags are for interactive runs")
    if args.timeout is not None and args.timeout <= 0:
        parser.error("--timeout must be positive")
    try:
        host = locate_host(args.host, args.release)
    except OSError as error:
        parser.exit(2, f"Cannot launch renderer: {error}\n")
    with tempfile.TemporaryDirectory(prefix="ui4me-renderer-") as temporary:
        directory = Path(temporary)
        evidence = args.evidence.resolve() if args.evidence else directory / "backend.json"
        backend_args = ["--headless-verify"] if args.headless else ["--native-verify"] if args.native_test else []
        if args.hold_callback:
            backend_args.append("--hold-callback")
        if args.crash_after_ready is not None:
            backend_args += ["--crash-after-ready", str(args.crash_after_ready)]
        if args.updates is not None:
            backend_args += ["--updates", str(args.updates)]
        if automated or args.evidence:
            evidence.parent.mkdir(parents=True, exist_ok=True)
            evidence.unlink(missing_ok=True)
            backend_args += ["--evidence", str(evidence)]
        path = directory / "app.toml"
        path.write_text(manifest(backend_args, app_id=args.app_id,
            backend_path=ROOT / "examples/python/renderer_demo.py",
            app_name="UI4ME Desktop Workspace", startup_timeout_ms=30000), encoding="utf-8")
        command = [host, "run", "--app", str(path)]
        if args.headless:
            command.append("--headless")
        if args.allow_unsupervised:
            command.append("--allow-unsupervised")
        env = os.environ.copy()
        if args.headless:
            env["UI4ME_TEST_MODE"] = "1"
        print("Launching", " ".join(command), flush=True)
        try:
            result = subprocess.run(command, cwd=ROOT, env=env,
                                    timeout=args.timeout or (90 if automated else None), check=False)
        except subprocess.TimeoutExpired:
            print("FAIL: renderer application exceeded the deadline", file=sys.stderr)
            return 124
        if result.returncode or not automated:
            return result.returncode
        try:
            report = json.loads(evidence.read_text(encoding="utf-8"))
            rows = report["results"]
            required = REQUIRED_CHECKS | ({"native_renderer_smoke"} if args.native_test else set())
            passed = {row["test"] for row in rows if row["status"] == "PASS"}
            if any(row["status"] != "PASS" for row in rows) or not required <= passed:
                raise ValueError("Backend evidence is incomplete or contains a failed check")
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"FAIL: cannot confirm renderer verification: {error}", file=sys.stderr)
            return 1
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
