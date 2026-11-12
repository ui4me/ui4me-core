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
archive, with host exceptions documented in third_party/README.md and the renderer
source trees documented in patches/README.md, each the
exact pinned package with a minimal patch: winit-wayland 0.31.0-beta.4
(Apache-2.0; clipboard MIME types), softbuffer 0.4.8 (MIT OR Apache-2.0;
non-blocking Wayland buffers), accesskit_atspi_common 0.21.1 (MIT OR
Apache-2.0; AT-SPI interface unregistration) and winit-win32 0.31.0-beta.4
(Apache-2.0; no window-state lock held across IMM calls). Their license files or declared
licenses are retained. Preserve their required notices when distributing compiled products.
The included font notice must accompany copies of that font. This source
repository does not itself constitute a published binary product or app store
submission.

## Migrated renderer

The following source/runtime components are now also used. Cargo.lock and
workspace manifests specify exact versions and sources; their upstream license
declarations and included notices govern those components.

| Component | Role | Source/license reference |
| --- | --- | --- |
| html5ever / markup5ever | HTML parsing | Servo projects; crate manifests/notices |
| Stylo / selectors | CSS parser, cascade and selectors | `patches/stylo`, predominantly MPL-2.0 |
| Taffy | Layout primitives and algorithms | `patches/taffy`, MIT |
| Parley / Fontique / HarfRust / Skrifa | Shaping, font selection and outlines | Linebender projects; crate manifests/notices |
| Vello CPU / peniko / kurbo | CPU paint and geometry | Linebender projects; crate manifests/notices |
| image / usvg / resvg | Local raster and static SVG resources | image-rs and svg projects; crate manifests/notices |

The renderer also carries a Noto Sans last-resort font and test/example fonts
with their OFL notices, hyphenation data at
`crates/ui4me-renderer/src/text/data/hyphenation/NOTICES.txt`, UA stylesheet
provenance at `crates/ui4me-renderer/src/style/ua/PROVENANCE.md`, and parser test
fixtures with their WPT notice. These data notices are preserved in the source
archive. The example's `@font-face` font has its own adjacent FONT-LICENSE.txt.

The root MIT license describes original UI4ME code; it does not relicense the
carried Stylo, Taffy, font or other third-party source/data. The source of the
carried modifications is included here. Retain applicable source and notices
when packaging downstream binary distributions.
