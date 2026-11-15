# Windows verification of integration `b2a31a4` (A + B + D + F merged, plus `a664fb8`)

winDEV (Windows Server 2022, VMID 104), source clone `F:\agent-workspaces\ui4me-core\stab-374b06f6\src` at `b2a31a4`,
`cargo build --release --locked -p ui4me-host` (MSVC, pinned toolchain 1.99.1).

## Native tests (release)

| Command | Result |
| --- | --- |
| `pe_stack.py target\release\ui4me-host.exe` | `SizeOfStackReserve=33554432 (32.00 MiB)` (1.0.1: 1048576) |
| `cargo test --release -p ui4me-host` | lib 94 passed; integration 8 passed, 1 ignored (includes transport T2) |
| `cargo test --release -p ui4me-renderer --test stack_depth` (T1) | 8 passed, 2 ignored |
| `cargo test --release -p ui4me-assets --test svg_depth` | 8 passed, 1 ignored |

## Windowed probes (interactive RDP session of `ui4me_audit`, real windows; Python 3.13.18 from source, `UI4ME_HOST_BINARY` = the built release host)

| Probe | 1.0.1 published wheel | integration `b2a31a4` |
| --- | --- | --- |
| `depth.py 40` | main-thread stack overflow | `DEPTH_PASS 40` |
| `depth.py 250` | main-thread stack overflow | `DEPTH_PASS 250` |
| `depthkind.py 250 fieldset` | not run (aborts at 40 divs) | `DEPTH_PASS 250` |
| `svgdepth.py 500` | pass | pass (image refused by the depth cap; host alive) |
| `svgdepth.py 700` | `ui4me-assets-0` stack overflow | pass (image refused; host alive) |
| `burst2.py 20 6 env` | pass/fail varies (audit: fail) | `PROBE_PASS`, 6/6 rounds |
| `burst2.py 100 6 env` | backend disconnected | `PROBE_PASS`, 6/6 rounds, no host warnings |

The Python binding still drops part of the burst on its side (2,875 events counted over six switches at N=100,
far fewer than emitted): that is F-29 (workstream E), not the host. Not run yet: Windows PowerShell 5.1 burst,
file dialogs / drag and drop / UIA after moving the engine off the main thread.
