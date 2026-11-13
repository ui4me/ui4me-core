# UI4ME.Core (PowerShell) 1.0.0 — qualification evidence, 2026-11-13

Native qualification of the PowerShell module `UI4ME.Core` 1.0.0 against the **unchanged**
UI4ME Core 1.0.0 native hosts (taken from the published `v1.0.0` wheels). No Rust or Python
source was changed. All runs below used the same staged module; its files are hashed in
[`module-files.sha256`](module-files.sha256) and its hosts in
[`HOST-SOURCE-MANIFEST.json`](HOST-SOURCE-MANIFEST.json).

## Identities

| Item | SHA-256 |
| --- | --- |
| `UI4ME.Core.psm1` | `57ddaa5ef408c24ca172b609c1ead7db40d05415efc40e0baff07cfe9e144378` |
| `UI4ME.Core.psd1` | `bea30b74db0708dc6e49c594ddadf186b544336b9652694715ffcb8931dd2d12` |
| `private/UI4ME.Transport.cs` | `227178342e547de5dce641ee7bd9d0dd5efd0895c87bfaff88f3d2a940653aad` |
| Linux host `runtimes/linux-x64/ui4me_core/_bin/ui4me-host` (41,037,056 B) | `22ba596e499b3b8d94c4fee6092e6dd96e4e3ae9899761acd8c232831bf015e2` |
| Windows host `runtimes/win-x64/ui4me_core/_bin/ui4me-host.exe` (30,220,800 B, console subsystem) | `c56698b654e794db2820bff73a2c8c7d658417c29ed511eaf6518a80e8ddaa47` |
| Source wheel `ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl` | `4e368121dc06387b966fb3f3653f636740eda78f88aed787b056d7eeccc71cb4` (release `SHA256SUMS`) |
| Source wheel `ui4me_core-1.0.0-py3-none-win_amd64.whl` | `6a3606d0ac99d403c0eb4eeb22e055f5b3147f14a30c46d5b0175a8d94d19fcc` (release `SHA256SUMS`) |

The wheels were downloaded from GitHub release `v1.0.0` (source commit `d053e09`) and verified
with its `SHA256SUMS` before staging. `tools/release/verify_psgallery.py` result:
[`verify_psgallery.json`](verify_psgallery.json) (PASS, 82 exported functions, 6 native files).
The Linux host links only `libc`, `libm`, `libgcc_s`; there are no relocated libraries.

## Environments

| Platform | Desktop | Backends | Input |
| --- | --- | --- | --- |
| Linux | Debian 13, KDE Plasma 6.3, native Wayland, 2048×1080 @1×, the user's real session | pwsh 7.7.0 (.NET 11); pwsh 7.2.24 (.NET 6) for Pester | uinput pointer/keyboard through KWin |
| Windows | Windows Server 2022 10.0.20348 (`omegaui`), normal user `ui4me_test` in RDP session 2, 1600×1000 @96 DPI | pwsh 7.6.9 (.NET 10); Windows PowerShell 5.1.20348 (.NET Framework 4.8) | genuine RDP mouse/keyboard (xdotool → xfreerdp3) |

On Windows the module was installed as a normal user into
`Documents\PowerShell\Modules\UI4ME.Core\1.0.0` and `Documents\WindowsPowerShell\Modules\UI4ME.Core\1.0.0`
and imported **by name**; the host ran from inside the installed module. No Python ran in any
application process (the test drivers are separate tooling processes).

## Results

