#!/usr/bin/env python3
"""Validate the stabilization tracker and render its Markdown matrix.

The tracker (docs/stabilization/tracker.json) is the machine-readable record of every audit finding,
specialist finding, test and release gate. This tool enforces the evidence rules:

* every consolidated finding F-01..F-29 (30 rows) and every specialist finding is present exactly once;
* statuses come from the fixed vocabulary;
* FIXED_VERIFIED requires at least one commit, at least one named regression test (unless the finding is
  a process/documentation item with explicit evidence), and a recorded Linux result plus a Windows result when
  the finding requires Windows verification;
* ALREADY_FIXED, NOT_REPRODUCED, DEFERRED and BLOCKED require a written justification in ``notes``.

Usage: ``python3 tools/stabilization_tracker.py check`` or ``... render`` (rewrites MATRIX.md).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TRACKER = ROOT / "docs" / "stabilization" / "tracker.json"
MATRIX = ROOT / "docs" / "stabilization" / "MATRIX.md"
STATUSES = ("OPEN", "IN_PROGRESS", "FIXED_VERIFIED", "ALREADY_FIXED", "BLOCKED", "DEFERRED", "NOT_REPRODUCED")
JUSTIFIED = ("ALREADY_FIXED", "NOT_REPRODUCED", "DEFERRED", "BLOCKED")
CONSOLIDATED = [f"F-{n:02d}" for n in range(1, 30) if n not in (5, 16, 17)] + ["F-16a", "F-16b", "F-17", "F-17b"]
PENDING = ("", "pending")


def load(path: Path = TRACKER) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def problems(doc: dict) -> list[str]:
    out: list[str] = []
    ids = [f["id"] for f in doc["findings"]]
    if sorted(ids) != sorted(CONSOLIDATED):
        out.append(f"consolidated set differs: missing {sorted(set(CONSOLIDATED) - set(ids))}, extra {sorted(set(ids) - set(CONSOLIDATED))}")
    if len(ids) != len(set(ids)):
        out.append("duplicate consolidated ids")
    spec = [s["id"] for s in doc["specialist"]]
    if len(spec) != len(set(spec)):
        out.append("duplicate specialist ids")
    known = set(spec)
    for f in doc["findings"]:
        for ref in f["specialist_refs"]:
            if ref not in known:
                out.append(f"{f['id']}: unknown specialist ref {ref}")
    for row in doc["findings"] + doc["specialist"]:
        rid, status = row["id"], row["status"]
        if status not in STATUSES:
            out.append(f"{rid}: bad status {status!r}")
            continue
        if status in JUSTIFIED and not row.get("notes", "").strip():
            out.append(f"{rid}: {status} needs a justification in notes")
        if status == "FIXED_VERIFIED":
            if not row.get("commits"):
                out.append(f"{rid}: FIXED_VERIFIED without a commit")
            tests = row.get("tests") or row.get("tests_done") or []
            if not tests and not row.get("evidence_only"):
                out.append(f"{rid}: FIXED_VERIFIED without named regression tests (or evidence_only justification)")
            ver = row.get("verification", {})
            if ver.get("linux", "") in PENDING:
                out.append(f"{rid}: FIXED_VERIFIED without a Linux verification result")
            if ver.get("required") in ("both", "windows") and ver.get("windows", "") in PENDING:
                out.append(f"{rid}: FIXED_VERIFIED without the required Windows verification result")
    for g in doc["gates"]:
        if not g.get("status"):
            out.append(f"{g['id']}: gate without status")
    return out


def _cell(text: object) -> str:
    return str(text).replace("|", "\\|").replace("\n", " ")


def render(doc: dict) -> str:
    lines = ["# Stabilization implementation matrix", "",
             "Generated from [tracker.json](tracker.json) by `python3 tools/stabilization_tracker.py render`. "
             "Do not edit by hand.", "",
             "## Consolidated findings", "",
             "| ID | Sev. | Kind | WS | Status | Tests | Linux | Windows | Commits | Remaining risk |",
             "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    order = {fid: n for n, fid in enumerate(CONSOLIDATED)}
    for f in sorted(doc["findings"], key=lambda r: order.get(r["id"], 999)):
        ver = f["verification"]
        win = ver.get("windows", "") if ver.get("required") in ("both", "windows") else f"n/a ({ver.get('windows','')})" if ver.get("windows") not in PENDING else "n/a"
        lines.append("| " + " | ".join(_cell(x) for x in (
            f["id"], f["severity"], f["kind"], f["workstream"], f["status"], ", ".join(f.get("tests") or f["tests_required"]),
            ver.get("linux", ""), win, " ".join(c[:9] for c in f["commits"]), f.get("remaining_risk", ""))) + " |")
    lines += ["", "## Specialist findings", "",
              "| ID | Sev. | Consolidated | WS | Status | Commits | Notes |", "| --- | --- | --- | --- | --- | --- | --- |"]
    for s in doc["specialist"]:
        lines.append("| " + " | ".join(_cell(x) for x in (
            s["id"], s["severity"], ", ".join(s["consolidated"]), s["workstream"], s["status"],
            " ".join(c[:9] for c in s["commits"]), s.get("notes", ""))) + " |")
    lines += ["", "## Tests T1–T16", "", "| ID | Status | Location | CI | Notes |", "| --- | --- | --- | --- | --- |"]
    for t in doc["tests"]:
        lines.append("| " + " | ".join(_cell(t[k]) for k in ("id", "status", "location", "ci", "notes")) + " |")
    lines += ["", "## Release gates", "", "| Gate | Requirement | Baseline (1.0.1) | Current | Evidence |", "| --- | --- | --- | --- | --- |"]
    for g in doc["gates"]:
        lines.append("| " + " | ".join(_cell(g[k]) for k in ("id", "title", "baseline", "status", "evidence")) + " |")
    counts: dict[str, int] = {}
    for row in doc["findings"]:
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    lines += ["", "Consolidated status counts: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())), ""]
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    doc = load()
    issues = problems(doc)
    for issue in issues:
        print("tracker:", issue, file=sys.stderr)
    if argv[1:2] == ["render"]:
        MATRIX.write_text(render(doc), encoding="utf-8")
    elif argv[1:2] == ["check"] and MATRIX.exists() and MATRIX.read_text(encoding="utf-8") != render(doc):
        print("tracker: MATRIX.md is stale; run render", file=sys.stderr)
        return 1
    return 1 if issues else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
