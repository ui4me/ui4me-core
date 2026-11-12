# UI4ME Core 0.2 — implementation and verification status

**The 0.2 implementation is supplied and builds. Native 0.2 qualification is
still pending on the two real target desktops.** This source package is ready
for the autonomous verification/debugging/install assignment in
[LOCAL_FINISH_PROMPT](LOCAL_FINISH_PROMPT.md). It is not labelled a fully
qualified desktop release by its preparation tests.

The locally qualified 0.1 report is preserved separately as
[BASELINE_0.1_STATUS](BASELINE_0.1_STATUS.md), with its historical evidence under
`docs/verification/2026-11-12/`. Its N01–N15 PASS results and measurements belong
to 0.1. They are not reused as new 0.2 results.

## Delivered implementation

The workspace contains one native host and four internal engine crates:
`ui4me-host`, `ui4me-document`, `ui4me-renderer`, `ui4me-assets`, and
`ui4me-engine-types`. The previous renderer application runtime/C ABI was not
migrated. Rust continues to own windows, event dispatch, presentation,
supervision and native services; the Python backend owns application logic.

The implemented integration includes strict document RPCs, atomic replacement,
DOM/style mutation batches, local resources, CPU painting, native input and
scrolling, file inputs, asynchronous native clipboard, IME surrounding text and
delete-surrounding, accessibility publication/actions, static SVG, canonical
HTML input/change events, the reference Python facade, a two-window example,
installation and qualification tools. The production constructor enforces the
selected hosted profile. Page-output APIs and legacy constructors are test-only;
contenteditable is rejected, and no printing/browser/JavaScript layer is exposed.

See [SPEC](SPEC.md) for architecture and limits and
[PROTOCOL_DOCUMENTS](PROTOCOL_DOCUMENTS.md) for the concrete application API.

## Preparation environment

Checks below ran in the preparation environment on 2026-11-12:
Linux x86_64, kernel 6.18.57, glibc 2.41, pinned Rust 1.99.1 and Python 3.12.14.
This is not the user's Debian/KDE Wayland desktop or Windows RDP session.

This environment rejects creation of ordinary AF_UNIX sockets with
`EPERM / Operation not permitted`. A direct independent socket probe reproduces
that restriction. It prevents the host's real Unix transport tests and native
headless process handshake here. Chromium also aborts when its process singleton
tries to create a Unix socket. No permissions or sandbox settings were bypassed
to produce a misleading PASS.

## Executed checks

| Check | Result | Evidence / interpretation |
| --- | --- | --- |
| Formatting of the five owned workspace packages | **PASS** | `format.json`, `format.txt` |
| Linux all-workspace/all-targets Clippy, warnings denied | **PASS** | `linux-clippy.txt` |
| Linux debug host executable build | **PASS** | `linux-build.txt` |
| Engine regular tests | **935 PASS; 9 deliberately ignored** | 214 assets + 190 document + 75 engine types + 456 renderer |
| Engine doctests | **9 PASS; 1 deliberately ignored** | Compile-fail contracts included |
| New hosted engine regressions | **17 PASS** | Included in the 456 renderer tests |
| Host unit tests | **74 PASS; 4 environment-blocked failures** | The unmodified test runner exits 101; see below |
| Host real-process/integration test compilation | **PASS** | `process-tests-compile.txt`; execution remains blocked here |
| Python regression suite | **41 PASS; 1 Windows-only test skipped** | 42 tests total, runner exits zero |
| Windows all-workspace/all-targets source/type/Clippy check | **PASS** | Same sources, features, patches and lockfile; only host SDK build step omitted |
| Windows checked-source identity | **PASS** | 966 compared Rust/build-input files, zero mismatches |
| Four CPU HTML/CSS fixture renders | **PASS** | Geometry, flex/grid, local font and static SVG PNGs; not a Chromium comparison |
| Complete reference application CPU render | **PASS** | Visible styled content, text, controls, local font/image/SVG; headless PNG only |
| Isolated debug installation and installed-example CPU render | **PASS** | Actual installer copied the host and complete example; 1080×760 render from `/tmp`, outside the checkout; not desktop installation qualification |
| Chromium reference comparison | **BLOCKED** | Browser launch fails on the environment's socket restriction |
| Real KDE Wayland / Windows RDP interaction | **NOT RUN HERE** | Required in the local finishing assignment |
| Release-binary size, startup, idle CPU and native churn measurements | **NOT RUN HERE** | Measure the renderer application locally; do not reuse probe-only 0.1 numbers |

Preparation evidence is in `docs/verification/v0.2-preparation/`. The engine
logs include the full four-crate run and a subsequent complete renderer rerun
after the external-SVG scaling fix. The supporting crates were unchanged by
that final renderer-only fix. The deliberately ignored tests are pre-existing
long fuzz/measurement/million-operation harnesses and an illustrative doctest;
the exact names are recorded in `engine-validation-summary.json`. No test was
removed or weakened to obtain the reported results.

The four blocked host tests are:

- `platform::linux::tests::active_listener_cannot_be_replaced_but_stale_socket_can`
- `transport::tests::disconnect_never_waits_for_a_full_gui_queue`
- `transport::tests::incoming_large_frames_obey_the_byte_budget`
- `transport::tests::local_round_trip_and_clean_disconnect`

