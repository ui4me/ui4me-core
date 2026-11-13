# UI4ME Core 1.0.0-rc.1 — status

**Linux/KDE Plasma Wayland checkpoint qualified; Windows Server 2022 not yet run.**
On 2026-11-13 the candidate passed the full automated verifier, Clippy and all
N01–N15, R01–R12 and C01–C12 native scenarios on Debian 13 / KDE Plasma 6.3
(native Wayland) with the installed release host
(`fc422be508059785022d3937d277e51d14e80be4f3731a47fdfff6828bdecc33`, source
commit `d94ecfc`). Seven product defects found by those scenarios are fixed in
this tree. Evidence, measurements and open findings:
[core-1.0-linux-2026-11-13](verification/core-1.0-linux-2026-11-13/README.md).

**Windows qualification is pending** (stage 2 of `LOCAL_FINISH_PROMPT.md`). The
0.2 Windows UI Automation selection stall remains an open blocker, and every
shared-code fix made here still needs Windows verification. The version stays
`1.0.0-rc.1`; no final release gate has been passed.

### Linux checkpoint summary

| Gate | Result |
| --- | --- |
| `tools/verify.py` (fmt, check, test, python, headless, renderer IPC, faults) | PASS; Rust 1,085 passed / 0 failed / 11 pre-existing ignored, nothing filtered |
| Clippy, all targets, `-D warnings` | PASS |
| Native ledgers (installed host) | host 15/15, renderer 12/12, completion 12/12 PASS, each with its automated subset |
| Browser oracle | R12 4/4 PASS; C10 focused fixtures match Chromium geometry |
| Measurements | ready median 569 ms, idle host 0.22 % CPU, 59.6 MB host+backend PSS; DOM churn flat |

Defects fixed during this checkpoint: keyboard-only Wayland clipboard
(winit-wayland seat patch), dropdown options/value missing from AT-SPI, node
references aliasing across replaced documents, dark UA control colors on pages
without `color-scheme` (Stylo P-006-20), decoded rasters retained after bundle
replacement or image removal, animations surviving removal of their stylesheet,
and AT-SPI table/tree/live/relationship metadata (accesskit_atspi_common patch).

Open findings, reported rather than closed: a residual ~17–19 KiB host PSS
growth per replacement of a *displayed* resource-bundle image (unattributed);
and the C04 wording about preedits on focus change versus the retained,
browser-like behavior of keeping the composed text in its own field. Both are
detailed in the checkpoint README.

The supplied 0.2 source is the baseline (reported commit `63e50dc`, verified as
the repository ancestor). The previous report is
[BASELINE_0.2_STATUS.md](BASELINE_0.2_STATUS.md). Historical results qualify
only their original source and environments.

## Implementation delivered

The [fixed specification](CORE_1.0_SPEC.md), [protocol additions](CORE_1.0_PROTOCOL.md)
and [capability ledger](CAPABILITY_SCOPE.md) define the scope. All 125 audit rows
have decisions: 37 completion implementations, 17 retained core capabilities,
8 explicit profile boundaries, 61 outside this core, and 2 assigned to the
application/UI library. Excluded items are not a promised later batch.

- Anonymous node queries and atomic mutations; node-aware focus, blur, focus
  inspection and hit testing, with stale document/node rejection.
- UTF-16 selection and replacement, native undo/redo and edit commands, Unicode
  grapheme handling, form reset/request-submit/custom validity and ordered form
  entries preserving duplicate names, submitters and file metadata.
- Expanded Python facade, close-policy routing, cancellation, native service
  helpers and bounded resource registration, plus a real-IPC completion fixture.
- Opt-in native file/text drops, additional input notifications, accessible
  table/tree and relationship metadata, busy/atomic live state, assistive
  ScrollIntoView, Linux application identity and combobox selection exposure.
- Window size constraints and requested/observed state; native attention,
  color/motion/contrast observation, parent-aware external open, native reveal
  and explicit platform availability reporting.
- Intrinsic/balanced flex, selected-option sizing, font-face feature/variation
  defaults, break-word, logical resize grips and inset corner radii. PNG bitmap
  color glyphs, optional hinting and optional higher precision CPU rasterization
  are included within the documented profile.
- Visible selected-file labels, bounded atomic resource bundles, decoded/SVG
  cache accounting and unused-resource purging. Native presentation uses the
  supplied buffer directly; retained full-frame raster storage is reused.
- A static default reference page, three native acceptance ledgers, independent
  Windows UI Automation probes and bounded diagnostic process cleanup.

## Checks completed during cloud preparation (historical)

Exact commands, counts, logs and scope are in the
[preparation report](verification/core-1.0-preparation/README.md) and its
[machine-readable record](verification/core-1.0-preparation/report.json).

| Check | Result and scope |
| --- | --- |
| Main Rust workspace run | **1,066 passed, 0 failed**, 11 pre-existing ignored harnesses, 12 explicitly filtered transport/process tests |
| Final native-preference regressions | **4 passed**, including unknown portal values and pending availability |
| Python suite | **48 passed, 1 Windows-only skipped**, 49 total |
| Linux Clippy, all workspace targets | **PASS**, warnings treated as errors |
| Windows MSVC source/type/Clippy check | **PASS**, all workspace targets; only the host resource build script disabled in a generated mirror |
| Owned workspace formatting | **PASS** |
| Default-build high-quality request | **PASS**: explicit unsupported-feature error without the required build feature |
| Optional high-quality raster and glyph hinting | **11 targeted regressions passed** with both options enabled |
| CPU reference render | **PASS**: local resources settled; 1080×760 PNG generated and visually reviewed |
| Audit/source consistency | All 125 unique IDs and counts verified; referenced implementation/test paths exist; 27 Python source files parse |
| Dependency preservation | Upstream package identities, versions, sources and checksums unchanged; existing native fixes preserved |

