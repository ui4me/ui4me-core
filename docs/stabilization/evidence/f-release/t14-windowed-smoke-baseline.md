# T14 windowed smoke: first runs against the baseline (2026-11-15, local Linux, nested compositors)

Nothing ran on the real desktop: each run used a private compositor on its own socket.

## weston --backend=headless, debug host built from this branch (baseline host code)

Command: `python3 tools/windowed_smoke.py` → FAIL

| probe | result | seconds | first frame logged | reason / detail |
|---|---|---|---|---|
| first-frame | PASS | 1.83 | True | text-ok seconds=1.00 |
| depth | FAIL | 2.45 | True | exit status 1; no RESULT PASS line — `thread 'main' (170981) has overflowed its stack` |
| svgdepth | PASS | 3.46 | True | depth=700 seconds=2.50 |
| burst | PASS | 4.01 | True | controls=100 rounds=4 mode=env events=3072 seconds=2.79 |

## kwin_wayland --virtual --socket ui4me-wsf-0, same debug host

Command: `python3 tools/windowed_smoke.py --compositor kwin --socket ui4me-wsf-0` → FAIL

| probe | result | seconds | first frame logged | reason / detail |
|---|---|---|---|---|
| first-frame | PASS | 2.01 | True | text-ok seconds=1.00 |
| depth | FAIL | 2.58 | True | exit status 1; no RESULT PASS line — `thread 'main' (177162) has overflowed its stack` |
| svgdepth | PASS | 3.49 | True | depth=700 seconds=2.50 |
| burst | PASS | 5.07 | True | controls=100 rounds=6 mode=env events=5120 seconds=3.83 |

## weston headless, the published 1.0.1 release host (sha256 be08dd88…)

Command: `python3 tools/windowed_smoke.py --host <1.0.1 release host>` → FAIL

| probe | result | seconds | first frame logged | reason / detail |
|---|---|---|---|---|
| first-frame | PASS | 1.82 | True | text-ok seconds=1.00 |
| depth | PASS | 3.51 | True | depth=250 rect={'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': |
| svgdepth | FAIL | 2.31 | True | exit status 1; no RESULT PASS line — `thread 'ui4me-assets-0' (209403) has overflowed its stack` |
| burst | FAIL | 90.21 | True | timed out after 90s; exit status 124; no RESULT PASS line — `ui4me-host error: application failed (reason=Backend connection lost: closed by host)` |

Reading: with the 1.0.1 release host the smoke reproduces F-03 (svgdepth 700: stack overflow on an assets worker) and F-02 (burst 100: "Backend connection lost: closed by host"). The debug host also aborts at depth 250 on the main thread (F-01 on Linux debug builds); with the debug host the burst does not trip F-02 locally (slower event production), so the nightly job should use a release host. These are baseline results that workstreams A/B/C fix; this branch changes no runtime code.
