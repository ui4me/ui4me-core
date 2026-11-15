# 1. Scope, method and limitations

## Audited revision

| Item | Value |
| --- | --- |
| Repository | `ui4me/ui4me-core` (GitHub, **private**) |
| Branch / commit | `main` @ `c6fd71dc5c15dcdfdcdc1500dc156c1b929c216c` (tag `v1.0.1`) |
| Version | 1.0.1, published 2026-11-14 to PyPI (`ui4me-core`) and the PowerShell Gallery (`UI4ME.Core`) |
| Working tree | clean at start and end of the audit; the only addition is this `docs/audit/` directory |
| Audit date | 2026-11-15 |
| Open GitHub items at audit time | issues #1 (backend disconnect on event bursts), #2 (host memory growth); draft PRs #3 (`fix/drop-saturated-notifications`), #4 (`fix/fixed-ancestor-viewport-rect`) |

Size of what was audited (non-blank lines are not counted separately; these are raw line counts):

| Component | Lines | Notes |
| --- | --- | --- |
| `crates/ui4me-renderer` | ~123k Rust | style adapter (Stylo), box tree, layout (Taffy), text (parley), scene, CPU raster (vello_cpu), interaction, a11y |
| `crates/ui4me-host` | ~16k Rust | IPC, lifecycle, supervision, winit integration, platform code (148 `unsafe` sites) |
| `crates/ui4me-document` | ~18k Rust | DOM arena, HTML parse (html5ever), mutation, batch decode |
| `crates/ui4me-assets` | ~19k Rust | images, fonts (WOFF/WOFF2), SVG (usvg/resvg), resource policy, decode workers |
| `crates/ui4me-engine-types` | ~5k Rust | shared types, geometry, diagnostics codes |
| `python/ui4me_core` | ~2.4k Python | client, application facade, direct-start bootstrap |
| `powershell/UI4ME.Core` | ~2.7k PowerShell + C# | module, C# transport |
| `tools/` | ~6k Python | verifier, acceptance harnesses, release tooling |
| Vendored: `patches/stylo`, `patches/taffy`, `third_party/*` (4 crates) | ~225k Rust | Stylo and Taffy as upstream + patch stacks; winit-wayland, winit-win32, softbuffer, accesskit_atspi_common with small patches |

## How the audit was organised

The coordinating session mapped the repository, ran the existing verification, reproduced the two open issues on real platforms and ran its own targeted experiments. Six specialist reviews ran in parallel, each with a written brief, a narrow scope, a ban on modifying the tree, and a fixed output format (classification, evidence, reproduction, severity, likelihood, remediation, alternatives, complexity, confidence):

| Area | Scope | Findings file |
| --- | --- | --- |
| HOST | IPC transport, protocol, auth, lifecycle, supervision, platform `unsafe` | [findings/HOST.md](findings/HOST.md) |
| RENDER | style/layout/text/scene/raster pipeline, invalidation, memory, performance | [findings/RENDER.md](findings/RENDER.md) |
| INTERACT | input, focus, IME, geometry, accessibility, window management, vendored platform patches | [findings/INTERACT.md](findings/INTERACT.md) |
| BIND | Python and PowerShell bindings, direct start, packaging, API parity, DX (the PowerShell part ran as a sub-review) | [findings/BIND.md](findings/BIND.md), [findings/BIND-powershell.md](findings/BIND-powershell.md) |
| INPUTSEC | untrusted input (HTML/CSS/SVG/fonts/images), resource policy, fuzzing, dependencies, licences | [findings/INPUTSEC.md](findings/INPUTSEC.md) |
| RELEASE | tests inventory, CI, publishing, provenance, repository process, release gates | [findings/RELEASE.md](findings/RELEASE.md) |

**Model routing.** Coordinator and all specialists ran on Claude Opus (the coordinator reports `claude-opus-5-5`); specialists and the adversarial review were requested at `xhigh` effort. The audit could not independently observe the subagents' model, only request it.

