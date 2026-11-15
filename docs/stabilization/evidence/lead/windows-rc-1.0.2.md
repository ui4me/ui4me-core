# Local 1.0.2 release candidate: Windows wheel built and qualified (F-11, F-21)

**What this is.** A release candidate built on winDEV from the integration branch at `bba9e6d` (version 1.0.2)
with the documented recipe, [`build_rc.cmd`](build_rc.cmd) → `tools/release/build_windows.ps1`: the sdist first,
then the wheel built from that sdist. The exact wheel file was then qualified with
`tools/release/qualify_windows_rc.py` in the interactive desktop session of `ui4me_audit`
([`job_qualify.ps1`](job_qualify.ps1)).

**What this is not.** It is not the artifact that would be published. The release process
(`docs/release/qualification.md`) qualifies the wheel that the CI `build-dist` run built for the tagged commit.
That needs the branch pushed and a GitHub Actions run, which were not authorized in this engagement. The
publish workflow pins the qualified hash (`expected_windows_wheel_sha256`). This run proves the tooling and the
artifact checks end to end on a real wheel. It also gives the expected behaviour for the CI-built one, whose
hash will differ unless the build is reproducible.

| Item | Value |
| --- | --- |
| Source commit | `bba9e6d` (stabilization/integration) |
| `ui4me_core-1.0.2.tar.gz` SHA-256 | `258e467651744c112eb032c7243a7ded28b02725511c5e8f724b8d43eca7ba85` |
| `ui4me_core-1.0.2-py3-none-win_amd64.whl` SHA-256 | `2c2dca519bde0fc2f9e428f6415f182531627d20f64f399f3466fc8a78e926c1` (13,432,641 bytes) |
| embedded `ui4me-host.exe` SHA-256 | `a0b16943ffb790804e89f723426b9fb596a887f19791eeb05edf42533ee5e246` |
| `verify_windows_artifact` | PASS (PE32+, console subsystem, `SizeOfStackReserve` 33554432, no debug host); `twine check` PASS |
| Qualification machine | Windows Server 2022 (10.0.20348), Python 3.13.18, interactive RDP desktop |

Qualification probes, run against a fresh venv that holds only that wheel (`pip install --no-index --no-deps`):

| Probe | Result |
| --- | --- |
| first frame | PASS (text rendered, 1.0 s) |
| `depth.py 250` (1.0.1: main-thread stack overflow at ~40) | PASS, 2.1 s |
| `svgdepth.py 700` (1.0.1: asset-worker overflow) | PASS, 2.5 s |
| `burst.py 100 6` (1.0.1: disconnect at 40 and 100 controls) | PASS, 6,316 events, 3.7 s |

Full record: [`qualification-windows-1.0.2-local-rc.json`](qualification-windows-1.0.2-local-rc.json).

To release: push, let `build-dist` build the candidate, run the same qualification on its Windows wheel, and pass
that record's hash to `publish.yaml`.
