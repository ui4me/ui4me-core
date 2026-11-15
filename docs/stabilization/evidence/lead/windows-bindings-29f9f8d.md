# Windows verification of workstream E (bindings) at 29f9f8d / 73e99d6

winDEV (Windows Server 2022), interactive RDP session of the test user `ui4me_audit` (jobs started with
`tools/windows/start_interactive.ps1 -Hidden`), host `target\release\ui4me-host.exe` built at cd4a12e (E changed
no Rust). Python from uv (3.13, 3.10); Windows PowerShell 5.1.20348; PowerShell 7.4.6 (portable zip in the
workspace, because the only pwsh on the VM is an Administrator-only Store alias). Files: [`windows-bindings-29f9f8d/`](windows-bindings-29f9f8d/).

| Check | Command / probe | Result |
| --- | --- | --- |
| Pester, real host, Windows PowerShell 5.1 | `tools/release/run_pester.ps1 -HostBinary …` ([log](windows-bindings-29f9f8d/pester-ps51.txt)) | **70 passed, 0 failed, 0 skipped**, 2 not run (PosixOnly) |
| Pester, real host, PowerShell 7.4.6 | same ([log](windows-bindings-29f9f8d/pester-pwsh7.txt)) | **70 passed, 0 failed, 0 skipped**, 2 not run (PosixOnly) |
| Pester, real host, Linux pwsh 7.7 | same | 72 passed |
| Python binding suites, 3.13 and 3.10 | `python -m unittest tools.test_binding_robustness tools.test_application tools.test_reference_client tools.test_bootstrap tools.test_launcher` | 100 tests OK on both (17/18 POSIX-only skips) |
| T16 / F-29 PowerShell burst, windowed, N=100 buttons × 6 colour-scheme rounds (10,800 transition events) | [`burst3.ps1`](windows-bindings-29f9f8d/burst3.ps1), 5.1 and 7, with and without a 3 ms callback ([out](windows-bindings-29f9f8d/burst.txt)) | PROBE_PASS ×4; droppedControl=0, droppedDiscrete=0; coalescible transition events dropped as designed (≈8,500–10,000) and counted. 1.0.1 baseline at N=40/100: disconnect + log flood ([baseline](windows-baseline-1.0.1.md)) |
| BIND-04 host lookup | `launcher._is_native_host` ([out](windows-bindings-29f9f8d/bind04.txt)) | real host True; pip/uv shims `ui4me-host.exe`, `ui4me.exe`, `ui4me-new.exe` False |
| BIND-10 / F-25 console Ctrl+Break, direct start | [`ctrlbreak_win.py`](windows-bindings-29f9f8d/ctrlbreak_win.py): own process group, `CTRL_BREAK_EVENT` after READY ([out](windows-bindings-29f9f8d/ctrlbreak.txt)) | Python: exit 130, `on_shutdown` ran, `run()` returned. PowerShell 5.1 and 7: exit 130, `Wait-UI4ME -UntilShutdown` returned, **no debugger prompt** (before E's console handler both entered "Entering debug mode … [DBG]"). 1.0.1/pre-E: launcher died with 0xC000013A |

Found on Windows and fixed during this verification (E: df14e52, 334ede8; lead: 73e99d6):
four Pester tests that only failed on Windows 5.1 (named-pipe timing in BIND-PS-05, transport-vs-deferral bound in
BIND-PS-03, discovery-time `-Skip` evaluation running POSIX tests and leaking a process), the PowerShell
ConsoleHost Ctrl+Break debugger, a POSIX-only signal-name test, and `run_pester.ps1` failing every Windows run on
the two legitimate POSIX-only skips.

Not verified natively: Ctrl+C typed into a real console window (Ctrl+Break by API only); PowerShell AllUsers on
Windows is not applicable (F-19 is Linux; verified in Docker by E).
