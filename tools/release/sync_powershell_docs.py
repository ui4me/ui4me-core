"""Copy the Python API and PowerShell command references into the self-contained agent skills.

The skills (bundled in the wheel and the PowerShell Gallery package) must not
depend on repository paths, so relative documentation links become absolute
GitHub links. Both skills therefore carry the same full reference.
usage: python tools/release/sync_powershell_docs.py [--check]
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = "https://github.com/ui4me/ui4me-core/blob/main/"
PAIRS = [(ROOT / "docs/reference/powershell-api.md", ROOT / "skills/ui4me-powershell/references/powershell-api.md"),
         (ROOT / "docs/reference/python-api.md", ROOT / "skills/ui4me-python/references/python-api.md")]


def standalone(source: Path) -> str:
    text = source.read_text(encoding="utf-8")
    folder = source.parent.relative_to(ROOT)

    def absolute(match: re.Match) -> str:
        target = match.group(2)
        if re.match(r"^[a-z]+:|^#", target):
            return match.group(0)
        resolved = (ROOT / folder / target.split("#")[0]).resolve().relative_to(ROOT).as_posix()
        anchor = "#" + target.split("#", 1)[1] if "#" in target else ""
        return f"{match.group(1)}({BASE}{resolved}{anchor})"

    return re.sub(r"(\[[^\]]*\])\(([^)\s]+)\)", absolute, text)


def main() -> int:
    check = "--check" in sys.argv
    stale = []
    for source, target in PAIRS:
        expected = standalone(source)
        if target.exists() and target.read_text(encoding="utf-8") == expected:
            continue
        if check:
            stale.append(str(target.relative_to(ROOT)))
        else:
            target.write_text(expected, encoding="utf-8", newline="\n")
    if stale:
        print("Out of date (run tools/release/sync_powershell_docs.py):", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
