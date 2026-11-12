# Implementation and verification status

Prepared on **2026-11-12**.

**Delivery state:** implemented source package, buildable/installable on the
preparation Linux environment, with native desktop qualification still pending.
This is not a claim that KDE or Windows RDP behavior has already passed.

The repository contains one Cargo workspace member, ui4me-host. Its shared
controller, Linux and Windows adapters, private protocol, supervision,
cancellable native dialogs, input/accessibility probe, Python client, examples,
fault fixtures, install/launcher tools and local finishing prompt are supplied
as working source implementations. There are no intentional native-platform
stubs or deferred method bodies for the declared v0.1 services.

## Checks completed here

| Check | Result | Evidence |
| --- | --- | --- |
| Rust formatting | PASS | verification/format.txt |
| Linux workspace/all-target Clippy, warnings denied | PASS | verification/linux-clippy.txt |
| Runnable Rust library regression tests | **44 passed** | verification/linux-unit-tests.txt |
| Four real Unix listener tests | BLOCKED by environment; explicitly filtered from the passing run | verification/cloud-ipc-environment.json |
| Seven real host/backend process scenarios | Compiled; execution blocked by local endpoint restrictions | verification/linux-process-tests-compile.txt |
| Linux host executable build | PASS | verification/linux-build.txt |
| Source installation into an isolated prefix, with standalone Python example | PASS, debug profile | verification/installed-consumer.txt |
| Installed executable diagnostic command | PASS | verification/doctor.txt |
| Windows Rust source/type/lint check, warnings denied | PASS, with exclusions below | verification/windows-source-check.txt |
| Python client/tool regressions | **17 passed, one Windows-only test skipped** | verification/python-tests.txt |
| CPU probe rendering and visual inspection | PASS; no native window involved | verification/probe-render.png and probe-render.txt |
| Python source compilation, Bash syntax, five PowerShell parser checks | PASS | tools/README.md |
| Windows shortcut helper C# compilation | PASS; COM behavior not executed | tools/README.md |
| Linux launcher validation, actual GIO launch, update and ownership-safe uninstall | PASS | tools/README.md |

The Rust library tests cover framing and partial reads, escaped-message bounds,
configuration validation, application lifecycle, close/quit transactions,
duplicate-ID failure, startup cancellation, deferred dialog-parent handling,
unreachable error-window fallback, Unicode editing/composition, accessibility
selection, icon decoding, log backpressure, process cleanup/reaping, instance
identity and namespace validation.

The seven binary scenarios are retained in the repository. They cover atomic
quit veto and nested RPC, authentication/malformed requests, disconnection,
backend crash, oversized frames, startup timeout and close before readiness.
Their managed backend is a Rust test subprocess, so they do not require Python.
The ignored fixture test is intentionally launched as that subprocess by the
integration tests; it is not an omitted acceptance scenario.

### Why some tests could not execute

The preparation environment is a Linux cloud container that returns EPERM
when creating pathname Unix socket endpoints. A private runtime directory does
not remove that restriction. Socket-pair, pure controller and ordinary process
tests can run; real listener tests and the actual host/backend launch cannot.
An attempted native Wayland test setup was stopped after confirming this
restriction. No fallback transport was substituted to manufacture a pass.

An initial unrestricted library test run produced four failures at socket
creation for this reason. The passing 44-test report explicitly names the four
filtered listener tests. Default local/CI verification still runs those tests.

This environment also has no actual KDE Wayland desktop or Windows RDP session.
No Windows VM or Proxmox recovery action was performed here.

### What the Windows source check means

The checked sources are the actual repository library, executable, unit and
integration tests, and diagnostic-render example. They were checked for
x86_64-pc-windows-msvc with the same dependencies/features and lockfile.

The check uses a temporary manifest that omits the build script because this
environment lacks the Windows SDK/resource compiler. **It does not verify
Windows resource compilation, executable linking, named-pipe security at
runtime, Job Object cleanup at runtime, native dialogs, UI Automation or RDP.**
The delivered repository retains the real build script, manifest and icon.

The reproducible source-only command from a non-Windows host is:

    rustup target add --toolchain 1.99.1 x86_64-pc-windows-msvc
    python3 tools/windows_source_check.py

The Windows local agent must use the actual workspace build and native tests,
not substitute this source check for Windows qualification.

## Material limits and decisions

1. **Native qualification is required.** All mandatory N01–N15 observations
   still need passing evidence on KDE Wayland and Windows Server 2022/RDP.
   The acceptance tool keeps unexecuted work blocked.
2. **winit is an exactly pinned prerelease.** Version 0.31.0-beta.4 supplies
   the data-transfer and expanded IME APIs used here. This is a dependency
   stabilization risk, not a reason to silently change the public contract.
3. **Native transfer allocation is not a hard memory sandbox.** The host caps
   accepted clipboard text and transfer payloads at 1 MiB, but the pinned
   upstream Wayland backend may read/allocate native source data before the
   host receives it. The host's IPC frame/queue limits do not bound that
   upstream allocation. Test realistic large transfers and record this limit.
4. **The probe is not a text/rendering engine.** It is a single-line editable
   diagnostic surface with Unicode-safe storage and IME hooks. Noto Sans and
   fontdue do not provide full script shaping or font fallback. These belong
   to the future renderer.
5. **Headless and unsupervised modes are explicit diagnostics.** Headless
   windows are logical records. Linux process-group fallback cannot qualify
   cleanup after abrupt host death; normal release tests must use the
   systemd user-service path.
6. **Focus and placement obey the OS.** Wayland top-level placement is
   unsupported, and focus requests cannot override compositor policy.
7. **Performance remains unmeasured on the target desktops.** No startup,
   memory, idle-CPU or production binary-size result is fabricated here.
   The isolated install test used the debug profile. Normal installation
   builds the configured release profile locally.
8. **Protocol v1 uses Unicode paths.** Non-Unicode native file selections
   return unsupported; unusable file-drop paths are omitted. There is no
   silent lossy filename conversion.

## Local completion

Execute docs/LOCAL_FINISH_PROMPT.md. The remaining task is to run the supplied
implementation and scenarios in the real environments, correct any discovered
defects, install it for normal-user use and make the local integration commit.
No new feature architecture is assigned to the finishing agent.

The prompt includes the existing authorization for administrator@<windows-vm>,
the dedicated account and F: workspace, native work inside its actual RDP
session, and verified recovery of only the windows-vm guest through root@<proxmox-host>.

After both targets pass, replace this preparation status with the actual target
versions, commands, measured performance, native evidence and commit identity.
Do not report release readiness while a mandatory native check is still
unexecuted or failing.
