# UI4ME Core 1.0.0 — Linux final qualification evidence (2026-11-13)

Debian 13.7 / KDE Plasma 6.3.6 / KWin native Wayland, normal user, physical HDMI monitor
(2048x1080, scale 1; 1.5 and 2 exercised). Scripted input came from kernel uinput devices
(`lab-scripts/inputd.py`), window geometry from KWin scripting, semantics from AT-SPI, speech
from Orca 48.1, IME from fcitx5 (pinyin; us-intl base layout), all configured temporarily.

| Identity | Value |
| --- | --- |
| Ledger source identity | `02bf81e422eb060d53abd5e9235fb85f5a23d1ed806d696294e77056ab939aac` |
| Installed host | `~/.cargo/bin/ui4me-host` 1.0.0, SHA-256 `c70346ba412e8348cb6cbbd9fd322128df9bec12accff10d4b26c26fd6dd3146` |
| Artifacts | see [release/artifact-sha256.txt](release/artifact-sha256.txt) |

## Index

* **Ledgers** (each `qualification: PASS`, bound to the source identity and host hash):
  [host N01–N15](ledgers/native-acceptance.json), [renderer R01–R12](ledgers/renderer-acceptance.json),
  [completion C01–C12](ledgers/completion-acceptance.json).
* **Per-check evidence** (`evidence/<ID>.json`: actions, expected, observed, limitations, build
  identity and SHA-256 of every referenced file) and the raw run transcripts:
  [ref-suite.txt](evidence/ref-suite.txt) (N01 N02 N08 N12 R01–R06 R08 R11),
  [group2.txt](evidence/group2.txt) (R07 R09 N09 N10 N11 N14),
  [probe-suite.txt](evidence/probe-suite.txt) (N03–N07; N13 rerun in
  [probe-suite-N13-rerun.txt](evidence/probe-suite-N13-rerun.txt)),
  [c-suite.txt](evidence/c-suite.txt) and [c-suite2.txt](evidence/c-suite2.txt) (C01–C12),
  [R05-orca-speech.txt](evidence/R05-orca-speech.txt).
* **RC2 features on the final binary**: [RC2-livebitmap-demo.txt](evidence/RC2-livebitmap-demo.txt)
  (installed demo: title bar, buttons, clean exit), [RC2-lab-suite-1.txt](evidence/RC2-lab-suite-1.txt)
  (8 resize directions, buttons, ≥1,000 frames x 3 sizes, newest frame, error/budget semantics,
  overload, teardown), [RC2-lab-suite-2.txt](evidence/RC2-lab-suite-2.txt) (1,000 bundle PNG
  replacements, intrinsic sizes, same-URL fonts, veto, scale, resize/animation/bitmap cost).
* **Automated gates**: [verify-report.json](automated/verify-report.json),
  [rust-test-results.txt](automated/rust-test-results.txt) (1,093 passed, 0 failed, 11 ignored),
  [python-tests.txt](automated/python-tests.txt), [clippy.txt](automated/clippy.txt).
* **Measurements**: [measure/summary.txt](measure/summary.txt) and the raw tool reports,
  including the AT-SPI-off attribution run.
* **Release checks**: [cargo deny advisories](release/cargo-deny-advisories.txt),
  [ELF/license review](release/elf-and-licenses.txt), [build_linux tail](release/build_linux-tail.txt),
  [cibuildwheel summary](release/cibuildwheel-summary.txt).
* **Clean installs**: [install-matrix.txt](wheel-install/install-matrix.txt) (CPython 3.10/3.12/3.13),
  [doctor.txt](wheel-install/doctor.txt), [consumer-report.json](wheel-install/consumer-report.json)
  (independent venv consumer), [verify-manylinux-artifacts.json](wheel-install/verify-manylinux-artifacts.json),
  [twine-check.txt](wheel-install/twine-check.txt), [auditwheel-show.txt](wheel-install/auditwheel-show.txt).
* **C10 browser comparison**: [c10/](c10/) (fixture, Playwright script, geometry from both engines).
* **Lab tooling** used for the native runs: [lab-scripts/](lab-scripts/) (RC2 lab app, suites,
  uinput daemon, AT-SPI helpers, GTK drag source, evidence recorder).

Full-resolution screenshots and complete host logs are kept in the ignored
`verification/core-1.0-linux-final/` tree on the qualification workstation; the evidence JSON
files record their SHA-256. A first qualification pass on source `1240f8e4…` found defects
F9 and F18 (see [STATUS](../../STATUS.md)); its evidence is superseded and not used here.

Notes recorded with the checks: the first N13 drag went to a stale duplicate Dolphin window
(clean rerun recorded); policy- and callback-dependent C03/C04/C07 steps were run on a fresh
lab document because a document replacement drops the lab's startup registrations; a first
font-swap sample in the RC2 suite was taken before the font finished loading (re-measured).
