#!/usr/bin/env python3
"""Record v0.2 native renderer acceptance separately from the host's N01–N15."""
from native_acceptance import main

CHECKS = {
    "R01": "Native local HTML/CSS/images/fonts/SVG render on the qualified host; no browser or network loading",
    "R02": "Controls and Python callbacks: live DOM batches, queries, replacement, stale identities and event order",
    "R03": "Real IME and cross-application clipboard in input/textarea; correct caret, selection and surrounding text",
    "R04": "Keyboard-only navigation, pointer hit testing/capture, selection, scroll and default actions",
    "R05": "Document accessibility semantics, text/value/selection/actions and an actual screen-reader journey",
    "R06": "Resize/scale and two documents/windows; correct focus, coordinates and resource ownership",
    "R07": "Asset roots, missing/malformed resources, payload/cache limits and cancellation during document replacement",
    "R08": "File input routes to parented cancellable native picker; HTML dialog/popover and form semantics",
    "R09": "Covered/minimized DOM updates, pending callback, backend failure and native emergency quit remain responsive",
    "R10": "Measured idle/startup/update behavior and settled memory after window plus DOM/resource churn",
    "R11": "Installed HTML/CSS/Python app and launcher run outside checkout as normal user on this target",
    "R12": "Pinned browser-reference fixtures reviewed; supported profile differences resolved or explicitly bounded",
}


if __name__ == "__main__":
    raise SystemExit(main(CHECKS, "renderer"))
