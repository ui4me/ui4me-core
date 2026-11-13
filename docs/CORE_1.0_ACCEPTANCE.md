# Core 1.0 acceptance and release evidence

This is the acceptance contract for the completion candidate described in
[CORE_1.0_SPEC.md](CORE_1.0_SPEC.md). The supplied implementation is not a native
qualification result. Historical 0.1 and 0.2 reports stay historical; in
particular, the 0.2 Windows R05 failure is not closed by installing this source (it was
investigated and closed in the 1.0 Windows qualification; see [STATUS](STATUS.md)).

The supported qualification targets are Debian/KDE Plasma on native Wayland
and Windows Server 2022 Desktop Experience in the dedicated normal-user RDP
session. Record the actual OS, desktop, architecture, account, session, display
scale and input method used. An SSH process in Windows session 0 is not an
interactive desktop test. Physical Windows hardware, physical multiple
monitors, X11 and macOS remain outside this claim unless separately tested.

## Order and finish line

Follow `LOCAL_FINISH_PROMPT.md`: finish Linux/Python first, commit and push that
verified checkpoint, then qualify Windows, commit and push that checkpoint,
then run the final regression against the resulting common source. The Windows
implementation is already supplied; this order separates debugging and VM load,
not implementation scope. Shared fixes found on Windows require relevant Linux
regression before closing that stage.

Keep all **N01–N15** host checks from `tools/native_acceptance.py`, all
**R01–R12** renderer checks in [RENDERER_ACCEPTANCE.md](RENDERER_ACCEPTANCE.md),
and all **C01–C12** completion checks below. The older renderer document's
procedures remain useful, but use new Core 1.0 evidence directories and the
current installed release executable. Explicit platform limits in the Core 1.0
contract are expected behavior, not invented successful OS operations.

A release requires all applicable recorded checks, the full automated verifier,
lint, native fault fixtures, installed launch, browser comparisons and
measurements. Screen-reader speech and independent accessibility actions are
separate requirements. A candidate becomes `1.0.0` only after those gates pass;
the version change itself changes the source and executable identity and must
be built, installed and checked before the final report.

## Evidence and reproducibility

Use separate directories for each source milestone and OS, for example
`verification/core-1.0-linux-01/` and `verification/core-1.0-windows-01/`.
The acceptance tools bind a ledger to `source_sha256`, which hashes executable
source inputs, lockfiles, tools and assets. A changed source tree needs a fresh
ledger; do not edit old ledgers to make them match. Reports and root `docs/`
files do not change that hash.

Run final checks with the **absolute installed executable path**, not a
discovered debug executable. Retain its SHA-256, size, `--version`, source hash,
build/install command, lockfile identity and the local commit. Retain these for
both OS builds. Record the actual binary/source identity in supporting evidence
as well as the ledger, so a screenshot cannot accidentally qualify a different
running build.

Example Linux commands, after installation:

```sh
python3 tools/verify.py --jobs 2 --output verification/core-1.0-linux-01/automated
cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings
python3 tools/native_acceptance.py --automated --host "$HOME/.cargo/bin/ui4me-host" --output verification/core-1.0-linux-01/host
python3 tools/renderer_acceptance.py --automated --host "$HOME/.cargo/bin/ui4me-host" --output verification/core-1.0-linux-01/renderer
python3 tools/completion_acceptance.py --automated --host "$HOME/.cargo/bin/ui4me-host" --output verification/core-1.0-linux-01/completion
python3 tools/browser_reference.py --host "$HOME/.cargo/bin/ui4me-host" --output verification/core-1.0-linux-01/browser
```

Use `python` and the installed executable under the test user's profile on
Windows, with separate output directories. The desktop acceptance commands
deliberately remain BLOCKED until the required observations have been recorded;
their automated subset alone does not finish a ledger. The completion smoke
fixture exercises real Python/host IPC through `completion_checks.py`; it does
not substitute for C01–C12 desktop observations.

Record each observation using a real evidence file:

```sh
python3 tools/completion_acceptance.py --output verification/core-1.0-linux-01/completion \
  --record C01 --status PASS --evidence verification/core-1.0-linux-01/c01-observation.json \
  --note "Native resize limits, unknown state fields and advisory attention observed"
```

