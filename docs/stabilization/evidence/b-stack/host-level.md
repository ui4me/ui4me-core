# Host-level runs (Linux, release host, private nested kwin_wayland --virtual, socket ui4me-wsb-0)

Scripts: docs/audit/evidence/scripts/depth.py, svgdepth.py; depth_kind.py = depth.py with the tag as
argv[2]; clipdepth.py = svgdepth.py with N chained clip-path instead of nested <g>. Windowed (native
Wayland event loop, now on the ui4me-engine thread). 'stack 1024' = `ulimit -s 1024` plus
UI4ME_ALLOW_UNSUPERVISED=1 (otherwise systemd-run would start the host with the manager's limits):
it reproduces the 1 MiB Windows main-thread reserve on Linux.

Baseline binary: release build of the tree before the host/box-tree/asset changes.

| run | baseline | fixed |
|---|---|---|
| depth.py 40, default stack | pass | pass |
| depth.py 250, default stack | pass | pass |
| depth_kind.py 250 fieldset, default stack | **abort** (thread 'main' has overflowed its stack) | pass |
| depth_kind.py 252 fieldset, default stack | **abort** | (covered by 250 + T1 at 256) |
| svgdepth.py 700, default stack | **abort** (thread 'ui4me-assets-0' has overflowed its stack) | pass (image refused, DECODE_FAILED) |
| clipdepth.py 2000, default stack | **abort** (thread 'ui4me-assets-1' has overflowed its stack) | pass (image refused) |
| depth.py 30, stack 1024 | pass | - |
| depth.py 40, stack 1024 | **abort** (thread 'main') | pass |
| depth_kind.py 30 fieldset, stack 1024 | **abort** | - |
| depth.py 250, stack 1024 | - | pass |
| depth_kind.py 250 fieldset, stack 1024 | - | pass |
| depth_kind.py 250 table, stack 1024 | - | pass |
| svgdepth.py 700, stack 1024 | - | pass |

Fixed-run output:

```
== fixed default depth.py 40
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': 8.0}}
DEPTH_PASS 40
exit=0
== fixed default depth.py 250
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': 8.0}}
DEPTH_PASS 250
exit=0
== fixed default depth_kind.py 250 fieldset
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 4008.0, 'y': 1906.4375}}
DEPTH_PASS 250
exit=0
== fixed default svgdepth.py 700
TEXT start
SVG_PASS 700
exit=0
== fixed default clipdepth.py 2000
TEXT start
CLIP_PASS 2000
exit=0
== fixed 1024 depth.py 40
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': 8.0}}
DEPTH_PASS 40
exit=0
== fixed 1024 depth.py 250
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': 8.0}}
DEPTH_PASS 250
exit=0
== fixed 1024 depth_kind.py 250 fieldset
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 4008.0, 'y': 1906.4375}}
DEPTH_PASS 250
exit=0
== fixed 1024 depth_kind.py 250 table
TEXT deep RECT {'rect': {'height': 22.0, 'width': 38.0, 'x': 8.0, 'y': 8.0}}
DEPTH_PASS 250
exit=0
== fixed 1024 svgdepth.py 700
TEXT start
SVG_PASS 700
exit=0
```
