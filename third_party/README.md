# Vendored dependency patches

These packages are used through `[patch.crates-io]` in the workspace manifest at
their exact pinned versions. Compare each directory with its first commit
("Vendor pristine ...") to see the complete patch.

## winit-wayland 0.31.0-beta.4

`winit-wayland-0.31.0-beta.4/` is the exact crates.io package pinned by
`Cargo.lock` (package SHA-256
`8352b11bc0d223a37bbbdb55d2250949917c313f3ee07a34da7d614d74fce69e`), used through
`[patch.crates-io]` in the workspace manifest. Its only change is in
`src/data_transfer.rs`: the MIME table also contains the conventional
`text/plain;charset=utf-8` and `text/html;charset=utf-8` spellings, listed
first so they are offered first.

Reason, observed on KDE Plasma 6.3 Wayland (2026-11-12): upstream only knows
`text/plain; charset=utf-8` (with a space). GTK 3 maps text requests to the
canonical spelling, so a GTK application could not paste text copied from the
host, and text copied from GTK reached the host through GTK's ASCII-escaped
`text/plain` offer (`→` arrived as `→`). Qt/Klipper and wl-clipboard were
unaffected. No newer winit-wayland release existed on that date and the crate
has no public API to add MIME types.

Drop this patch when an upstream release offers the canonical type; keep the
version pin otherwise identical. The package is licensed Apache-2.0 (LICENSE).

Second change (Core 1.0 Linux qualification, 2026-11-13), in `src/seat/mod.rs`:
upstream created the `wl_data_device` and primary-selection device only when the
seat gained *pointer* capability and destroyed them when the pointer went away.
On a keyboard-only Plasma Wayland seat (no mouse attached) every clipboard write
therefore failed with "Tried to set the clipboard, but no seat has received
input", even immediately after a real key press, because `set_clipboard` needs
both a data device and the latest input serial. The patched seat handler creates
both selection devices when either keyboard or pointer capability appears and
drops them only once neither remains. Reproduced and verified with the installed
release host's native clipboard round trip (fails before, passes after) on a
seat with only a keyboard; pointer-equipped seats keep the same devices.

## softbuffer 0.4.8

`softbuffer-0.4.8/` is the exact crates.io package pinned by `Cargo.lock`
(package SHA-256
`aac18da81ebbf05109ab275b157c22a653bb3c12cf884450179942f81bcbf6c3`). Its only
change is in `src/backends/wayland/mod.rs`.

Upstream `buffer_mut()` keeps two buffers and, when the back buffer has not
been released, waits in `blocking_dispatch()` until the compositor releases it.
On KDE Plasma 6.3 a fully occluded or minimized window keeps receiving frame
callbacks without its newest buffer being latched, so that wait never ended:
the host's GUI thread froze, including input, IPC replies and the Ctrl+Shift+Q
emergency quit (reproduced deterministically with a covered second window
receiving text updates on 2026-11-12).

The patched backend processes releases that have already arrived, and if the
back buffer is still held it swaps in a new buffer and parks the busy one until
its release (at most four parked buffers per surface). If that bound is reached
it returns the error text "Wayland compositor still holds every buffer"
instead of blocking; ui4me-host then skips that frame and requests another
frame-callback-paced redraw. The Windows backend is unchanged. Licensed
MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).

## accesskit_atspi_common 0.21.1

`accesskit_atspi_common-0.21.1/` is the exact crates.io package pinned by
`Cargo.lock` (package SHA-256
`24f5baf62e34a4a6c30e3d697e4782f821a7337049c44c3b40576a27dfbb580d`; license
MIT OR Apache-2.0 as declared in its Cargo.toml). Its qualified 0.1 change is in
`src/adapter.rs`: `Drop for Adapter` unregisters the AT-SPI interfaces of every
node that `register_tree()` and later updates registered, before announcing the
window's removal. A unit test (`dropping_adapter_unregisters_every_registered_interface`)
covers it and fails without the patch.

Upstream (also the newest accesskit_unix 0.24.1 on 2026-11-12) left every
destroyed window's D-Bus objects registered in zbus's object server. Measured on
KDE Plasma 6.3 with tools/measure_resources.py, host private memory grew about
12.7 KiB per window create/close cycle (800 cycles: 16.5 to 25.5 MiB); with the
patch about 3.8 KiB per cycle. The remainder is zbus 5.19 keeping the empty
intermediate object-path node `/org/a11y/atspi/accessible/<adapter>` and its child
map; zbus exposes no API to prune it, so it is recorded as a residual limit.

## winit-win32 0.31.0-beta.4

`winit-win32-0.31.0-beta.4/` is the exact crates.io package pinned by
`Cargo.lock` (package SHA-256
`cf3b2326760dad4a8fe721a8c8a40fd4f7efb338ef468f340794ea1efcd5261e`), added during
0.2 qualification. Its only change is in `src/window.rs`, `request_ime_update`.