The evidence file must contain actions, expected and observed outcomes,
environment/build identity, and relevant screenshot, native tree, RPC or
process-counter references. A PASS record requires an actual interactive
session. Record failures as FAIL and unexecuted/environment-blocked work as
BLOCKED. An expected `unsupported` error can satisfy a documented boundary
check; a hung operation, missing observation or unexpected error cannot.

After the last source or version change, rerun the full automated gates on both
targets, the installed native smoke/fault checks, all affected scenarios and a
final cross-platform integration pass. Only claim final qualification for the
tested final identities. Preserve previous failure evidence and explain the
fix or diagnostic conclusion.

## Completion scenarios

### C01 — Window constraints, observed state and attention

Create a normal, resizable window with paired minimum and maximum dimensions.
Resize it interactively beyond each limit, then change both limits and repeat.
Clear each pair using explicit nulls and resize beyond the former bound. Send
an incomplete pair, zero/out-of-range size, and a minimum greater than its
maximum; each must fail before altering the previous window configuration.
Native decorations or the OS may impose additional minimum sizes; record them.

Compare `window.get.observed` with actual focus, maximize, fullscreen and
restore actions. On Wayland, minimized state, visibility and global placement
can be unknown and must be null. Requested values must remain separately
identified. On native Linux, creating a hidden window, changing top-level
visibility, forcing focus, setting position, and un-minimizing through the
unsupported pinned backend must return `unsupported`; they must not report
that an ignored request happened. Windows exercises those supported operations.

Request informational and critical attention while another app is focused.
The result acknowledges the request and leaves `presented` unknown; the window
manager chooses the visual effect. A Wayland compositor without xdg-activation
must return unavailable/unsupported, and canceling urgency on Wayland is
unsupported. Windows cancellation is exercised too. Record native observations
without demanding focus stealing or a specific compositor animation.

### C02 — Native preferences and override precedence

Capture `desktop.preferences`, document `environment`, and
`desktop.preferences_changed` notifications. With no app override, change the
desktop color scheme and reduced-motion setting through the native settings
UI, where the platform exposes them. The document should update its media
behavior without recreation. Change high contrast and verify its observed
value/source; high-contrast observation does not promise a complete CSS
forced-colors palette.

Select an explicit app color/motion override, change the OS preference again,
and verify the app override still wins. Return each override to `system` and
verify the current observation applies. Missing portal keys or unavailable
settings remain null and use a documented document default; never label a
default as an OS observation. Record that limitation if the native service
cannot expose a particular preference. Restarting the portal/session service
must not leave permanently stale preference state. Restore the original user
settings after the scenario. Ordinary idle time must not poll a healthy portal.

### C03 — Anonymous nodes, atomic updates and retired identities

Run the real-IPC completion fixture, then interact with an app view that contains
elements without HTML IDs. Query those nodes, inspect and mutate them, and
verify the visible result. Submit a batch with an initially valid update and a
later invalid node; the whole batch must roll back. Remove a target, create a
different element in its place, and reuse the old handle: it must fail without
modifying the replacement. Repeat across document replacement and unload.

Exercise an event callback that performs another host RPC and one that removes
its own target. There must be no deadlock, duplicate delivery to a replacement
element, or stale Python facade that remains writable. Retain the IPC report
and visible before/after evidence.

### C04 — Text selections, editing history and real IME

Use text including an accented character, a supplementary emoji and a combining
sequence. Read/set forward and backward selections, replace the selected range,
then undo and redo while checking value, selection and caret. UTF-16 offsets
that split a surrogate pair must be rejected atomically. Normal keyboard
navigation/deletion must preserve the declared grapheme behavior.

Use the actual configured IME on each desktop: preedit, candidate positioning,
commit, cancel, surrounding-text deletion and focus changes between fields.
Repeat while selecting text and in password/read-only fields. A preedit must
not become committed merely because focus changes. No IME event storm, mutex
freeze or backend loss is acceptable. A paste of a finished Chinese string is
not evidence of composition. On Wayland, check the declared purpose/hints where
the input method exposes them. Windows purpose hints are explicitly unavailable
in the pinned backend; ordinary composition remains required. Preserve R03 and
N06's real clipboard and keyboard-layout checks.

