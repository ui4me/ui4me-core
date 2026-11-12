# Core 1.0 preparation evidence

These checks were executed while preparing `1.0.0-rc.1`. They qualify the
stated source/test behavior only. KDE Wayland and Windows native qualification
remain local work under `LOCAL_FINISH_PROMPT.md`.

## Results

| Check | Outcome | Evidence |
| --- | --- | --- |
| Main workspace Rust tests | 1,066 passed; 0 failed; 11 pre-existing ignored; 12 environment-filtered | [rust-tests.txt](rust-tests.txt) |
| Final preference tests | 4 passed | [preferences-tests.txt](preferences-tests.txt) |
| Optional high precision and hinting | 11 completion regressions passed | [high-quality-tests.txt](high-quality-tests.txt) |
| Python | 48 passed; 1 Windows-only skip | [python-tests.txt](python-tests.txt) |
| Linux Clippy | All workspace targets, warnings denied, PASS | [linux-clippy.txt](linux-clippy.txt) |
| Windows Rust source/type/Clippy | All targets in MSVC source mirror, PASS | [windows-source-check.txt](windows-source-check.txt) |
| Formatting | Five owned packages, PASS | [format-check.txt](format-check.txt) |
| Unsupported quality request | Default build exits 1 with explicit unsupported-feature error | [default-quality-rejection.txt](default-quality-rejection.txt) |
| Reference CPU frame | Resources settled; rendered and visually reviewed | [workspace.png](workspace.png), [JSON diagnostics](workspace-render.json) |

[report.json](report.json) contains the commands, source identity, counts,
filtered test names and qualification limits. Filtered counts in the two
*targeted* runs are ordinary name selection and are not additional environment
failures. Counts from overlapping runs must not be added as unique tests.

## Source revisions and regression findings

The main run includes the complete default-feature workspace suite, including
unit, integration and documentation targets, except for the explicitly listed
transport tests. It includes the new document/editing, accessibility, CSS,
resource, atomicity and pixel regressions.

Review and tests found and corrected a stale selected-file label cache: changing
file selection did not change the DOM revision, allowing old boxes/layout to be
reused. A presentation-label dirty flag now prevents that reuse and survives an
injected failed scene update. The selection/reset and recovery pixel regression
passes with the default and optional higher precision renderers.

The initial accessibility bootstrap expectation was updated for deliberately
exposed label relationships and nonactivating ScrollIntoView actions. The main
suite passes with those semantics. Equivalent Clippy/format corrections were
made afterward. The last native-preference corrections preserve unknown portal
values and pending availability; all four focused preference tests then passed.
The Windows source check found an ambiguous module/crate import, which was fixed
and verified by rerunning the complete Windows source check.

Python tests cover the expanded facade and two diagnostic watchdog failures,
including taskkill failure/timeout and failed reaping. Python source parsing
also checks the executable fixtures. The new completion fixture uses real IPC
and still requires its first local execution; cloud transport restrictions did
not justify replacing it with a mock pass.

## Environment boundary

AF_UNIX listener binding returns EPERM in this cloud environment. A private,
user-owned XDG_RUNTIME_DIR was supplied for Rust checks; ownership checks were
not bypassed. Python socket-pair tests work but do not establish a production
listener or real-process lifecycle result. Four host unit tests and eight
host-process tests are explicitly filtered in the main run. The native local
agent must run them without the filters and run headless reference/fault stages.

The Windows check mirrors the full workspace and preserves dependency features,
patches and lockfile. It disables only the host resource build script, which
requires the Windows SDK. It does not link an executable, run C#/PowerShell,
exercise named pipes/Job Objects or observe an RDP desktop.

The reference image is a 1080×760 CPU render with settled local assets. It was
reviewed for readable text, visible controls and resize grip, local images and
layout. Initial RESOURCE_PENDING entries in its diagnostic history are not
pending work at command completion. This image is not a native desktop or
browser-comparison pass.

## Local finish

Run the complete verifier and installed-release N01–N15, R01–R12 and C01–C12
scenarios on Linux first; commit and push. Then qualify Windows, resolve the
inherited independent UIA text selection/read blocker, commit and push. Finally
run the common-source stabilization and final-version checks and push. Follow
[CORE_1.0_ACCEPTANCE.md](../../CORE_1.0_ACCEPTANCE.md) for exact procedures.
