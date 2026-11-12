# UI4ME Core 1.0.0-rc.1 — delivery status

**Complete source candidate; native release qualification remains pending.**
The completion implementation is supplied for Linux and Windows, together with
the Python application API, regression tests, reference application, capability
decisions and autonomous local finishing procedure. This archive does not claim
that the new source has already passed KDE or Windows desktop tests.

Use [LOCAL_FINISH_PROMPT.md](../LOCAL_FINISH_PROMPT.md): qualify Python/Linux KDE
on native Wayland, commit and push; qualify Windows Server 2022 in the normal
test user's RDP session, commit and push; then stabilize and qualify the final
common source and push the final result. Preserve the existing Git history.

The supplied 0.2 source is the baseline. Its reported commit is `63e50dc`; the
input archive contains no Git metadata, so this package does not manufacture
ancestry. The previous report is [BASELINE_0.2_STATUS.md](BASELINE_0.2_STATUS.md).
Historical results qualify only their original source and environments.

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

## Checks completed during preparation

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

## Native work still required

For each target, run the full verifier and Clippy, install the release build,
and complete N01–N15, R01–R12 and C01–C12 with source/binary-bound evidence.
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
- The baseline retained about 19 KiB per closed document window with AT-SPI
  enabled, attributed there to zbus. This candidate has not been measured on
  that desktop; compare the trend and investigate new growth locally.
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
