# Core 1.0: fixed scope and implementation contract

This is the completion specification for **1.0.0-rc.1**, built on the supplied
0.2 source whose reported final local commit is `63e50dc`. A source archive
cannot prove Git ancestry; the local completion procedure preserves the real
repository and imports this tree on top of it. This candidate includes both
Linux and Windows implementations. Desktop qualification remains separate from
implementation and from successful cloud builds.

The finish line is one complete core, qualified in two environments: Debian/KDE
Plasma on native Wayland and Windows Server 2022 Desktop Experience in the
normal user's interactive RDP session. Physical Windows hardware, multiple
physical monitors, X11 and macOS are not part of this qualification claim.

## Product and ownership

The application consists of local HTML, CSS/assets and Python application logic.
The Rust executable owns the native application lifecycle, windows, input, IME,
plain-text clipboard, native services, accessibility, layout and CPU painting.
Python runs in a supervised child process and communicates through bounded,
authenticated local JSON-RPC. Other language clients can implement that same
protocol; this delivery supplies and tests Python.

The GUI thread owns each mutable document. Resource and native-service workers
return bounded completions to that owner. They never mutate the DOM from a
worker, wait for Python while holding GUI control, or retain a valid target
merely because an HTML ID is reused. A native window has at most one document.
Host document identities, renderer node generations, and native request epochs
protect replacement, deletion and asynchronous clipboard/picker/drop races.

The five workspace packages remain internal boundaries:

| Package | Responsibility |
| --- | --- |
| `ui4me-host` | Event loop, supervision, lifecycle, private protocol, OS adapters and presentation |
| `ui4me-renderer` | Hosted document API, style/layout integration, editing, semantic tree and painting |
| `ui4me-document` | Retained DOM, parsing, control state, validation and structural invariants |
| `ui4me-assets` | Bounded local loading, decoding, fonts and in-memory bundles |
| `ui4me-engine-types` | Shared status, identity and contract types |

The architecture in `SPEC.md` and lifecycle in `HOST_0.1_SPEC.md` remain the
foundation. This document and `CORE_1.0_PROTOCOL.md` define the completion
changes. Earlier version reports are historical evidence only.

## Completion scope

The machine-readable `CAPABILITY_SCOPE.json` and readable `CAPABILITY_SCOPE.md`
give every one of the 125 audit rows a disposition. They preserve the audit's
original recommendation; a recommendation in that spreadsheet is not itself a
promise to implement every possible extension. The selected contract is finite.
There is no mandatory chain of later feature releases hidden in the ledger.

### Application API and editing

The document protocol now exposes anonymous node queries and atomic mutations,
focused-element inspection, node-aware focus/blur and viewport hit testing.
Opaque node handles do not require adding synthetic HTML IDs. Existing ID-based
operations remain convenient for ordinary application code.

Text controls support UTF-16 selection/caret queries, setting selection,
replacement and editing commands through the existing editor. Replacement is a
single undo transaction and respects disabled/read-only state and registered
`beforeinput` cancellation. Unicode grapheme and word navigation uses
`unicode-segmentation`; the protocol's UTF-16 offsets remain distinct from
Python character indices. Clipboard editing uses the existing native deferred
path, with stale-context rejection when focus/document state changes.

Form reset, request-submit and custom validity use the existing control model.
Form collection returns ordered entries, preserving repeated names, successful
controls, selected options, submitter identity and file metadata. Reset clears
file state and editing history for the associated controls. File input painting
shows the selected filename or count. Native file pickers still own file choice.

Application notifications expose useful editing, selection, mouse-boundary,
context-menu, double-click and auxiliary-click behavior. Notifications arrive
after native dispatch. Cancellation is a pre-registered rule; Python callbacks
cannot retroactively cancel an edit or block the GUI thread waiting for a reply.

### Desktop integration

Window minimum/maximum dimensions are explicit paired logical constraints.
Changing or clearing a constraint is validated before applying it. Queries
separate requested configuration from observed state; unknown OS information is
`null`, not a fabricated `false`. Attention requests acknowledge submission,
with compositor policy and unsupported cancellation reported explicitly.

Desktop preference observation reports color scheme, reduced motion and high
contrast with source, availability and revision. Documents follow observed
color/motion preferences unless an app override is selected. Unknown native
settings remain unknown. Observing high contrast does not claim a complete CSS
forced-colors palette implementation.

External open accepts an optional parent. Reveal-in-file-manager uses the native
service and reports when it cannot forward a parent. A successful request is
not proof that another application became visible. External applications remain
outside UI4ME's supervised backend process tree.

Native IME receives available field-purpose hints while keeping the existing
composition and surrounding-text behavior. The current Windows windowing
backend does not apply purpose hints; the capability contract says so. This
does not disable ordinary or password text composition on Windows.

