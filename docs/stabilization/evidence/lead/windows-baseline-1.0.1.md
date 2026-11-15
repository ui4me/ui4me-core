# Windows baseline re-run on the published 1.0.1 wheel (2026-11-15)

Environment: winDEV (Windows Server 2022, VMID 104 on pvez, started from stopped), interactive RDP session of the
local test user `ui4me_audit`, Python 3.13.18 venv with the published `ui4me_core-1.0.1-py3-none-win_amd64.whl`
(the audit's venv, used read-only). Jobs run with `tools/windows/start_interactive.ps1 -Hidden`; probes are the
audit scripts `depth.py`, `svgdepth.py`, `burst2.py`. A host release build was compiling on the VM concurrently.

| Probe | Result |
| --- | --- |
| `depth.py 30` | `DEPTH_PASS 30` |
| `depth.py 40` | host abort: `thread 'main' (556) has overflowed its stack` (F-01) |
| `depth.py 250` | host abort: `thread 'main' (5960) has overflowed its stack` (F-01) |
| `svgdepth.py 500` | `SVG_PASS 500` |
| `svgdepth.py 700` | host abort: `thread 'ui4me-assets-0' (6116) has overflowed its stack` (F-03) |
| `burst2.py 10 6 env` | pass, 1,080 transition events |
| `burst2.py 20 6 env` | pass this run (1,488 events); the audit saw N=20 fail (E3). The threshold is timing dependent. |
| `burst2.py 40 6 env` | `sending on a full channel` → `application failed (reason=Backend connection lost: closed by host)`, followed by hundreds of `connection is closed` warnings (F-02, HOST-08); app window left open until the 90 s job timeout |
| `burst2.py 100 6 env` | same failure (F-02) |
