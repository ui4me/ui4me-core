# Third-party components

The repository's original code is MIT-licensed. Upstream dependencies retain
their own licenses. Cargo.lock records the exact resolved source versions.
No Tauri, Qt, Zed or terminal application code has been copied into this host.
The Cargo package's `MIT AND OFL-1.1` expression reflects its original MIT code
and the bundled font's separate OFL license; it does not relicense dependencies.

## Directly relevant projects

| Component | Role | Upstream |
| --- | --- | --- |
| winit | Native event loop/windows, input, IME and data transfer | https://github.com/rust-windowing/winit |
| softbuffer | CPU presentation surface | https://github.com/rust-windowing/softbuffer |
| AccessKit | Accessibility tree and native platform adapters | https://github.com/AccessKit/accesskit |
| ashpd | Wayland parent-window export for portal integration | https://github.com/bilelmoussaoui/ashpd |
| zbus / zvariant | Desktop D-Bus/portal communication | https://github.com/dbus2/zbus |
| windows / windows-sys | Public Windows API bindings | https://github.com/microsoft/windows-rs |
| fontdue | Diagnostic font rasterization | https://github.com/mooman219/fontdue |
| png | Bounded application-icon decoding and diagnostic image output | https://github.com/image-rs/image-png |
| unicode-segmentation | Grapheme-aware probe editing | https://github.com/unicode-rs/unicode-segmentation |
| serde / serde_json | Typed configuration and protocol serialization | https://github.com/serde-rs/serde |
| tracing / tracing-appender | Structured diagnostics and a bounded background log writer | https://github.com/tokio-rs/tracing |

The accessibility adapter follows the public AccessKit platform adapter API
and ownership requirements. It is a small integration written for winit 0.31,
not a vendored copy of accesskit_winit.

## Bundled font

The probe includes NotoSans-Regular.ttf under its upstream SIL Open Font
License. The complete supplied notice is preserved at
crates/ui4me-host/assets/FONT-LICENSE.txt. The font is used only for the native
integration probe; its inclusion does not imply coverage or shaping for every
writing system.

## Distribution

Source dependencies are downloaded by Cargo rather than vendored into this
archive. Preserve their required notices when distributing compiled products.
The included font notice must accompany copies of that font. This source
repository does not itself constitute a published binary product or app store
submission.
