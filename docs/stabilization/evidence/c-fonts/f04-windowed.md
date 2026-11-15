# F-04 windowed replay (Linux, private nested KWin)

Command (both hosts at the same time, one compositor; `run.sh` in the workstream scratch dir):

    env -u WAYLAND_DISPLAY -u DISPLAY kwin_wayland --virtual --socket ui4me-wsc-0 --width 1280 --height 900 --no-lockscreen &
    env -u DISPLAY WAYLAND_DISPLAY=ui4me-wsc-0 PYTHONPATH=python UI4ME_HOST_BINARY=<host> \
      LEAK_TAG=<tag> LEAK_CSS=custom LEAK_CSSTEXT='.u4-badge { font-weight: 600; }' \
      LEAK_SECS=900 LEAK_SAMPLE=750 LEAK_DIR=docs/audit/evidence/scripts \
      python3 docs/stabilization/evidence/c-fonts/issue2_replay_long.py

`issue2_replay_long.py` = the audit's `issue2_replay.py` with configurable duration/sample rate.
Hosts: `cargo build --release --locked -p ui4me-host`; baseline = 1e49a6a (plus a test-only shim
that does not change behaviour), fixed = c955e44. Windowed mode (real frames), ~48 ticks/s, region
inserted/removed every 50 ticks. 900 s each (15 min). Host VmRSS, KiB:

| t (s) | tick | baseline | fixed |
| --- | --- | --- | --- |
| 15.5 | 750 | 58164 | 53276 |
| 47.2 | 2250 | 68444 | 53700 |
| 78.9 | 3750 | 78232 | 54020 |
| 110.8 | 5250 | 86724 | 54032 |
| 142.6 | 6750 | 97012 | 54008 |
| 174.6 | 8250 | 106664 | 54088 |
| 206.4 | 9750 | 115872 | 54124 |
| 237.6 | 11250 | 124944 | 54308 |
| 269.4 | 12750 | 134776 | 54580 |
| 300.3 | 14250 | 143840 | 54608 |
| 331.3 | 15750 | 153488 | 54632 |
| 362.5 | 17250 | 162704 | 54648 |
| 393.8 | 18750 | 172176 | 54684 |
| 425.4 | 20250 | 181820 | 54712 |
| 456.7 | 21750 | 191096 | 54792 |
| 488.1 | 23250 | 199956 | 54652 |
| 520.0 | 24750 | 209796 | 55092 |
| 551.2 | 26250 | 219060 | 53336 |
| 582.3 | 27750 | 227372 | 53204 |
| 613.7 | 29250 | 237600 | 51124 |
| 644.7 | 30750 | 247268 | 47712 |
| 676.0 | 32250 | 255372 | 46268 |
| 707.5 | 33750 | 259344 | 45584 |
| 738.9 | 35250 | 262420 | 44332 |
| 770.9 | 36750 | 270984 | 45444 |
| 803.2 | 38250 | 281160 | 45040 |
| 835.4 | 39750 | 289500 | 44732 |
| 866.7 | 41250 | 289472 | 43536 |
| 897.9 | 42750 | 296132 | 43544 |

Baseline: 58.2 -> 296.1 MiB, linear, ~0.26 MiB/s (issue #2 reported 0.3 MiB/s; gate R4 fails).
Fixed: 53.3 MiB at 15 s, peak 55.1 MiB (520 s), 43.5 MiB at 898 s: flat (the late decrease is the
allocator returning memory). Full series: `raw/windowed-baseline.log`, `raw/windowed-fixed.log`.