All fail at the environment's socket creation boundary with `EPERM`. This is
recorded as a blocked requirement, not converted into four passes. All 25 added
host/document/snapshot/accessibility regression tests passed, alongside the
available inherited host tests. The default verifier still runs the full suite
on the user's machines; it does not skip these tests automatically.

The Windows source checker disabled only the host's SDK-dependent resource
build step in a separate generated mirror. It does not validate Windows SDK
resource embedding, executable linking, native execution, RDP, named-pipe
security, Job Object behavior or screen-reader speech. The exact checked input
hash is recorded in `windows-source-current-inputs.json`.

## Integration defects found and fixed during preparation

- **Document replacement races:** every native completion/action is tied to a
  document epoch; stale clipboard reads, pickers, drops and accessibility actions
  cannot act on a replacement document or a reused renderer node identity.
- **Clipboard editing:** native transfers are deferred and bounded. Failed cut
  writes preserve text; delayed paste/cut operations recheck editing context.
  The `copy_selection` RPC admits raw and encoded response sizes before queuing
  its clipboard side effect.
- **Native IME:** bounded UTF-8 surrounding text and byte-boundary validation are
  implemented, password contents are hidden, and replacement/focus resets reject
  late composition events. Cheap mutation-time state updates preserve batching;
  caret geometry is refreshed while rendering.
- **Default actions and event semantics:** prevented keydown now suppresses its
  paired text insertion; Space activates a button once; hosted `input` and
  `change` use their normal HTML meanings instead of donor compatibility aliases.
- **File-input completion:** changing/removing/disabling the input while its
  picker is open invalidates the result. Single-file inputs request a single
  native selection.
- **Static SVG:** percentage roots with a valid viewBox have a usable intrinsic
  render size. External SVGs now request rasterization at display size/device
  scale, rather than stretching their natural-size pixels. Pixel regressions
  cover external and inline rendering at 1× and 2×.
- **Backend callback deadlock:** an application callback waiting for a native
  dialog no longer occupies the only worker available for close/quit policy.
  Ordered document notifications and lifecycle requests have separate bounded
  workers; shutdown does not wait for an arbitrarily blocked notification callback.
- **Bounded behavior and diagnostics:** file reads reject FIFOs without blocking;
  oversized document events produce an explicit error; external file drops fail
  rather than silently truncating paths. Semantic refreshes are coalesced within
  an owner-thread pump batch.
- **Verification tooling:** formatting targets owned packages without traversing
  unused upstream generator workspaces. Windows source checking retains the
  complete engine and patched dependency graph. Native acceptance cannot report
  overall PASS from incomplete observations and records source/binary identity.

## Preserved foundation and provenance

[SOURCE_PROVENANCE.json](SOURCE_PROVENANCE.json) records SHA-256 values of the
supplied qualified host archive and renderer donor, and the byte-preservation
checks. The source ZIP has no Git metadata; baseline commit `4123f73` is the
user-reported local commit, not a fabricated ancestor in this package.

The qualified winit-wayland and softbuffer patch trees are byte-identical to the
supplied 0.1 trees. AccessKit's adapter teardown fix is byte-identical and combined
with the donor's expanded/invalid state mapping. The host's backend supervision,
configuration, lifecycle module, transport, logging, platform containment and
probe implementation remain unchanged. Existing dependency versions from the
0.1 lockfile were retained; the host product version changed to 0.2.0 and renderer
dependencies were added. Carried Stylo/Taffy runtime source trees are unchanged
from the donor; their notices and patch files are included.

## Remaining qualification and known limits

Run the complete automated verifier, N01–N15 and R01–R12 on the final source tree
in both required environments, install the release application as a normal user,
exercise the actual desktop menu path, and record measurements. The local agent
must debug any defects those runs reveal and commit the tested result without
pushing. See [LOCAL_FINISH_PROMPT](LOCAL_FINISH_PROMPT.md) for the full assignment.

The 0.1 known limitations remain historical risks to recheck: approximately
3.3 KiB retained per closed window in zbus on Linux; KDE's folder picker/app
chooser parenting limitations; the accessibility application name; and the
probe font's limited glyph coverage. The HTML renderer has its own shaping and
font fallback pipeline, but installed fonts still determine script coverage.
Actual screen-reader speech, mixed physical monitors, physical Windows hardware,
X11 and macOS are not claimed tested by this delivery.

Basic static SVG support does not claim all browser-equivalent percentage sizing
for roots without a viewBox. Diagnostics include retained stage/resource history;
a historical `RESOURCE_PENDING` entry is not the same as a live
`pending_resources: true` state. The reference snapshots wait until live local
resource work has settled.

Asset limits are component budgets, not total process-memory promises. At sixteen
loaded documents the configured decoded/vector/cache plus SVG raster allowances
can total 768 MiB, with a transient old+new document during replacement. Frames,
fonts, DOM/layout, transport and allocator costs are additional. Actual idle CPU,
release size and sustained memory behavior must be measured with the real 0.2
application.