The main Rust run preceded the final pending-preference regression and equivalent
lint/format corrections. The four preference tests then passed. The final
Windows check covers the corrected Windows import. Supplementary optional-raster
results are recorded in the preparation report.

**The 12 filtered tests are not passes.** This environment rejects AF_UNIX
listener binding with `EPERM`. Socket-pair Python tests work but do not qualify
the production socket. Four host unit tests and eight real-process integration
tests must run unfiltered locally. This also prevents qualification of real-IPC
headless reference and fault fixtures here. No alternate production transport
or test-only success fallback was introduced.

The Windows source check does not link an executable or exercise Windows,
named pipes, Job Objects or RDP. C#/PowerShell UIA execution remains local work.
Neither source checking nor a headless PNG qualifies native input, IME,
accessibility, services or presentation.

## Open release blocker: Windows UI Automation text selection

The 0.2 report records an independent UIA client stalling while selecting or
reading a text selection, despite a responsive application and working NVDA
speech. **That blocker is not marked fixed here.** The old diagnostic combined
operations, so it did not identify the failing call or prove an AccessKit
provider deadlock.

The new diagnostic isolates TextPattern acquisition, DocumentRange text,
Select, GetSelection and selected-range GetText in separate client processes.
It uses an explicit MTA or STA thread, logs before/after markers, reads initial
selection before changing it and has bounded watchdog cleanup. MTA is the
required client; STA is a separately recorded comparison. The local agent must
compare an actual native text control and the simpler host probe, reproduce the
cause and fix any host/provider defect. No speculative AccessKit Windows
provider patch is included. See the
[R05 procedure](CORE_1.0_ACCEPTANCE.md#windows-r05-isolate-each-uia-operation).

Do not waive this gate because NVDA works, disable text patterns or turn a
timeout into an empty successful selection. A harness-caused failure can be
closed only with corrected compliant-client and control-comparison evidence.

## Native work still required (Windows)

Linux is complete for this source (see above). For Windows, run the full
verifier and Clippy, install the release build, and complete N01–N15, R01–R12
and C01–C12 with source/binary-bound evidence, including the R05 UIA matrix.
The [acceptance procedures](CORE_1.0_ACCEPTANCE.md) cover lifecycle, services,
real IME, Orca/NVDA, independent UIA, document interactions, installed launch and
resource/performance measurements. Runtime availability is separate from
implementation and qualification in the capability API.

After Windows fixes affect shared code, rerun the affected Linux cases. Final
release gates run against the common source. Set `1.0.0` only after those gates,
then rebuild/reinstall and verify that version too.

## Explicit limits and inherited issues

- Linux targets native Wayland. The pinned backend does not implement explicit
  visibility changes or programmatic focus; these now return errors instead of
  claiming success. Attention remains compositor-dependent.
- Windows IME field-purpose hints are not applied by the pinned windowing
  backend. Ordinary composition remains supported and must be retested.
- The baseline KDE folder picker was not attached despite the parent request.
  External reveal APIs cannot currently forward a parent. Service acceptance
  does not prove that another application's window appeared.
- With AT-SPI enabled, about 18–21 KiB is retained per closed document window
  on KDE (measured 2026-11-13; 0.2: ~19 KiB), attributed to zbus object-path
  retention. accesskit_unix 0.24.1 provides no AT-SPI Table/TableCell
  interfaces; table indices are published as object attributes.
- Rendering remains CPU based. Animated content costs work even though the
  default reference is static. Do not relabel 0.2 performance as new measurements.
- Static SVG and the declared HTML/CSS profile are supported. This is not a
  complete browser, networking/JavaScript runtime, printing system, GPU renderer
  or full forced-colors implementation. Font fallback depends on installed
  fonts; PNG glyph support does not claim every color-font format.
- Physical Windows hardware, physical multiple monitors, X11 and macOS are
  outside the present qualification environments.

## Integrity and history

[SOURCE_PROVENANCE.json](SOURCE_PROVENANCE.json) identifies the input archive and
audit by SHA-256. The Wayland, softbuffer and Windows IME vendor trees, AT-SPI
teardown, transport, supervision/lifecycle and Python transport client were
checked byte-for-byte against the supplied baseline. The new AT-SPI
identity/selection change is a separately documented small patch.

Run `python3 tools/verify_package.py` before editing. `docs/SOURCE_FILES.sha256`
covers delivered files and excludes itself. After local fixes, preserve the
original delivery manifest and produce a new final evidence record; do not
alter checks to hide differences.

This is complete source, assets, patches and tests. It excludes Git metadata,
private lab configuration, build products and prebuilt platform binaries.
Setup needs the pinned toolchain, native prerequisites and dependency downloads.
No local desktop, account, VM, installation or remote Git repository was
modified during this cloud preparation.