### C05 — Forms and file-control presentation

Exercise keyboard and pointer submission, a specified submitter, reset, custom
validity and canceled default actions. Verify ordered form entries preserve
duplicate names, omit disabled/non-successful controls, and retain submitter
identity. Invalid/custom-invalid controls must prevent the declared submission;
clearing validity must allow it. Reset restores default values/selections and
does not leave an undo transaction capable of reviving pre-reset values.

Choose one and multiple real files, cancel the picker, and reset the form.
Check that visible file-control labels show selected names and return to their
empty state on reset. Check Unicode names and path/value reporting separately
from the visible label. Retain native picker lifetime/cancellation checks from
N05 and R08.

### C06 — Incoming native drops and target retirement

Use nested regions with `data-ui4me-drop="files"`, `"text"`, `"files text"`
and `"none"`, plus disabled and inert regions. Drag actual local files from the
file manager and text from another native app. The nearest eligible explicit
policy chooses the receiver and data type; `none`, disabled and inert reject.
Regions without an opt-in reject document drops. The diagnostic probe retains
its independent host test behavior.

Observe the OS Copy/reject cursor and `drag_feedback` transitions. The host
reports target/acceptance transitions instead of flooding Python on every
pointer movement. Leaving or dropping clears feedback. On an offer containing
both URI and text types, a text-only receiver must choose text, not an unwanted
file transfer. Capture the original `node` and document identity in the final
drop event.

Delay a transfer, then remove/replace its receiver or change its policy before
completion. It must be discarded, never hit-tested again onto a replacement.
Exercise document replacement/window close during transfer and transfer-size
limits: at most 1 MiB text/URI payload and 1,024 local Unicode file paths.
Unparseable/non-local URI entries fail explicitly. No general browser
DataTransfer object or outgoing native dragging is claimed.

### C07 — Pointer, context, editing and selection events

With real pointer and keyboard input, record context-menu, auxiliary-click,
double-click and declared mouse enter/leave events on nested elements. Compare
target/current-target, bubbling order and related-node identities with the
document contract. Remove an event target during a callback and confirm no
callback is redirected to a reused ID.

Exercise beforeinput/input, selection and text-change events with allowed and
canceled edits, including paste and IME commit. Cancellation must prevent the
documented default action without reporting a mutation that did not occur.
Asynchronous Python callbacks do not retroactively cancel an already completed
native default; use the declared synchronous policy mechanism. Confirm normal
typing and scrolling remain responsive while Python is temporarily blocked.

### C08 — Accessibility semantics, value speech and assistive actions

Inspect actual AT-SPI/UIA trees for table/grid row and cell relationships, tree
levels/selection, labels, busy/atomic live-region metadata and bounds. Trigger
an assistive ScrollIntoView on off-screen content and a text run. The expected
content becomes visible and text selection does not change as a side effect.
Remove/recreate a document and close windows; stale objects must not alias new
nodes. Retain native focus and action evidence from N07/R05.

On Linux, the AT-SPI application name must equal the manifest's display name,
not `ui4me-host`. Focus the collapsed combobox, read its current selection with
Orca, change it by keyboard and through the native selection interface, and
read it again. Capture the selected child's accessible label and the
active-descendant/selection events. The control label and selected value must
remain distinct. A semantic JSON value or an adapter unit test alone does not
qualify Orca speech.

On Windows, complete the independent R05 matrix below in addition to the NVDA
journey. An NVDA success cannot substitute for a stalled UIA Select/GetSelection
operation. A successful native operation must also produce the expected
selection/value in the actual control.

### C09 — Runtime resources and ownership

Display a resource from a registered `ui4me-res://` bundle, replace the same
bundle with different content, and verify the visible asset refreshes. Replace
it with an invalid traversal/over-budget registration: the whole old bundle
must remain valid. Exercise the per-request and aggregate source-byte/entry
limits from [CORE_1.0_SPEC.md](CORE_1.0_SPEC.md), including repeated small
registrations that must not evade the aggregate budget.

