#!/usr/bin/env python3
"""Record Core 1.0 completion scenarios alongside existing host/renderer gates."""
from native_acceptance import main

CHECKS = {
    "C01": "Window size constraints enforce and clear; observed states distinguish unknown from false; attention acknowledged honestly",
    "C02": "System color/motion/contrast changes update documents; explicit app overrides win and return-to-system works",
    "C03": "Anonymous node mutations, atomic rollback, document/node retirement and callback generations remain safe",
    "C04": "UTF-16 selection/replacement plus undo/redo and real IME compose; no split emoji/grapheme, read-only and cancelled edits unchanged",
    "C05": "Reset/submit/custom validity/ordered duplicate form entries, submitter identity and visible selected-file labels work",
    "C06": "Opt-in file/text drops choose nearest eligible target; none/disabled/inert reject; async target replacement does not retarget",
    "C07": "Native context/aux/double-click and mouse-boundary/edit/selection events agree with declared cancellation rules",
    "C08": "Native accessibility table/tree relationships, busy/atomic live updates and assistive ScrollIntoView; Linux app identity/combobox value",
    "C09": "Resource bundles replace atomically, reject escapes/over-budget, invalidate displayed assets and purge only unused decoded entries",
    "C10": "Intrinsic/balanced flex, break-word, logical resize grip, inset radii, font-face defaults and bitmap glyphs render within documented profile",
    "C11": "Parent-aware open and reveal use native services; truthful unavailable/cancel outcomes; external application survives UI4ME exit",
    "C12": "Installed reference is static while idle; no unexpected redraw loop, sustained input stall, or unexplained resource growth under agreed workloads",
}

if __name__ == "__main__":
    raise SystemExit(main(CHECKS, "completion"))
