# Version 0.2 renderer acceptance

Version 0.1 was qualified in KDE Plasma 6.3 on native Wayland and Windows Server
2022 in the `ui4me_test` RDP desktop. Its recorded N01–N15 results belong to that
source revision. Version 0.2 attaches the document engine to the same host and
must pass the host regressions again, plus the renderer checks below. Existing
v0.1 reports are historical evidence; they are not v0.2 PASS records.

The targets for this release are **KDE native Wayland** and **Windows Server 2022
Desktop Experience through the dedicated normal-user RDP session**. A source
cross-check from Linux does not replace Windows execution. X11, physical Windows
hardware, physical multi-monitor hardware and macOS are outside the qualified
matrix unless separately exercised and explicitly recorded.

## Evidence rules

Start each integration milestone in new evidence directories. The native tools
bind a ledger to a hash of the executable source/lockfiles/assets and reject a
different source tree. Documentation and generated verification outputs do not
affect that hash. Preserve previous ledgers; a changed build needs new evidence
for the affected checks and a final integration pass.

The automated native run records the selected host executable's path, SHA-256
and size. Use `--host /absolute/path/to/installed/ui4me-host` for the final release
gate so a leftover debug binary is not selected by discovery. An executable hash
identifies what ran; the matching source build command and source identity must
still be retained to establish how it was built.

```sh
python3 tools/native_acceptance.py --automated --output verification/v02-final/host
python3 tools/renderer_acceptance.py --automated --output verification/v02-final/renderer
```

Run these in the actual target desktop. The automated part performs smoke checks;
it deliberately exits BLOCKED until all native observations are recorded. It
does not infer keyboard input, screen-reader speech or compositor presentation
from RPC success. Record an observation with an existing evidence file:

```sh
python3 tools/renderer_acceptance.py --output verification/v02-final/renderer \
  --record R03 --status PASS --evidence verification/v02-final/ime-observation.json \
  --note "Native preedit, commit, cancellation and clipboard exchange observed"
```

Use `python` on Windows. Each evidence file must contain the real command/build
identity, OS/session/user/display information, actions, expected and observed
results, and references to actual logs/screenshots/tree dumps where appropriate.
Record failures and blockers directly. Screenshots of a finished string alone do
not establish input-method composition. A running executable alone does not
establish a document's accessibility or asset behavior.

## Automated gates before desktop qualification

```sh
python3 tools/verify.py --jobs 2 --output verification/v02-automated
cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings
python3 tools/run_renderer.py --headless --evidence verification/v02-headless.json
```

The full verifier covers the whole Rust workspace, Python tests, the retained
probe, real document protocol checks, and process/fault fixtures. The renderer
fixture asserts queries/pagination, Unicode values, atomic updates and rollback,
fragment mutation, rejected-load preservation, serialized UTF-8 pagination, node
inspection, document overlays, stylesheets, nested RPC inside an ordered focus
callback, separate documents, stale-document rejection, readiness and shutdown.
It requires the complete expected backend report rather than trusting exit zero.

Render without a desktop:

```sh
ui4me-host render --html examples/python/ui/index.html --output verification/workspace.png \
  --width 1080 --height 760 --scale 1
```

This exercises the CPU engine and bounded local resources. It is a diagnostic
PNG path, not a printing/PDF feature or evidence of native window behavior.

## Required native scenarios

### R01 — Local document and visible rendering

Run `python3 tools/run_renderer.py` in the actual desktop. Observe the local Noto
Sans font, stylesheet, PNG landscape, SVG mark, flex/grid form layout, table,
rounded surfaces and CSS status animation. Resize below 800 CSS pixels and back
to exercise the responsive rule. Open `index.html` separately in an external
browser for an informal structural comparison; use R12 for controlled fixtures.
Record a full-window screenshot and any renderer diagnostics. Confirm that no
browser process, local HTTP server, remote font or network image is required.

Verify text is not the fixed v0.1 probe surface and that the default installed
application loads its assets independently of the repository working directory.
Record missing system fallback fonts explicitly; do not call placeholder boxes
successful CJK/emoji rendering. The bundled Latin font is not a complete CJK/emoji
font collection.

### R02 — Controls, Python and live document state

Edit Name, Notes, Priority, Estimate, Progress, checkbox and radio buttons. Add
several rows with accented text and HTML-sensitive text such as `<tag> & value`;
the new row must contain literal application text. Confirm the preview follows
the Name field and that one transaction updates the row/count/status together.
Switch palettes and exercise the second document. The native smoke fixture also
checks rollback, fragment removal, stylesheet replacement and document generation
rejection. Retain its backend report alongside the visible-input observation.

Confirm event callbacks do not arrive out of order during ordinary typing and
that their nested document calls complete. Slow application work may delay other
Python callbacks; native editing and event-loop control must continue.