Purge unused decoded entries while a displayed asset is still referenced.
Source ownership and live decoded entries must survive. Remove those references,
purge again, unregister the bundle and verify stats distinguish registered
source bytes from decoded cache bytes. Decode failures must be visible as
diagnostics and preserve bounded work, not crash the host or evict unrelated
working resources.

### C10 — Declared rendering profile

Use focused fixtures for intrinsic flex bases, balanced wrapping, content-sized
selects, `word-break:break-word`, logical-direction resize grips, inset shadows
with nonuniform/elliptical radii, and actual selected-font feature/variation
defaults. Resize at multiple viewport widths and compare declared standard
behavior with the browser reference on the same fonts/assets. Record both
geometry and pixels; font rasterization differences require explanation rather
than arbitrary pixel-perfect claims.

Check a PNG-backed bitmap glyph using an installed/fixture font that actually
contains that format; an unrelated monochrome outline glyph is not evidence.
Keep the default raster path and diagnostic high-precision/hinting opt-ins
distinct. Excluded standard `line-clamp`, `anchor-size()`, implicit scope roots
and font metric/size overrides must remain excluded in the profile; accepting
their syntax is not a PASS for rendering them. Run every retained R12 browser
fixture as well as focused evidence for the additions.

### C11 — Parented external open and reveal

Open an HTTPS target and a local Unicode file through the native service with
an optional live parent. When a chooser appears, observe its relation to the
specified window; `parent_forwarded:true` records sending the parent identifier,
not a guarantee that a desktop service honors it. Cancel a Linux portal chooser,
close its parent, and quit while it is pending: cancellation must complete
without retaining an orphaned chooser or blocking the GUI. A missing service
must return an explicit unavailable error.

Open a local directory and confirm it opens the directory itself. Reveal a
file and a directory, including spaces/non-ASCII names and a symlink where
supported, and confirm the containing folder/item selection. Reveal uses
FileManager1.ShowItems on Linux and Shell item selection on Windows; neither
API accepts a parent, so `parent_forwarded:false` is expected. No shell command
string is constructed. Unsupported URI schemes must be denied.

Confirm the external viewer/file manager is outside UI4ME's supervised backend
tree and survives normal and forced host exit, as required by N11. A successful
request has `presented:null`; record the actual external UI separately.

### C12 — Static idle, cost and sustained workload

Launch the installed reference app normally, without test flags, animation-demo
classes or an application/system reduced-motion override added for measurement.
After resources settle, record host and backend CPU counters for at least
60 seconds, visible-window responsiveness and any redraw activity. This ordinary
static reference must not continuously repaint. An occasional OS notification
or input caret blink must be distinguished from a sustained render loop.

Measure startup, first frame/document readiness, idle memory, repeated document
mutations and window create/close cycles using the same release builds and
workloads as the recorded comparison. The resource helper now follows the
actual system preference by default and records the selected policy:

```sh
python3 tools/measure_resources.py --renderer --reduced-motion system --host "$HOME/.cargo/bin/ui4me-host" \
  --startup-runs 5 --idle 60 --cycles 400 --sample-every 50 --dom-cycles 400 \
  --output verification/core-1.0-linux-01/renderer-measurements.json
```

For a controlled comparison to historical runs that explicitly disabled
animation, use `--reduced-motion reduce` in a separate evidence file. The
`no-preference` override is also explicit. Label these scopes; an override
cannot replace the ordinary static-idle check above. Test an intentionally
enabled animation separately, then disable it and verify redraw cost returns to idle.
Repeat sustained editing, clipboard, IME and native opening with no input stall
or backend loss.

## Windows R05: isolate each UIA operation

Use `tools/windows/uia_probe_matrix.py`, `uia_text_probe.ps1` and
`UiaTextProbe.cs` in the test user's active RDP session. They create every UIA
object on one explicitly chosen client thread, log before/after markers, run
each mode in a fresh process and enforce an external process deadline. The five
modes independently test obtaining TextPattern, reading DocumentRange text,
selecting a range, getting the selection, and reading selection text.
The selection-read branch does not first call Select or DocumentRange. The
default matrix reads the selection before Select, which changes target state.
Restore the intended target selection between independent reproductions; a
fresh client process does not reset the application. Use an individual `--mode`
when preparing the target separately for each operation.

