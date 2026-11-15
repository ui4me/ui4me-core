# Evidence

Reproduction scripts and logs retained from the audit. Paths in scripts were normalised:
`<repo>` = a checkout of this repository, `<scratch>` = a temporary working directory, `<home>` = the user's home. Replace them before running.

## Coordinator probes (`scripts/`), run against the published 1.0.1 artifacts

| Script | Finding | Usage |
| --- | --- | --- |
| `burst2.py` | F-02 | `python burst2.py N ROUNDS env` in a desktop session; N=20 fails on 1.0.1 |
| `burst.ps1` | F-02 | `powershell -File burst.ps1 -N 20` (edit the module import path) |
| `depth.py` | F-01 | `python depth.py 40` on Windows aborts the host |
| `svgdepth.py` | F-03 | `python svgdepth.py 700` aborts the host on Linux and Windows |
| `issue2_replay.py` + `doc.html` + `region.html` | F-04 | issue #2 replay (theme import removed); see the issue for variants |
| `churn.py` | E10 | 400 window/document cycles, prints host RSS |
| `job_*.ps1` | — | how the Windows runs were driven inside an interactive RDP session via `tools/windows/start_interactive.ps1` |

Linux windowed runs used a private nested compositor so that no window appeared on a real desktop:
`kwin_wayland --virtual --socket ui4me-audit-0 --width 1280 --height 900 --no-lockscreen`, then `WAYLAND_DISPLAY=ui4me-audit-0 python …`.

## Specialist probes

- `scripts/host/` — raw-socket adversarial backend (`adv_backend.py`, `adv_run.py <scenario>`), signal, secondary-launch and fd-exhaustion tests (HOST).
- `scripts/render-harness/` — in-process renderer harness (`leak`, `issue2`, `settext`, `deep`, `movedeep`, `profile` modes) (RENDER).
- `scripts/interact-probe/` — headless interaction/geometry probe (INTERACT).
- `scripts/bind/` — Python binding experiments `e*.py`, burst probes; `scripts/bind/ps/` PowerShell experiments and the mock peer (BIND).
- `scripts/release/` — container smoke tests for the published wheel and module (RELEASE).

## Logs (`logs/`)

Windows RDP-session outputs (`depth-*.txt`, `psb-*.txt`, `thresh-*.txt`, `win-burst-out.head.txt`), Linux windowed logs (`nested-20.log`, `depth-linux-*.log`, `svg-linux-700.log`, `bold.log`/`none.log`/`hbold.log` for issue #2, `churn.log`) and the verifier report.