| Gate | Linux pwsh 7.7 | Windows pwsh 7.6 | Windows PowerShell 5.1 |
| --- | --- | --- | --- |
| Pester (unit + transport against a socket/pipe peer) | 43/43 (also 43/43 on pwsh 7.2.24 + Pester 5.7.1) | 43/43 | 43/43 |
| Native full run (`tests/native/Qualify.ps1`, 28 checks) | 28/28, host exit 0 | 28/28, host exit 0 | 28/28, host exit 0 |
| Windows extras (UIA COM TextPattern, Microsoft Pinyin) | n/a | 2/2 | 2/2 |
| Backend killed while streaming bitmaps | host exit 1 (reported failure), no `ui4me-bitmap-*` left in `$XDG_RUNTIME_DIR` | exit 1, none left in `%TEMP%` | exit 1, none left in `%TEMP%` |
| Doc examples (`DocExamples.ps1`, 13 checks) | 13/13 | — | — |
| Examples + `New-UI4MEProject` app | counter, live-bitmap, frameless, generated: pass | — | — |

Raw records: `native/<run>/results.jsonl` (backend checks, each with its detail) and
`native/<run>/harness-*.json` (driver observations, window geometry, process tree, exit code).
Pester transcripts: `pester/`. Summary: [`summary.json`](summary.json).

### What the 28 native checks cover

Automatic, against the real host: capabilities/preferences/window list; HTML file in a folder
with spaces and non-ASCII characters plus relative stylesheet; Unicode text round-trip and
escaping; atomic batch commit and rollback; classes, attributes, HTML insertion, removal and
selector query; opaque nodes (query, inspect, children, node batch); stylesheet
add/replace/list/remove with layout verification; form values, checkbox, select, selection,
form data and reset; environment overrides, scroll, focus, diagnostics; generated resource
bundle; system clipboard round trip **and** interop with another application (wl-paste/wl-copy,
Win32 clipboard); typed RPC errors and external-open policy (Windows refuses `.cmd`);
live bitmaps 64×64, 320×200 and 1024×768 × 1000 frames each, sequence/stride checks, stale
handle rejection, ≥30 frames/s floor; bitmap file cleanup; second window with close veto then
allow and handle retirement; document replacement making old documents/nodes stale locally and
at the host; RPC timeout/cancel leaving no pending state.

Interactive, with real input: left click, right click (`contextmenu`, button 2) and middle click
(`auxclick`, button 1); typing into a focused input (`input` per edit, `change` on Tab);
prevent-default registered before a real submit; event ordering while a slow callback makes
RPCs (`target-start,target-end,other`); native save dialog returning the exact non-ASCII path
with spaces; native open dialog cancelled from PowerShell (`Stop-UI4MERpc`) and verified gone;
maximize/minimize/restore; frameless title-bar drag and all eight resize edges
(n, s, e, w, ne, nw, se, sw) measured by the compositor/Win32; HTML title-bar buttons driving
native maximize/restore/minimize; native close vetoed by the PowerShell policy, then allowed,
followed by orderly host shutdown.

### Measurements

