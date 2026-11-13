#!/usr/bin/env python3
"""Run the PowerShell examples and a New-UI4MEProject app on the real KDE desktop.

Test tooling only. Each app is started through the bundled host of a staged/installed
UI4ME.Core module (found through PSModulePath, as an installed module would be), driven
with real input, captured, closed natively, and its exit status and log checked.

usage: examples_linux.py --modules DIR_CONTAINING_UI4ME.Core --out DIR
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "docs/verification/core-1.0-linux-final-2026-11-13/lab-scripts"))
import lab  # noqa: E402

CLOSE = """var ws = workspace.windowList(); for (var i=0;i<ws.length;i++){ if (ws[i].caption.indexOf(%s)>=0){ ws[i].closeWindow(); OUT('ok'); } }"""


def run_app(manifest: Path, title: str, out: Path, name: str, env: dict, interact=None):
    host = Path(env["PSMODULEDIR"]) / "UI4ME.Core/runtimes/linux-x64/ui4me_core/_bin/ui4me-host"
    log = out / f"{name}.log"
    t0 = time.time()
    with log.open("w") as f:
        proc = subprocess.Popen([str(host), "run", "--app", str(manifest)], env=env, stdout=f, stderr=subprocess.STDOUT)
        w = None
        for _ in range(60):
            w = lab.window(title)
            if w:
                break
            time.sleep(0.3)
        record = {"name": name, "manifest": str(manifest), "window": w, "window_s": round(time.time() - t0, 2)}
        if w:
            time.sleep(1.2)
            lab.activate(title)
            time.sleep(0.4)
            if interact:
                try:
                    record["interaction"] = interact(w)
                except Exception as error:
                    record["interaction_error"] = repr(error)
                time.sleep(0.8)
            w = lab.window(title) or w
            lab.screenshot(out / f"{name}.png", region=(int(w["x"]), int(w["y"]), int(w["w"]), int(w["h"])))
            lab.kwin_script(CLOSE % json.dumps(title))
        try:
            record["exit"] = proc.wait(30)
        except subprocess.TimeoutExpired:
            proc.kill()
            record["exit"] = "timeout"
    text = log.read_text(errors="replace")
    record["errors"] = [l[:300] for l in text.splitlines() if '"level":"ERROR"' in l or '"stream":"stderr"' in l]
    record["pass"] = bool(record["window"]) and record["exit"] == 0 and not record["errors"] and "interaction_error" not in record
    return record


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--modules", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    a = ap.parse_args()
    out = a.out
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PSModulePath=str(a.modules.resolve()), PSMODULEDIR=str(a.modules.resolve()))
    for secret in ("PSGALLERY_API_KEY", "PYPI_API_KEY"):
        env.pop(secret, None)
    examples = ROOT / "powershell/examples"
    results = []

    def counter(w):
        cx, cy = w["cx"], w["cy"]
        # Button centres in the example's layout (client px at 760x520).
        for _ in range(3):
            lab.click(cx + 178, cy + 417); time.sleep(0.25)
        lab.click(cx + 341, cy + 417); time.sleep(0.8)
        clip = subprocess.run(["wl-paste", "-n"], capture_output=True, text=True, timeout=5).stdout
        if clip != "Count = 3":
            raise AssertionError(f"clipboard after Copy: {clip!r}")
        return {"clipboard": clip}

    results.append(run_app(examples / "counter/app.toml", "UI4ME PowerShell Counter", out, "counter", env, counter))
    results.append(run_app(examples / "live-bitmap/app.toml", "PowerShell Live Pixels", out, "live-bitmap", env,
                           lambda w: time.sleep(3) or {"watched_s": 3}))

    def frameless(w):
        before = lab.window("PowerShell Custom Chrome")
        lab.drag(w["cx"] + 200, w["cy"] + 20, w["cx"] + 320, w["cy"] + 90, steps=20)
        time.sleep(0.6)
        after = lab.window("PowerShell Custom Chrome")
        return {"moved": (before["x"], before["y"]) != (after["x"], after["y"]), "before": before, "after": after}

    results.append(run_app(examples / "frameless/app.toml", "PowerShell Custom Chrome", out, "frameless", env, frameless))

    project = out / "generated app ü"
    subprocess.run(["pwsh", "-NoProfile", "-c", f"Import-Module UI4ME.Core; New-UI4MEProject -Path '{project}' -Name 'Generated Check' -AppId org.example.GeneratedCheck -Force | Out-Null"],
                   env=env, check=True)

    def generated(w):
        cx, cy = w["cx"], w["cy"]
        return {"note": "typed name and clicked Greet", "steps": [type_and_greet(cx, cy, w)]}

    def type_and_greet(cx, cy, w):
        # The input sits left of the Greet button in the generated layout.
        lab.click(cx + w["cw"] * 0.35, cy + 222)
        lab.type_text("Ada")
        lab.click(cx + w["cw"] * 0.80, cy + 222)
        return True

    results.append(run_app(project / "app.toml", "Generated Check", out, "generated", env, generated))
    (out / "examples.json").write_text(json.dumps(results, indent=2, ensure_ascii=False))
    print(json.dumps([{k: r.get(k) for k in ("name", "pass", "exit", "errors", "window_s")} for r in results], indent=2, ensure_ascii=False))
    return 0 if all(r["pass"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