Upstream updates the window state and then calls `ImmAssociateContextEx`
(through `ImeContext::set_ime_allowed`) while still holding the window-state
mutex. When a Text Services Framework composition is active, dissociating or
associating the input context makes TSF complete the composition synchronously
and send `WM_IME_COMPOSITION`/`WM_IME_ENDCOMPOSITION` to the same window
procedure, whose handlers lock that mutex: the GUI thread deadlocked
(`Mutex::lock_contended` under `CtfImmGenerateMessage`). Reproduced on Windows
Server 2022 with Microsoft Pinyin on 2026-11-12 by moving focus to another field
during a composition (mouse click, Tab or UI Automation `SetFocus`). The patch
copies what it needs, releases the mutex, and only then calls into IMM.

Drop this patch when an upstream release no longer holds the window state across
IMM calls. Licensed Apache-2.0 (LICENSE).

## Version 0.2 merge of renderer accessibility behavior

The 0.2 tree preserves the qualified `adapter.rs` teardown patch above and also
carries the donor renderer's `src/node.rs` mapping of AccessKit expanded/invalid
states to AT-SPI. The carried change is recorded in
`accesskit_atspi_common-0.21.1/renderer-state-mapping.patch`. These patches solve
different issues and must not overwrite one another when updating upstream.

The 0.1 measured results above are historical, not a 0.2 rerun. The complete
0.1 status report remains in `docs/BASELINE_0.1_STATUS.md`. Further source patches
needed by the renderer are documented separately in `patches/README.md`.

## Core 1.0 AT-SPI application identity and active descendant

`accesskit_atspi_common-0.21.1/core-1.0-identity-selection.patch` contains only
the changes made after the supplied, locally qualified 0.2 tree. It is a diff
against that tree, not against pristine upstream. The package version, upstream
checksum and licenses above remain unchanged. No 1.0 changes were made to
`adapter.rs`; its qualified teardown patch is retained byte for byte.

The new `set_application_name` function in `context.rs`, exported by `lib.rs`,
sets the process's accessible display name once, before the first adapter is
created. `AppContext::new` uses it in preference to the executable name. The
host supplies `app.name` from its manifest; a later attempt to change that
process identity is an error. This avoids exposing every application as
`ui4me-host` to AT-SPI clients. Replace this patch with an upstream application
identity API when one is available.

The `node.rs` addition emits the existing AT-SPI `ActiveDescendantChanged`
event when an AccessKit container's active descendant changes. The renderer
now supplies the selected option as a combobox's active descendant while
retaining separate accessible label and value. The adapter already exposes
selected options through its Selection interface; it must also report this
semantic navigation change. The host test
`atspi_combo_exposes_selected_option_and_active_descendant_changes` exercises
both the native adapter's selected-child/name projection and the new event
without a running desktop bus.

These are new source changes, not a claim of a completed Orca journey. Current
Linux screen-reader qualification must check initial focus, keyboard selection
changes, app naming and teardown. The independent Windows UI Automation R05
text-selection hang is a separate blocker; no speculative Windows provider
patch is included here.

## Core 1.0 AT-SPI table, tree, live-region and relationship metadata

`accesskit_atspi_common-0.21.1/core-1.0-atspi-metadata.patch` is a diff against
the tree after `core-1.0-identity-selection.patch` (added during Core 1.0 KDE
qualification, 2026-11-13). The renderer already gives AccessKit table/grid
row and column indices, counts and spans, tree levels, live/atomic/busy state and
explicit labelled-by/described-by/details/flow-to/error-message relationships
(DOC-29/DOC-30), but the pinned AT-SPI adapter published only `posinset`,
`setsize`, `id` and `controller-for`: an inspection of the installed host on
KDE showed table cells without indices, tree items without levels, live regions
without live/atomic attributes and controls without label relations.

The patched `attributes()` adds 1-based `rowindex`/`colindex`, `rowcount`/`colcount`,
spans above one, `level`, `live`/`atomic`/`busy` on the region and
`container-live`/`container-atomic`/`container-busy` on its descendants, the
attribute names browsers use on AT-SPI. `relation_set()` adds `LabelledBy`,
`DescribedBy`, `Details`, `FlowsTo` and `ErrorMessage`, resolved in the node's
tree and filtered like the existing `ControllerFor`. The host test
`atspi_publishes_table_tree_live_metadata_and_relationships` fails without it.

Limit: `accesskit_unix` 0.24.1 implements no AT-SPI `Table`/`TableCell` D-Bus
interfaces, so table navigation commands of a screen reader that require those
interfaces are not available; cell indices are object attributes only.
