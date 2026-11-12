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

## Version 0.2 merge of renderer accessibility behavior

The 0.2 tree preserves the qualified `adapter.rs` teardown patch above and also
carries the donor renderer's `src/node.rs` mapping of AccessKit expanded/invalid
states to AT-SPI. The carried change is recorded in
`accesskit_atspi_common-0.21.1/renderer-state-mapping.patch`. These patches solve
different issues and must not overwrite one another when updating upstream.

The 0.1 measured results above are historical, not a 0.2 rerun. The complete
0.1 status report remains in `docs/BASELINE_0.1_STATUS.md`. Further source patches
needed by the renderer are documented separately in `patches/README.md`.