### R03 — Actual input method, selection and clipboard

Use a real configured IME in both an `<input>` and `<textarea>`. Observe preedit,
candidate position, commit, cancellation, focus changes during composition,
selection replacement and surrounding-text/caret updates. Include accents,
non-Latin composition and emoji. Test at the initial scale and the altered scale
used for R06. Injecting a complete Unicode string over RDP is a separate test.

Copy and paste plain text in both directions with another native application.
On Linux include a GTK application, retaining the v0.1 regression. On Windows
include a local desktop application; record whether RDP clipboard redirection is
enabled and test that separately. Read immediately after writing, replace a
selection, undo/redo supported text edits and confirm actual resulting values.
Preserve and restore the desktop's pre-test clipboard and IME configuration.

### R04 — Local interaction and default actions

Navigate using Tab/Shift+Tab and activate buttons with the keyboard. Exercise
select/radio/check/range controls and disabled/read-only controls using the
engine's supplied fixtures where needed. Verify caret motion, selection, mouse
drag hit testing, scrolling over the nested table and scroll-into-view after
adding enough rows to overflow. Resize while interacting and confirm coordinate
mapping remains correct. Engine pointer-capture regressions and native drag
observations together cover capture/release and element removal.

Submit the form with the keyboard. Preregistered submit prevention must keep the
action local, without browser navigation or a late Python cancellation race.
Verify ordinary edits still work when Python is temporarily blocked in R09.

### R05 — Native accessibility and speech

Inspect the actual document tree through AT-SPI on Linux and UI Automation on
Windows. Confirm logical roles/names, labels, value/checked/selected state,
editable text, selection, bounds and focus. Invoke an action through the native
accessibility interface, then confirm the document and backend observe it.
Replace a document, close a second window and inspect for stale accessible nodes.

Complete a short journey with a real screen reader: find Name, read its label and
value, edit it, move to the checkbox, activate Add row, and read the changed
content/status where supported. Retain tree/action evidence and a record of
actual speech or the observed reader output. An exported semantic JSON tree is
useful debugging evidence but cannot qualify speech, native focus events or
cross-process accessibility transport. If an actual screen reader cannot run,
record that part BLOCKED instead of silently weakening this scenario.

### R06 — Window, scale and surface lifetime

Open the second document. Independently resize, maximize, minimize, restore,
focus and close the windows. Move/reopen the details document and confirm its
events do not target the main document. Change the supported desktop scale,
exercise pointer/IME/caret/accessibility geometry, then restore the original
display setting. Record physical vs RDP display configuration and scale values.
No physical multi-monitor claim can come from a single virtual display.

Repeat after several document replacements and resource loads. The renderer must
recreate pixel surfaces without losing logical document state, changing the
normal host lifecycle policy, or applying retired resources to a new document.

### R07 — Resource policy and bounded work

Run and inspect the asset/document/renderer regression tests for allowed roots,
path escapes, data URLs, blocked network resources, malformed image/font/SVG
inputs, payload bounds, cache limits, canceled/replaced loads and atomic mutation
failures. Retain the exact test command and outcomes; a generic build log is not
enough. The runtime uses the hosted profile, not the broader legacy donor profile.

In a temporary copy of the reference application's assets, make a local image or
stylesheet unavailable and verify a bounded visible/diagnostic failure without a
host hang. Restore the copy and reopen. Exercise rapid close/reopen while local
fonts/images are loading and confirm another window remains usable. Do not edit
the committed assets merely to create a native-test fixture; keep destructive
resource variants in the verification workspace.

### R08 — Native pickers and document overlays

Activate the Attachment file input and the explicit Choose a file button. For
each, test selection, cancellation and closing the parent/whole application while
the native picker is outstanding. The picker must be associated with the correct
native window when the OS supports it; no late selection may reach a replacement
document. Confirm a file input change returns selected Unicode local paths and
does not permit injecting files through a text setter.

Open About this workspace (HTML modal dialog), cycle focus, close with its button
and Escape, then open Quick help (popover) and dismiss it. Confirm top-layer
painting, hit testing, focus restoration and form validity/default behavior.
Keep the existing KDE folder-picker parenting limitation distinct from a new
renderer regression, and retain N05's open/save/folder cancellation checks.

### R09 — Covered/minimized updates, blocked callbacks and failure

Run the supplied live-update diagnostic:

```sh
python3 tools/run_renderer.py --updates 0.25
```

Fully cover the window for at least 30 seconds, then minimize it for at least 30
seconds. Observe continuing `UPDATE ... document command completed` logs and
responsive native control in the second window. Restore the main window and
confirm the latest state appears without replaying a large stale frame queue.
Repeat covered-window emergency quit with Ctrl+Shift+Q where the native focus
permits it. This specifically guards the v0.1 softbuffer freeze regression.

