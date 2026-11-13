# UI4ME Core 1.0.0 — Windows final qualification evidence (2026-11-13)

Windows Server 2022 Standard Evaluation 10.0.20348 (VM `omegaui`, x64), normal user `ui4me_test`
(not an administrator) in its interactive RDP session (session 2, xfreerdp3 client, 1600x1000 at
96 DPI; 144 DPI and 1280x800 exercised by reconnecting). Input was real RDP keyboard/mouse input
(xdotool on the client's X display); observation came from an in-session agent (Win32, native
IUIAutomation via comtypes, Win32 clipboard, psutil/QueryProcessCycleTime) reached over a
private named pipe. Microsoft Pinyin (zh-CN) was added for the IME checks and removed afterwards;
NVDA 2026.3.1 portable gave the screen-reader journey.

| Identity | Value |
| --- | --- |
| Ledger source identity | `a089b4f5338f49417f56e95a7bef7f5cb6c8f3db7c0afba938628fac7d271d5d` (same tree on Linux) |
| Installed host | `%USERPROFILE%\.cargo\bin\ui4me-host.exe` 1.0.0, SHA-256 `bc704f662345611188b650cc9db1fa696c4934943ccc153100a74c0ba2836f6d` |
| Artifacts | [release/verify-windows-artifacts.json](release/verify-windows-artifacts.json), [STATUS](../../STATUS.md) |

## Index

* **Ledgers** (each `qualification: PASS`, 39/39, recorded inside the interactive session, bound to
  the source identity and host hash, automated subsets PASS):
  [host N01–N15](ledgers/native-acceptance.json), [renderer R01–R12](ledgers/renderer-acceptance.json),
  [completion C01–C12](ledgers/completion-acceptance.json).
* **Per-check evidence** [evidence/](evidence/) (`<ID>.json`: actions, expected, observed,
  limitations, build identity and SHA-256 of each referenced transcript, screenshot and log).
* **Transcripts** of the final-binary runs [transcripts/](transcripts/):
  `probe_suite` (N03 N04 N05 N07 N13), `ime_suite` (N06), `ref_suite` (N01 N12 R01–R06 R08 R11),
  `n02`, `nvda_suite` (R05), `scale_suite` (N08 N14), `group2` (R07 R09 N09 N10 N11),
  `c_suite` (C01–C05 C07–C09 C12), `c_suite2` (C06 C11), `c10`, `rc2_suite` and `rc2_perf`
  (frameless chrome, 8 resize directions, live bitmaps, bundles, fonts, veto, teardown, costs),
  `final_misc` (N15/R10 measurements, R12), `measure-1200`, `uia_final` + `uia_chrome`
  (TextPattern matrix), `consumer_start`, `wheel-matrix`.
* **TextPattern investigation** [uia/](uia/): per-provider reports (UI4ME, WordPad, Chrome) for
  the managed and the native COM client in MTA and STA, and the managed client's
  `AccessViolationException` logs.
* **Automated gates** [automated/](automated/): verify report (7/7 PASS), Rust results
  (1,007 passed, 0 failed, 9 ignored), Python tests, Clippy.
* **Measurements** [measure/](measure/) (raw `measure_resources.py` reports).
* **Release checks** [release/](release/): Windows artifact validation, Linux twine/validator/
  auditwheel/install matrix, `cargo deny` advisories.
* **Linux regression on the final source** [linux-regression/](linux-regression/).
* **Lab tooling** [lab-scripts/](lab-scripts/): the in-session agent (`agent.py`, `w.py`, `uia.py`,
  `relay.py`), the Linux-side drivers (`wl.py`, `cw.py`, `rdpctl.py`, `startmenu.py`), every suite,
  the C-lab app (`wlab/`, a Windows port of the Linux lab with a named-pipe control channel), the
  WinForms OLE drag source (`DragSrc.cs`), and the ledger recorder (`rec_win.py`,
  `record_all.py`). Credentials are never stored; the RDP password is read from a local file
  outside the repository.

Screenshots and full host logs stay in the ignored `verification/core-1.0-windows/` archive on
the qualification workstation; the evidence JSON files record their SHA-256.

Notes: harness corrections made during exploratory runs (Start-Menu item lookup, Explorer item
coordinates, the Pinyin candidate window covering a click target, an RDP lab app still closing
when the next suite started) are visible in the lab scripts; the final transcripts were produced
in one pass on the final binary.