| | Linux pwsh 7.7 | Windows pwsh 7.6 | Windows PowerShell 5.1 |
| --- | --- | --- | --- |
| Host start → backend ready (incl. C# compile) | 0.70 s | 0.43 s | 0.26 s |
| Live bitmap frames/s, 64×64 / 320×200 / 1024×768 | 3464 / 2559 / 445 | 601 / 567 / 240 | 619 / 574 / 238 |
| Backend working set during bitmap runs | 249–264 MB | 191–197 MB | 89–102 MB |
| Managed heap growth over 3000 frames | 7.2 MB | 7.3 MB | 6.9 MB |
| Idle CPU (20 s, 40 s after ready): backend / host | 0.20 % / 0.25 % | 0.07 % / 0.07 % | 0.05 % / 0.07 % |

The bitmap producer runs faster than the display; `Send-UI4MELiveBitmap` returns when the host
has taken the frame, so memory stays flat. Idle CPU right after start is higher (up to ~4 % for
~35 s on Linux) because of .NET JIT tiering and GC; it then settles to the values above.

Windows: the backend has no visible console window (its `conhost.exe` is hidden) and the app
shows one taskbar button; the host's helper window is a non-activating tool window.

## Defects found and fixed during qualification

| # | Defect in the supplied overlay | Fix |
| --- | --- | --- |
| P1 | Pester suites could not run (Pester 5 discovery scoping) | Setup moved into `BeforeAll`; shared helpers |
| P2 | `ConvertTo-Json`/`ConvertFrom-Json` quirks: Windows PowerShell 5.1 array wrapping, PowerShell 7 date coercion of strings, depth limits | Strict JSON codec in the C# helper (duplicate keys and non-finite numbers rejected) |
| P3 | A full callback queue killed the connection; responses could be starved by notifications | Reader classifies frames; responses bypass the bound; overflowing notifications are counted and dropped, requests get `-32005`, lifecycle requests always admitted (Python parity) |
| P4 | Reader thread wrote rejections synchronously: deadlock on Windows pipes when the peer was busy writing | Dedicated bounded rejection writer |
| P5 | Unix-socket connect disposed its wait handle before `EndConnect` (`ObjectDisposedException` on slow connects) | Handle kept until `EndConnect` |
| P6 | Notifications re-entered inside a running callback (out of order) | Notifications deferred while a callback runs; lifecycle requests still answered |
| P7 | `MemoryMappedFile.CreateFromFile` overload absent on .NET Framework | Mapping in C# with a `UI4ME_NETFX` variant |
| P8 | Windows PowerShell 5.1: 1024×768 frames at 6 frames/s (mandatory `[byte[]]` parameter binding inspects every element) | Untyped `-Pixels` with explicit conversion; bulk `Marshal.Copy` into the view |
| P9 | `byte[]` returned from functions arrives as `object[]` and was rejected | Coerced for resources and bitmaps |
| P10 | `-Try` re-entrancy guard used a re-entrant monitor (no protection on one thread) | Per-bitmap busy flag |
| P11 | Non-ASCII scripts without BOM misread by Windows PowerShell 5.1 | Module sources ASCII; scripts with non-ASCII text carry a BOM; enforced by a test |
| P12 | Fallback to `ui4me-host` on `PATH` could start the Python console wrapper | Removed; only the bundled host or `UI4ME_HOST_BINARY` |
| P13 | NuGet extraction drops the Linux execute bit | `Get-UI4MEHostPath` restores it |
| P14 | Examples imported the module only by name, live-bitmap example drew per pixel (seconds per frame) and spun a CPU | Installed-or-checkout import; `BlockCopy` frames paced to 60 frames/s |
| P15 | Misc. contract fixes: `$Event` parameter shadowed an automatic variable, policy results taken from all output, stale-document checks by id instead of identity, missing `resizable`/typed `Set-UI4MEWindow` fields, Wayland-refused `focus` used in docs | Fixed; documented |

## Reused host evidence (not re-proved here)

Host rendering, CSS, accessibility trees beyond the TextPattern read above, NVDA/Orca journeys,
Explorer/Dolphin drag and drop, DPI scaling other than 96 DPI/1×, and the 39 native N/R/C
scenarios are properties of the unchanged host binary and are covered by the Core 1.0.0
evidence in `../core-1.0-linux-final-2026-11-13/` and `../core-1.0-windows-final-2026-11-13/`.
They were not repeated with a PowerShell backend; the PowerShell binding only transports the
same events and commands.

## Reproduce

```bash
python tools/release/build_psgallery.py --linux-wheel … --windows-wheel … --out dist/psgallery
python3 powershell/tests/native/drive_linux.py --host dist/psgallery/UI4ME.Core/runtimes/linux-x64/ui4me_core/_bin/ui4me-host \
  --module dist/psgallery/UI4ME.Core/UI4ME.Core.psd1 --out out/linux-full --mode full
python3 powershell/tests/native/drive_windows.py --edition ps51 --mode full --out out/win-ps51-full   # lab VM + RDP
```

The drivers require the lab tooling described in `powershell/tests/native/drive_*.py`.