Then run:

```sh
python3 tools/run_renderer.py --hold-callback
python3 tools/run_renderer.py --crash-after-ready 3
```

In the first run click Add row to block its Python callback; native text editing,
caret, resize/repaint, document accessibility and the emergency quit path must
remain responsive. Moving a compositor-owned window alone is insufficient. In
another blocked-callback run, use the native window close control: the default
close policy runs on the independent request pool, orderly shutdown bypasses the
blocked notification, and the backend must exit without waiting for that callback.
Repeat native close while the explicit file-picker callback is awaiting its reply.
In
the second run observe the host's failure UI and working Quit action after the
backend exits. Also repeat the supplied host fault fixtures and managed process
tree forced-host-death scenario using production containment.

### R10 — Measurements and settled growth

Use the installed release binary under the normal desktop user:

```sh
python3 tools/measure_resources.py --renderer --host /absolute/path/ui4me-host \
  --startup-runs 5 --idle 30 --cycles 400 --dom-cycles 800 --sample-every 50 \
  --output verification/v02-final/renderer-measurements.json
```

The tool uses real loaded documents, waits for local resources, disables CSS
animation for idle sampling, samples after window cycles, and separately samples
insert/update/remove DOM churn. Raw logs are retained beside a requested report.
First native-window-frame time is identified separately from document readiness;
neither is silently relabeled as fully rendered content. On Linux compare PSS
with PSS; on Windows compare private bytes with private bytes.

Record executable size, host/backend memory, startup and idle CPU, document update
behavior and warm-cache trends. Check that repeated identical resources reuse
bounded caches and that transient DOM nodes are reclaimed. Investigate sustained
new linear growth; do not hide it in a single start/end total or discard warm-up
samples without explaining them. The historical Linux host accessibility leak
was about 3.3 KiB per closed window; distinguish that known upstream issue from
new renderer growth with attributable evidence. The v0.1 probe's size/memory/startup
numbers are a baseline, not an expected cost for a newly included CSS/text engine.

### R11 — Installed application outside the checkout

Install into a new permanent normal-user location:

```sh
python3 tools/install.py --example-dir ~/.local/share/ui4me-renderer-reference
python3 tools/create_launcher.py install --app ~/.local/share/ui4me-renderer-reference/app.toml
```

On Windows use a directory under the test user's `%LOCALAPPDATA%`. Keep a permanent
Python interpreter; the manifest pins its exact path. The default installation
copies HTML, CSS, all local assets, client, facade and application backend. Use
`--probe` in a different destination to retain the v0.1 host probe.

Launch from the actual desktop menu with no developer terminal, no checkout cwd
and no development environment variables. Check the app's name/icon/taskbar,
Unicode/spaced application paths, no extra Windows console, simultaneous launch
forwarding, all local assets, form actions and orderly shutdown. Leave the
qualified installation and launcher available after the tests. Never overwrite
an unrelated launcher or existing application directory.

### R12 — Controlled browser reference

In a separate QA Python environment, install only the optional oracle dependencies
and a local Chromium build:

```sh
python3 -m pip install -r tools/requirements-browser.txt
python3 -m playwright install chromium
python3 tools/browser_reference.py --host /absolute/path/ui4me-host \
  --output verification/v02-final/browser
```

`--chromium /path/to/chromium` reuses an existing compatible executable. This tool
does not ship Chromium/Node/npm, use a web server, or add JavaScript to the runtime.
It renders the same local fixtures through the host's CPU CLI and external
Chromium, pins the local font and viewport/scale, blocks HTTP(S), records versions
and source/binary hashes, and writes both PNGs plus an absolute difference PNG.

Required cases are B01 box geometry/gradient, B02 flex/grid, B03 local font/text,
and B04 basic static SVG. Thresholds are declared in `tools/browser-fixtures/cases.json`
before execution. Geometry thresholds permit sparse antialiased edge differences;
text/SVG thresholds allow bounded rasterizer differences. Review the images as
well as numbers. Do not raise a tolerance, remove a fixture, crop away a mismatch
or relabel an unsupported result merely to pass. Investigate a failure and fix the
supported implementation; explicitly document any remaining profile boundary.
The four fixtures do not establish arbitrary-browser-page compatibility.

## Release decision

The final report names the exact source commit and executable, both tested OS
sessions, all automated gates, N01–N15 and R01–R12, measured costs, and actual
remaining limitations. A mandatory unresolved defect or missing native scenario
means the release is not fully qualified. A useful development build can still be
installed and reported honestly, but never described as perfectly tested. Preserve
v0.1 evidence and write new v0.2 evidence under a distinct dated directory.
