# Platforms, dependencies and limitations

| Target | Qualified configuration | Binary wheel |
| --- | --- | --- |
| Linux | Debian 13 / KDE Plasma 6.3 / native Wayland on x86-64 | `py3-none-manylinux_2_34_x86_64` (glibc 2.34+) |
| Windows | Windows Server 2022 Desktop Experience, x86-64, normal user in interactive RDP | `py3-none-win_amd64` |
| macOS, Linux X11, ARM64, Windows client editions, physical multi-monitor variations | Not qualified | None for 1.0.0 |

These are the configurations tested for the first stable release. They are not claims that the library fails on other Linux/Windows installations; they are also not promises of universal compatibility. Native desktop testing, especially actual keyboard/IME and accessibility, cannot be replaced by running unit tests over SSH.

## Linux notes

- The host uses native Wayland and desktop portals; it does not ship Chromium, WebKit, a display server or Qt.
- Some Wayland focus, visibility and positioning requests are unavailable through the pinned backend. The API returns explicit unsupported results rather than pretending the compositor performed them.
- KDE's folder picker may not attach to the parent window despite the parenting request. The host cannot force external app chooser windows to attach.
- AT-SPI on the tested system retains about 25 KiB per closed window; with accessibility disabled, that growth was negligible. This is documented as upstream resource retention.
- Color/emoji and CJK glyph presentation depend partly on installed system fonts. UI4ME supports native IME; it does not install the user's languages or fonts.

## Windows notes

- A normal interactive desktop is required for GUI tests and applications; an SSH shell alone is not an RDP/console desktop.
- The Windows host uses private named pipes, Job Objects, Text Services/IME, UI Automation, OLE drag-and-drop and Shell integration.
- The pinned AccessKit UI Automation adapter does not implement all table/grid pattern interfaces. NVDA and a native COM TextPattern client were exercised during qualification.
- Microsoft managed `System.Windows.Automation` has a known client-side failure when reading selected ranges against both UI4ME and unrelated native controls on the tested server. Do not treat that third-party crash as a provider hang; use a native IUIAutomation client for TextPattern checks.
- The Open-With chooser may be external to UI4ME; cancellation of that chooser is not observable. Some Aero Snap operations may override requested size constraints by a few pixels.
- The `ui4me-host` console command installed by pip is a small wrapper that waits for the native host. Closing the console or pressing Ctrl+C stops both, but if a process manager force-terminates only the wrapper the host keeps running. For services, schedulers or launchers, start `...\Lib\site-packages\ui4me_core\_bin\ui4me-host.exe` directly (and pass that path as `--host` to `tools/create_launcher.py`). On Linux the wrapper hands off to the host with `exec`, so signals reach it.
- Physical monitors, Windows client editions and Windows high-contrast changes were not fully exercised for this release.

## Software boundaries

UI4ME is a **native desktop HTML/CSS renderer**, not a general browser. JavaScript, web navigation, remote HTTP fetch, browser storage, embedded iframes/video, PDF/printing and a rich-text editing engine are out of scope. Static SVG and common CSS/HTML functionality are provided within the [supported feature profile](../CAPABILITY_SCOPE.md).

## Diagnostics and troubleshooting

`ui4me-host doctor` checks the host. `doc.diagnostics()` reports document/CSS/resource issues. `app.capabilities()` describes native operations; check results instead of assuming a platform request succeeded. See [verification evidence](../STATUS.md) and [native acceptance contracts](../CORE_1.0_ACCEPTANCE.md).
