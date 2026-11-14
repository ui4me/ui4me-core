# Platforms

## Supported platforms

| | Linux | Windows |
| --- | --- | --- |
| Architecture | x86-64 | x64 |
| OS | glibc 2.34 or newer (`manylinux_2_34` wheel) | Windows 10, 11 and Server 2022 desktop editions |
| Desktop session | **Wayland** compositor, with a systemd user session and a D-Bus session bus | Interactive desktop session (local or RDP) |
| Python | 3.10–3.14 | 3.10–3.14 |
| PowerShell | PowerShell 7.2+ | PowerShell 7.2+ and Windows PowerShell 5.1 (.NET Framework 4.7.2+) |
| Qualified on | Debian 13, KDE Plasma 6.3, native Wayland | Windows Server 2022 Desktop Experience, interactive RDP session |

Not supported: macOS, ARM64, 32-bit systems and **X11 sessions**. The Linux host is built with winit's Wayland backend only and has no X11 or XWayland fallback, so it needs `WAYLAND_DISPLAY` (or the default `wayland-0` socket). Windows 10 and 11 client editions use the same Win32 code paths as Server 2022 but were not part of the qualification runs. PowerShell 7.0 and 7.1 are refused when the module is imported.

The packages contain a prebuilt host: the Python wheel (`py3-none-manylinux_2_34_x86_64` / `win_amd64`) and the PowerShell module (both hosts) need neither Rust nor a compiler.

**Linux services used at runtime:** a Wayland compositor; `systemd-run --user` for supervision (see [host CLI](../reference/host-cli.md#supervision)); `xdg-desktop-portal` for file dialogs, external open and appearance settings; the `org.freedesktop.FileManager1` service for reveal-in-folder; the AT-SPI bus for accessibility. Check an installation with `ui4me-host doctor`.

## Platform differences

### Windows and focus

Wayland gives the compositor, not the application, control over placement and focus. The host refuses these requests on Linux with an "unsupported" error instead of pretending they worked:

| Operation | Linux (Wayland) | Windows |
| --- | --- | --- |
| Create a hidden window / show or hide an existing window | Refused | Supported |
| Focus a window (`focus=True`) | Refused; use an attention request | Supported |
| Set the window position (`x`, `y`) | Refused; observed position is `None` | Supported (position reported in physical pixels) |
| Restore a minimized window (`minimized=False`) | Refused (no protocol request exists) | Supported |
| Minimize, maximize, fullscreen, resize, title, min/max size | Supported (compositor may adjust) | Supported |
| Request attention | Requires the compositor's `xdg-activation`; clearing a request is refused | `critical` / `informational`, clearable |
| Activate on second launch | Forwards `XDG_ACTIVATION_TOKEN`; the compositor decides whether to raise | Primary window is focused |

Query what the current host supports with `app.capabilities()` (Python) or `Get-UI4MECapabilities` (PowerShell): each feature reports `implemented` and `available`. Window state queries separate what you requested from what the platform reports (`observed`); values a platform cannot report are `None`/`$null`.

Window frames are drawn by the compositor where it provides server-side decorations (KDE). Frameless windows with `data-ui4me-drag`/`data-ui4me-resize` regions use the native move/resize operations on both platforms.

### Native services

| Service | Linux | Windows |
| --- | --- | --- |
| Open / save / choose-folder dialogs | `xdg-desktop-portal` FileChooser, parented to the window | Win32 common item dialogs (`IFileOpenDialog`/`IFileSaveDialog`) |
| Open URL or file | Portal `OpenURI`, parented | `ShellExecuteEx`; executable file types (`.exe`, `.bat`, `.lnk`, …) are refused |
| Reveal in file manager | `org.freedesktop.FileManager1.ShowItems` (no parent window) | `SHOpenFolderAndSelectItems` |
| Clipboard | Wayland clipboard, plain text | Win32 clipboard, plain text |
| Desktop preferences | Portal `org.freedesktop.appearance`: `color-scheme`, `reduced-motion`, `contrast` | Registry `AppsUseLightTheme`, `SPI_GETCLIENTAREAANIMATION`, `SPI_GETHIGHCONTRAST`, updated on `WM_SETTINGCHANGE` |
| Incoming file/text drops | Wayland data transfer (`text/uri-list`, text) | OLE drag and drop |
| Per-app directories (`desktop.paths`) | `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_CACHE_HOME`, `$XDG_STATE_HOME` + app ID | Roaming AppData (config), Local AppData `\data`, `\cache`, `\state` |
| Supervision | systemd user service (`systemd-run --user`) | Job Object (kill on close) |
| IPC endpoint | Unix socket in `$XDG_RUNTIME_DIR/ui4me-host` (mode `0700`) | Named pipe restricted to the logon session |

A preference the desktop does not publish stays `None` (unknown) rather than being guessed; CSS `prefers-*` queries then use their default.

### Text input and IME

Composition (preedit), surrounding text and the candidate-window position are supported on both platforms through the active input method. Input **purpose and hints** derived from `type` and `inputmode` (password, email, URL, number, phone, …) are sent on Wayland only; the pinned winit Windows backend cannot apply them, so the `ime_purpose_hints` capability is false there.

### Accessibility

The host exposes the document through AccessKit: **AT-SPI** on Linux (Orca) and **UI Automation** on Windows (Narrator, NVDA). The manifest's application name is used for the AT-SPI application node.

### Display scaling

Window sizes and CSS pixels are logical. The host renders at the window's current scale factor (the Wayland output scale, or the Windows per-monitor DPI) and re-renders when it changes; `window.event` reports `resized` (physical size and scale) and `scale_changed`.

## Known limitations

**Linux**

* At an output scale other than 1, AT-SPI reports element extents in physical pixels (AccessKit adapter behaviour).
* `aria-labelledby` pointing at an element that is not itself exposed gives the name but no labelled-by relation. AT-SPI Table/TableCell interfaces are not provided; row and column indices are exposed as attributes.
* While the AT-SPI bus is enabled, about 25 KiB is retained per closed window.
* KDE's folder chooser is not parented to the window, and KDE's portal does not publish a reduced-motion value.
* The resize grip of `resize` stays at the physical lower-right corner in RTL content.
* A composition interrupted by a focus change stays in the field that owned it.
* An empty `text/uri-list` drop is delivered as a drop with zero paths.

**Windows**

* AccessKit's UI Automation adapter has no Grid/Table/GridItem patterns (tables appear as DataGrid/DataItem trees) and exposes `aria-describedby` as FullDescription rather than a relation.
* The managed `System.Windows.Automation` client cannot read text from a selection range on Windows Server 2022; native IUIAutomation clients (Narrator, NVDA) can.
* The "Open with" chooser for unassociated file types is a separate shell process: its cancellation is not reported and closing the parent window does not close it.
* Delayed-render drop data from OLE sources is read synchronously during the drop.
* Vertical Aero Snap can size a window a few pixels beyond its maximum height; ordinary edge and corner drags clamp exactly.
* Microsoft Pinyin consumes Tab while composing.
* Right and middle mouse presses do not move focus (same on Linux).
* High-contrast theme switching and multi-monitor DPI changes were not part of the qualification runs.

**Both platforms**

* Plain-text clipboard only (no HTML or image formats); no Linux primary selection.
* No tray icons, native menus, notifications, global shortcuts, always-on-top or transparent windows, owned/child windows, or outgoing drags to other applications.
* No printing or PDF output.

For HTML and CSS limits see [HTML and CSS scope](../concepts/html-css-scope.md). For setup problems see [Troubleshooting](troubleshooting.md).