**Independence.** Specialists were told the project was largely written and "qualified" by AI agents, and to treat docs, comments and recorded qualifications as hypotheses. Where the coordinator re-ran a specialist claim on real hardware, the result is recorded in the register, including two specialist claims that turned out to be wrong in detail (see [02-findings-register.md](02-findings-register.md), "Coordinator verification").

## Environments actually exercised

| Environment | Used for |
| --- | --- |
| Local Linux workstation (Debian 13, KDE Plasma 6 Wayland, 16 cores, 30 GB) | full `tools/verify.py`, unit tests, headless host experiments, heaptrack/gdb |
| **Private nested compositor** `kwin_wayland --virtual --socket ui4me-audit-0` | all *windowed* Linux experiments (issues #1 and #2, nesting depth, SVG, window churn). It never touched the user's real desktop, and no real input was injected. |
| Docker (local daemon) | clean-room contributor build (`debian:13`), PyPI wheel on `python:3.10-slim` and `python:3.14-slim`, PSGallery module on `mcr.microsoft.com/powershell` |
| `winDEV` (Windows Server 2022, VMID 104 on `pvez`) | started from *stopped*; a dedicated local user `ui4me_audit` was created and an RDP session opened from a local Xvfb. All Windows experiments ran inside that interactive session against the **published** 1.0.1 wheel and module. |
| GitHub API (`gh`) | issues, PRs, CI run history and logs, environments, protection, attestations |
| PyPI / PowerShell Gallery | downloads of the published artifacts; hash and attestation checks |

Not exercised: `pvey` and the Windows template (no destructive Windows test was needed), real screen readers (Orca, NVDA, Narrator), real keyboard/mouse input on any desktop, macOS (unsupported), GNOME/Sway (unsupported but plausible user environments), fractional scaling, multi-monitor DPI moves. **Windows coverage is one Windows Server 2022 VM over RDP** (100 % scale, RDP clipboard and IME redirection); Windows 10/11 desktops were not tested.

**Dimensions not covered in depth** (named so readers do not assume they were checked):
- Windows High Contrast / forced colours: the scope doc says `forced-colors` and `prefers-contrast` are not recognised and no forced palette is applied (`docs/concepts/html-css-scope.md`); a real accessibility gap, added to roadmap 8.7, not probed further.
- Bidirectional, RTL and complex-script *editing* (caret movement, selection, deletion in mixed-direction text): claimed in scope, not probed.
- Locked-down Windows (AppLocker/WDAC blocking unsigned executables in user-writable paths; PowerShell Constrained Language Mode blocking the module's `Add-Type`): reasoned only (roadmap 4.3).
- Non-ASCII user profile names and paths; file dialogs without `xdg-desktop-portal`.
- Privacy: the reviewer checked that `Cargo.lock` contains no HTTP or telemetry crates and the bindings make no network calls. No telemetry was found.

## Verification run by the coordinator

`python3 tools/verify.py --keep-going --jobs 8` (default stages) on the clean tree:

| Stage | Result | Time |
| --- | --- | --- |
| fmt | PASS | 1 s |
| check | PASS | 53 s |
| test | PASS (1094 Rust tests passed, 0 failed, 11 ignored) | 183 s |
| python | PASS (120 tool tests, 1 skipped: Windows named pipe) | 21 s |
| headless | PASS | 61 s |
| renderer | PASS | 3 s |
| faults | PASS | 11 s |

The verifier itself states `"native_desktop_qualified": false`: it is build, unit and headless evidence only. Report: [evidence/logs/verify-report.json](evidence/logs/verify-report.json).

The same suite **fails in GitHub CI for every non-release change** (see RELEASE-02): it passed here only because the tree was exactly the release tree.

## Experiments by the coordinator (all against published 1.0.1 artifacts)

| # | Experiment | Platform | Result |
| --- | --- | --- | --- |
| E1 | `burst2.py`: N buttons, 3 transitioned colours each, toggle `set_environment(color_scheme)` every 0.4 s | Linux headless | never fails: **headless emits 0 transition events** |
| E2 | same | Linux windowed (nested KWin) | N=20 fails on 3rd switch, N=100 on 1st: "sending on a full channel" → "Backend connection lost: closed by host" |
| E3 | same | Windows RDP session, Python 3.13 | N=10 passes (1,080 events), **N=20, 40, 60, 80, 100 fail** |
| E4 | `burst.ps1` (same document, PowerShell module) | Windows PowerShell 5.1 | N=5, 10 pass; **N=20, 40 fail** |
| E5 | issue #2 `replay.py` | Linux windowed | bold variant 63.2 → 92.1 MiB in 95 s (~0.31 MiB/s); plain variant flat; headless bold slow growth |
| E6 | `depth.py`: N nested `<div>` | Linux windowed | 30, 60, 250 pass; 256 rejected by the documented limit |
| E7 | same | Windows | 20, 30 pass; **40, 60, 120, 250: `thread 'main' has overflowed its stack`** (host abort). PE header of the published `ui4me-host.exe` (sha256 `8d7eab92…`): `SizeOfStackReserve = 1048576` |
| E8 | `svgdepth.py`: `data:` SVG with N nested `<g>` in an `<img>` | Linux windowed | 100, 300 pass; **700: `thread 'ui4me-assets-0' has overflowed its stack`** (host abort) |
| E9 | same | Windows | 20–500 pass; **700: same abort** |
| E10 | `churn.py`: 400 × (open window, 3 reloads, close) | Linux windowed | pass, host RSS flat at ~50–51 MiB |
| E11 | SIGTERM to the direct-start launcher | Linux windowed | host unit gone in 0.28 s |
| E12 | `pythonw idle.py` (BIND-15 check) | Windows | host runs; no console window, no new `conhost` process ([log](evidence/logs/win-pyw5-result.txt)) |
| E13 | E4 repeated with a version stamp | Windows PowerShell 5.1.20348.5773 Desktop | N=10 passes, N=20 fails ([logs](evidence/logs/)) |

Scripts and logs: [evidence/](evidence/). Specialists' own experiments are summarised in their findings files; their key scripts are retained under `evidence/scripts/<area>/`. Paths written as `<scratch>/…` in the findings files refer to the session-temporary scratch directory and are **not** retained.

## Constraints honoured

- No production source, build configuration, public interface or release artifact was modified. No commit, push, merge, PR, issue or publication was made.
- Experiments used scratch directories, a temporary git worktree (removed) and disposable containers (removed with their images).
- No credentials were printed or stored in the repository. The `winDEV` test user's password is stored only in a mode-0600 file outside the repository.

## Lab assets left in place (final state)

| Asset | State |
| --- | --- |
| `winDEV` (VMID 104 on `pvez`) | **stopped**, as it was found. Baseline snapshot `clean_system_01` untouched; no snapshot created or rolled back. |
| Local user `ui4me_audit` on `winDEV` (member of Remote Desktop Users; logged off) and workspace `F:\agent-workspaces\ui4me-core\audit-ac27d3dd\` (uv-managed Python 3.13, venv with the 1.0.1 wheel, the saved 1.0.1 module, probe scripts, logs) | **kept** as a reusable Windows GUI test environment for the roadmap's verification items (e.g. 6.0). Password in a mode-0600 file outside the repository. Delete with `Remove-LocalUser ui4me_audit` and by removing the workspace folder when no longer needed. |
| Local Xvfb `:89`, `xfreerdp3` session, nested `kwin_wayland` (`ui4me-audit-0`) | **stopped** |
| Docker containers and images used by the RELEASE review | removed |
| `pvey` | not used |
| Repository | only the untracked `docs/audit/` directory added; nothing committed |
