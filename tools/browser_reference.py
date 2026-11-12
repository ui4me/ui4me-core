#!/usr/bin/env python3
"""Compare explicit supported HTML/CSS fixtures with an external Chromium oracle.

This optional development tool needs Playwright Python, a local Chromium install
and Pillow. None is used or shipped by the application runtime. A passing report
qualifies only these fixtures, at their declared viewport and tolerance.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import uuid

from native_acceptance import source_identity
from run_example import ROOT, locate_host

FIXTURES = ROOT / "tools/browser-fixtures"


def compare_images(reference: Path, actual: Path, difference: Path, *,
                   channel_threshold: int = 16) -> dict:
    """Return image metrics without changing images or tolerances to hide errors."""
    from PIL import Image, ImageChops, ImageStat
    with Image.open(reference) as reference_image, Image.open(actual) as actual_image:
        a, b = reference_image.convert("RGB"), actual_image.convert("RGB")
        if a.size != b.size:
            return {"same_size": False, "reference_size": list(a.size), "actual_size": list(b.size)}
        delta = ImageChops.difference(a, b)
        delta.save(difference)
        maximum = ImageChops.lighter(ImageChops.lighter(*delta.split()[:2]), delta.split()[2])
        histogram = maximum.histogram()
        pixels = a.width * a.height
        return {"same_size": True, "width": a.width, "height": a.height,
                "mean_absolute_channel_error": sum(ImageStat.Stat(delta).mean) / 3.0,
                "channel_threshold": channel_threshold,
                "fraction_pixels_over_threshold": sum(histogram[channel_threshold + 1:]) / pixels,
                "maximum_channel_error": max(index for index, count in enumerate(histogram) if count)}


def within_tolerance(metrics: dict, tolerance: dict) -> bool:
    return (metrics["same_size"]
            and metrics["mean_absolute_channel_error"] <= tolerance["mean_absolute_channel_error"]
            and metrics["fraction_pixels_over_threshold"] <= tolerance["fraction_pixels_over_threshold"])


def sha256(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--chromium", type=Path, help="Existing Chromium executable; otherwise use Playwright's local browser")
    parser.add_argument("--output", type=Path,
                        default=ROOT / "verification/browser-reference" / uuid.uuid4().hex[:10])
    parser.add_argument("--fixture", action="append", help="Run named cases only (all cases are required for R12)")
    parser.add_argument("--timeout", type=float, default=60)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("--output must be new or empty so evidence from different runs cannot mix")
    output.mkdir(parents=True, exist_ok=True)
    report = {"schema": 1, "suite": "browser-reference", "status": "BLOCKED",
              "time_utc": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
              "source_sha256": source_identity(), "native_desktop_qualified": False,
              "scope": "Only the named fixtures at the declared viewport, scale and tolerance; not full browser conformance",
              "results": []}

    def save():
        (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    try:
        from playwright.sync_api import sync_playwright
        import PIL
        host = str(Path(locate_host(args.host, args.release)).resolve())
        cases = json.loads((FIXTURES / "cases.json").read_text(encoding="utf-8"))["cases"]
        if args.fixture:
            requested = set(args.fixture)
            unknown = requested - {case["id"] for case in cases}
            if unknown:
                parser.error("Unknown fixtures: " + ", ".join(sorted(unknown)))
            cases = [case for case in cases if case["id"] in requested]
        report.update({"host": host, "host_sha256": sha256(Path(host)),
                       "playwright_version": importlib.metadata.version("playwright"),
                       "pillow_version": PIL.__version__, "complete_suite": not bool(args.fixture)})
        source = output / "source"
        shutil.copytree(FIXTURES, source)
        shutil.copytree(ROOT / "examples/python/ui/fonts", source / "fonts")
        shutil.copy2(ROOT / "examples/python/ui/mark.svg", source / "mark.svg")
        report["fixture_sources"] = {path.relative_to(source).as_posix(): sha256(path)
                                     for path in sorted(source.rglob("*")) if path.is_file()}
        with sync_playwright() as playwright:
            # The browser is only an external QA oracle. Do not launch a web
            # server, import page code into the host, or permit remote resources.
            launch_options = {"headless": True}
            if args.chromium:
                launch_options["executable_path"] = str(args.chromium.resolve())
            browser = playwright.chromium.launch(**launch_options)
            report["browser_version"] = browser.version
            try:
                for case in cases:
                    directory = output / case["id"]
                    directory.mkdir()
                    actual, expected, difference = (directory / name for name in ("ui4me.png", "chromium.png", "difference.png"))
                    page_path = source / case["html"]
                    command = [host, "render", "--html", str(page_path), "--output", str(actual),
                               "--width", str(case["width"]), "--height", str(case["height"]), "--scale", "1"]
                    row = {"fixture": case["id"], "scope": case["scope"], "command": command,
                           "tolerance": case["tolerance"], "status": "FAIL"}
                    report["results"].append(row)
                    try:
                        result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                                timeout=args.timeout, check=False)
                        (directory / "render.log").write_bytes(result.stdout)
                        row["renderer_exit_code"] = result.returncode
                        if result.returncode or not actual.is_file():
                            raise RuntimeError("UI4ME render command failed or did not produce a PNG; see render.log")
                        context = browser.new_context(viewport={"width": case["width"], "height": case["height"]},
                                                      device_scale_factor=1, color_scheme="light", reduced_motion="reduce",
                                                      locale="en-US")
                        try:
                            context.route("http://**/*", lambda route: route.abort())
                            context.route("https://**/*", lambda route: route.abort())
                            page = context.new_page()
                            page.goto(page_path.as_uri(), wait_until="load", timeout=args.timeout * 1000)
                            page.evaluate("() => document.fonts.ready")
                            # Only the external browser oracle evaluates this
                            # diagnostic script. Fixture/application files have no JS.
                            row["browser_fonts"] = page.evaluate("() => Array.from(document.fonts).map(f => ({family:f.family,status:f.status}))")
                            if any(font["status"] != "loaded" for font in row["browser_fonts"]):
                                raise RuntimeError("Browser fixture font did not load")
                            page.screenshot(path=str(expected), animations="disabled")
                        finally:
                            context.close()
                        metrics = compare_images(expected, actual, difference,
                                                 channel_threshold=case["tolerance"]["channel_threshold"])
                        row["metrics"] = metrics
                        row["status"] = "PASS" if within_tolerance(metrics, case["tolerance"]) else "FAIL"
                        row["files"] = {name: str(path.relative_to(output)) for name, path in
                                        (("renderer", actual), ("browser", expected), ("difference", difference)) if path.exists()}
                    except Exception as error:
                        row["error"] = f"{type(error).__name__}: {error}"
                    print(f"{case['id']}: {row['status']}", flush=True)
                    save()
            finally:
                browser.close()
        report["status"] = "PASS" if report["results"] and all(row["status"] == "PASS" for row in report["results"]) else "FAIL"
    except (ImportError, FileNotFoundError, importlib.metadata.PackageNotFoundError) as error:
        report["reason"] = f"Missing optional QA dependency: {error}"
    except Exception as error:
        # Failure to launch the external browser is an unmet oracle prerequisite.
        # Once fixture execution starts, a comparison/tool error is a failure.
        report["status"] = "FAIL" if report["results"] else "BLOCKED"
        report["reason"] = f"{type(error).__name__}: {error}"
    finally:
        save()
    print(f"Browser reference: {report['status']}; {output / 'report.json'}")
    return {"PASS": 0, "FAIL": 1, "BLOCKED": 2}[report["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