Incoming drops are opt-in through the nearest ancestor's
`data-ui4me-drop="files text"`, either individual token, or `none`. Disabled and
inert targets reject drops. Native feedback follows the accepted type. The
target node generation is captured when dropping and revalidated after async
transfer, so deleting or replacing it cannot redirect a drop to another node.
This is a bounded desktop drop contract, not browser `DataTransfer` or outgoing
native dragging.

### Accessibility

The semantic projection includes busy/atomic live-region metadata, table/grid
and tree relationships, safe reference relationships and assistive
ScrollIntoView. Text-run scroll actions reveal the requested content without
changing the selection. Linux AT-SPI application identity uses the manifest
name; active descendant changes are sent for combobox selections.

The previous Windows out-of-process UIA selection stall is an **open native
qualification blocker**, not an assumed upstream fix. The corrected diagnostic
has independent Select, GetSelection and range-GetText operations, explicit
MTA/STA apartments, flushed markers and a per-process watchdog. Its supplied
code is complete. The local Windows stage must reproduce, isolate and fix the
actual defect or disprove the original result with controlled evidence. NVDA
speech alone does not qualify those separate API operations.

### Rendering and resources

The selected CSS additions are intrinsic flex bases, balanced flex wrapping,
content-sized selects, font-face feature/variation defaults tied to the actual
selected font, legacy `word-break:break-word`, logical resize direction and a
visible resize grip, and blurred inset shadows with elliptical/nonuniform radii.
PNG-backed bitmap glyph support is enabled. Existing static raster/SVG, complex
shaping, variable fonts, flex/grid/table layout and supported effects remain.

The profile is `ui4me-desktop-1.0`. Standard `line-clamp` and `anchor-size()` are
reported as excluded where they cannot be implemented truthfully; the existing
documented legacy clamp path is retained. Implicit `@scope` roots and font-face
metric/size overrides are excluded. Explicit scope roots and ordinary font sizing
remain. A parser accepting syntax does not constitute implemented rendering.

Glyph hinting is opt-in with `UI4ME_GLYPH_HINTING=1`. The default is unchanged.
Higher precision rasterization is a diagnostic build option:

```sh
cargo build --locked -p ui4me-host --features diagnostic-raster-quality
UI4ME_RASTER_QUALITY=high target/debug/ui4me-host render --help
```

The equivalent environment variable can be set in PowerShell before invoking
the built executable. A request for high quality without the compiled feature
returns an explicit unsupported error. This option is not a GPU backend or an
idle-performance promise. Normal applications use the default CPU path.

Each document can register immutable resources under `ui4me-res://name/path`.
Registration replaces a bundle atomically; live references are invalidated and
reloaded through the existing resource machinery. Cache statistics distinguish
decoded cache use from registered source bytes. Purge removes unused decoded
entries only; it cannot silently revoke registered source or live ownership.

Limits are enforced independently: 2 MiB encoded RPC frames; 1 MiB decoded
resource bytes and 256 entries per registration request; 16 MiB aggregate
registered source bytes, at most 64 bundle names, and bounded bundle entry/path
counts per document. A replacement discounts its old allocation only after
validation; repeatedly registering small bundles cannot evade the aggregate
source budget. Existing document, asset decode, queue and cache limits remain.

## Explicit boundary

Core 1.0 does not implement a browser, JavaScript, remote page loading, HTTP
asset fetching, rich contenteditable, printing/PDF, audio/video, a GPU renderer,
full SVG text/cascade/animation, animated raster images, advanced display color
management, native menus/tray/notifications/global shortcuts, full touch/pen
gestures or custom native popup hierarchies. Components, routing, themes,
virtualized application views and business logic belong above the core.

Normal app development can now use the fixed protocol and profile. A new core
feature requires a concrete application need and an explicit new decision; it
is not required to call the selected 1.0 contract complete.

## Qualification and completion

The local procedure is **Linux/Python qualification → commit and push → Windows
qualification → commit and push → final regression of the resulting common
source → final commit and push**. This order concentrates debugging without
deferring the Windows implementation. Windows fixes to shared code require
targeted Linux regression before closing that stage.

Retain N01–N15 and R01–R12, add C01–C12, and run the complete automated verifier
at integration checkpoints. New code has unit/contract checks and a real-IPC
completion fixture in the reference app. A final installed release build must
be identified by binary hash and source identity in each native ledger.
Unexecuted or environment-blocked tests remain BLOCKED. Candidate builds become
1.0.0 only after actual native release gates pass; final version changes must be
built and checked too. `LOCAL_FINISH_PROMPT.md` is the complete autonomous runbook.
