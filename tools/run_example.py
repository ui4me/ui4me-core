#!/usr/bin/env python3
"""Launch the reference backend with the exact active Python interpreter."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def product_version():
    """The workspace version, so generated reference manifests never drift from it."""
    import re
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = text.split("[workspace.package]", 1)[1]
    return re.search(r'^version\s*=\s*"([^"]+)"', section, re.M).group(1)


def checkout_environment(base=None):
    """The environment for a host whose backend imports `ui4me_core` from this checkout."""
    env = dict(os.environ if base is None else base)
    env["PYTHONPATH"] = os.pathsep.join(filter(None, [str(ROOT / "python"), env.get("PYTHONPATH")]))
    return env


def locate_host(explicit=None, release=False):
    if explicit:
        path = Path(explicit).expanduser().resolve()
        if not path.is_file():
            raise FileNotFoundError(path)
        return str(path)
    name = "ui4me-host.exe" if os.name == "nt" else "ui4me-host"
    for mode in (("release", "debug") if release else ("debug", "release")):
        candidate = ROOT / "target" / mode / name
        if candidate.is_file():
            return str(candidate)
    found = shutil.which("ui4me-host")
    if found:
        return found
    raise FileNotFoundError("Build first: cargo build --locked -p ui4me-host")


def manifest(backend_args, app_id="org.ui4me.HostReference", backend_path=None, icon_path=None,
             app_name="UI4ME Host Reference", app_version=None, startup_timeout_ms=15000):
    app_version = app_version or product_version()
    backend_path = Path(backend_path) if backend_path else ROOT / "examples/python/demo.py"
    icon_path = icon_path if icon_path is not None else str(ROOT / "crates/ui4me-host/assets/ui4me.png")
    def q(value):
        # Literal UTF-8 also keeps non-BMP path characters valid in TOML; JSON
        # surrogate-pair escapes are not valid TOML Unicode scalar escapes.
        return json.dumps(value, ensure_ascii=False)
    return "\n".join([
        "[app]", f"id = {q(app_id)}", f"name = {q(app_name)}", f"version = {q(app_version)}",
        f"icon = {q(str(icon_path))}",
        "", "[backend]", f"program = {q(sys.executable)}",
        f"args = {q([str(backend_path)] + backend_args)}",
        f"cwd = {q(str(backend_path.parent))}", "", "[host]",
        f"startup_timeout_ms = {startup_timeout_ms}", "shutdown_timeout_ms = 5000",
        "max_windows = 16", 'instance = "single"', "",
    ])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--native-test", action="store_true")
    parser.add_argument("--allow-unsupervised", action="store_true")
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--external-target", help="File/URL for the reference app's explicit F8 desktop-open action")
    parser.add_argument("--timeout", type=float, default=None)
    parser.add_argument("--app-id", default="org.ui4me.HostReference")
    args = parser.parse_args()
    if args.headless and args.native_test:
        parser.error("--headless and --native-test are mutually exclusive")
    backend_args = ["--self-test"] if args.headless else ["--native-test"] if args.native_test else []
    if args.external_target:
        backend_args += ["--external-target", args.external_target]
    try:
        host = locate_host(args.host, args.release)
    except OSError as error:
        parser.exit(2, f"Cannot launch host: {error}\n")
    with tempfile.TemporaryDirectory(prefix="ui4me-reference-") as folder:
        automated = args.headless or args.native_test
        evidence = args.evidence.resolve() if args.evidence else Path(folder) / "backend.json"
        if automated or args.evidence:
            evidence.parent.mkdir(parents=True, exist_ok=True)
            evidence.unlink(missing_ok=True)
            backend_args += ["--evidence", str(evidence)]
        path = Path(folder) / "app.toml"
        path.write_text(manifest(backend_args, args.app_id), encoding="utf-8")
        command = [host, "run", "--app", str(path)]
        if args.headless:
            command.append("--headless")
        if args.allow_unsupervised:
            command.append("--allow-unsupervised")
        env = checkout_environment()
        if args.headless:
            env["UI4ME_TEST_MODE"] = "1"
        print("Launching", " ".join(command), flush=True)
        try:
            result = subprocess.run(command, cwd=ROOT, env=env, timeout=args.timeout, check=False)
        except subprocess.TimeoutExpired:
            print("FAIL: host exceeded the requested test deadline", file=sys.stderr)
            return 124
        if result.returncode or not automated:
            return result.returncode
        # The host's successful exit alone cannot prove that its backend got
        # through the test: require the completed backend report as well.
        try:
            report = json.loads(evidence.read_text(encoding="utf-8"))
            rows = report["results"]
            required = {"authenticated_handshake", "two_windows", "application_ready",
                        "concurrent_requests", "asynchronous_close_requested",
                        "application_quit_requested", "shutdown_received"}
            if args.native_test:
                required.add("native_clipboard_round_trip")
            passed = {row["test"] for row in rows if row["status"] == "PASS"}
            if any(row["status"] == "FAIL" for row in rows) or not required <= passed:
                raise ValueError("Backend evidence is incomplete or contains a failed check")
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"FAIL: cannot confirm completed backend test: {error}", file=sys.stderr)
            return 1
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