Example required MTA run, using the actual installed reference host PID:

```powershell
python tools/windows/uia_probe_matrix.py --pid 1234 --title "Desktop workspace" --automation-id name --apartment MTA --timeout 15 --output verification/core-1.0-windows-01/uia-reference-mta
```

Replace `1234` with the observed PID; do not use a guessed PID. Capture a
separate focused run with `--focus`, and repeat without that flag after moving
focus elsewhere. Run `--apartment STA` in a separate directory for diagnosis.
MTA is the required automation client. A diagnostic STA failure is recorded
with its control comparison, not hidden by an aggregate result and not used to
waive an MTA failure.

Run the same matrix against:

1. The renderer reference Name field, including a nonempty selection and an
   empty/caret selection. Restore the intended text/selection between cases.
2. The installed host's diagnostic probe (`ui4me-host probe`) as a simpler
   AccessKit control. Discover its actual title/PID; use `--first-edit`
   to select its first Edit control where appropriate.
3. A native editor control that actually supports TextPattern, discovered by
   PID/title and automation ID or `--control-name`. Record its identity; an
   unsupported control is not a passing comparison.

Repeat with NVDA stopped and running, and preserve focused/unfocused state.
Record which operations return, their last markers, range counts/text lengths,
and the actual control selection after Select. The independent selection read
must agree with that selection. The NVDA journey remains required for speech,
focus and the live status announcement.

If a call times out, retain its client process log and the corresponding host
and client stacks before changing code. Identify whether the stop is in client
marshaling, platform transport, provider code or the host action/update seam.
Do not patch AccessKit just because it is on the dependency list. The supplied
0.2 stack capture shows a responsive/rendering host and does not establish a
provider lock deadlock. A provider fix requires an isolated failing operation,
a reproduced cause and a focused regression. If MTA Select/GetSelection still
hangs, Windows R05 and final Windows qualification remain FAIL regardless of
NVDA success. No timeout-to-success conversion or shortened test is allowed.

## Measurement comparisons and known retention

The earlier results in [BASELINE_0.2_STATUS.md](BASELINE_0.2_STATUS.md) are
comparison data, not universal budgets: Linux had about 0.23% host idle CPU,
60.1 MB host-plus-backend memory and 527 ms median document readiness; the
tested Windows session reported 40.9 MB and 133 ms, with idle CPU below its
displayed precision. Keep Linux PSS and Windows private-byte metrics distinct.
Record CPU normalization, sample duration, VM load, fonts, scale, accessibility
state and warm/cold startup conditions before interpreting differences.

Use at least five startup samples and repeated idle/memory runs on comparable
conditions. Establish a comparison envelope before accepting a regression.
Investigate increases greater than both normal run-to-run noise and roughly
25% of a comparable baseline; use practical floors such as 50 ms for startup
and 10 MB for total memory so tiny/noisy denominators do not decide the gate.
For static idle, a sustained added half percentage point of one CPU core, or
CPU above 1% without explained activity, requires investigation even when
startup is acceptable. These are investigation thresholds, not permission to
hide smaller reproducible render loops or input stalls. Explain any accepted
cost with measurements and the implemented feature responsible for it.

Repeated document changes should settle without an unexplained upward trend.
Measure window-cycle slopes after warmup, with accessibility enabled and
disabled when attribution is needed. The supplied 0.2 Linux report documented
about 19 KiB retained per closed document window while the accessibility bus was
on, attributed to the existing zbus object-path retention; the exact slope
depends on accessible object count. The qualified teardown patch remains.
Do not claim this residual retention is fixed or demand a fabricated zero.
Compare identical fixtures/cycle counts, record the known slope separately,
and investigate new growth beyond it. Windows should retain its previously
flat settled behavior under the comparable workload.

Finish with a concise evidence index containing all ledger paths, final source
and binary hashes, actual target/session identities, automated and native
results, measured costs, fixed defects and remaining explicit limits. A
BLOCKED/FAIL requirement must stay visible in the final status. Once the fixed
contract passes, ordinary app development can proceed without adding another
mandatory feature phase to this release.
