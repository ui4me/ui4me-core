# Implementation and verification status

Qualified on **2026-11-12** on the two v0.1 target environments below.

**Delivery state:** all automated gates and all fifteen mandatory native
acceptance scenarios (N01–N15) pass on both targets with the final integration
tree, installed for a normal user. The host is ready to use in these tested
v0.1 environments. Physical Windows hardware, other Linux desktops, X11 and
macOS were not tested (see limits).

The supplied preparation report (cloud container, no desktop) is superseded;
its files remain under `docs/verification/*.txt|json|png` for history.

## Target environments

| | Linux | Windows |
| --- | --- | --- |
| Machine | Linux workstation, x86_64, 16 CPUs | VM `<windows-vm>` (Proxmox), x86_64, 12 vCPUs, 20 GB |
| OS | Debian GNU/Linux 13 (trixie), kernel 6.12.116 | Windows Server 2022 Standard Evaluation 10.0.20348 (Desktop Experience) |
| Desktop | KDE Plasma 6.3.6, KWin 6.3.6 native Wayland, HDMI-A-1 2048×1080@60 scale 1 | RDP session 2 of the dedicated normal user `ui4me_test` (member of Users and Remote Desktop Users only); xfreerdp3 3.15 from the Linux workstation, 1600×1000, desktop scale 100 % (150 % for N08), RDP clipboard redirection disabled |
| Portals / IME | xdg-desktop-portal 1.20.3, xdg-desktop-portal-kde 6.3.5, fcitx5 5.1.12 pinyin (temporary) | Microsoft Pinyin (temporary for the test user) |
| Toolchain | rustc/cargo 1.99.1 (pinned), Python 3.13.5 | rustc/cargo 1.99.1 (per-user rustup), MSVC Build Tools 17.14.44, Windows SDK 10.0.26100, Python 3.13.17 (all-users install) |
| Workspace | this checkout | `F:\ui4me\src` (synced byte-exact from this tree; tree id recorded per run) |

Native GUI work on Windows ran inside the user's interactive RDP session
through `tools/windows/start_interactive.ps1` (verified session 2, visible
window station); input was real RDP keyboard/mouse input, observation used UI
Automation and screenshots. The same username over SSH was used only for
builds and headless tests.

## Automated gates (final tree)

| Check | Linux | Windows (as `ui4me_test`) |
| --- | --- | --- |
| `python3 tools/verify.py` (fmt, check, test, python, headless, faults) | PASS | PASS |
| Rust library tests | 53 passed | 45 passed |
| Real process/IPC integration tests (Unix socket / named pipe) | 8 passed (+1 fixture) | 8 passed (+1 fixture) |
| Python tests (21) | 20 passed, 1 Windows-only skipped | 20 passed, 1 Linux-only skipped |
| Headless reference application | PASS | PASS |
| Headless fault fixtures (incl. Windows forced host kill) | 7 PASS | 8 PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | PASS |
| `python3 tools/windows_source_check.py` (cross-target lint) | PASS | — |
| Vendored crate unit tests (accesskit_atspi_common, 8) | PASS | — |

Reports: `verification/final-linux/` (local), `verification/windows-vm/win-final/`
(copied from the VM), summarized in `docs/verification/2026-11-12/`.

## Native acceptance (both targets: 15/15 PASS)

Ledgers: `docs/verification/2026-11-12/linux-acceptance.json` and
`windows-acceptance.json` (written by `tools/native_acceptance.py` inside the
interactive sessions, with evidence hashes). Per-scenario commands and
observations: `*-evidence-summary.json`. Raw logs and screenshots stay in the
git-ignored `verification/` tree.

| | Requirement | KDE Wayland | Windows RDP |
| --- | --- | --- | --- |
| N01 | Launch, identity, icon, grouping | Kickoff launch; `app_id`/desktop file `org.ui4me.HostReference`; one grouped task entry with icon | Start Menu launch; shortcut and process AppUserModelID `org.ui4me.HostReference`; one grouped taskbar button |
| N02 | Second launch → one backend | while running and 3× simultaneous cold launches | same |
| N03 | Window ownership/ops/close | resize, (un)maximize, minimize, fullscreen, retitle, close veto/allow; placement and un-minimize rejected | same plus positioning and un-minimize; real `WM_CLOSE` |
| N04 | Clipboard with other apps | wl-clipboard, Klipper (Qt), GTK 3 both ways, Unicode; 2 MiB rejected (1 MiB bound) | PowerShell and Notepad both ways, Unicode; 2 MiB rejected |
| N05 | Dialogs, cancel, races | portal open/save/folder, Esc, F7, parent close, quit | IFileDialog owned/modal, same matrix |
| N06 | Real IME | fcitx5 pinyin: inline preedit, candidates at caret, commit, edit, cancel, selection | Microsoft Pinyin via RDP scancodes, same |
| N07 | Accessibility | AT-SPI tree, Tab focus, AT-SPI focus/click/caret | UIA tree, Tab/Shift+Tab, SetFocus, Invoke, ValuePattern |
| N08 | Scale change | 1 → 1.5 → 1 with re-render | RDP reconnect at 150 % |
| N09 | Freeze/crash, local responsiveness | crash failure UI, typing, accessible Quit; held decision, typing, Ctrl+Shift+Q | same |
| N10 | Child/grandchild cleanup | normal and forced host kill (systemd user service) | normal and forced host kill (Job Object) |
| N11 | External app survives | Kate via portal OpenURI | Notepad via ShellExecute |
| N12 | Installed normal-user launch | `~/.cargo/bin`, app in `~/.local/share/ui4me-reference`, desktop entry | `%USERPROFILE%\.cargo\bin`, app in `%LOCALAPPDATA%\ui4me-reference`, Start Menu shortcut |
| N13 | Drag-and-drop | Dolphin file, KWrite text | Explorer file, WordPad text |
| N14 | Session isolation | separate KWin realm has its own instance | RDP disconnect/reconnect continuity; SSH-logon launch does not forward into session 2 |
| N15 | Performance | see below | see below |

After the last code change both targets re-ran the automated native subset,
all native fault fixtures and targeted re-checks (dialogs, clipboard,
instance forwarding, installed launch): `linux-final-regression.txt`,
`windows-final-regression.txt`.

## Measurements (final installed release builds, `tools/measure_resources.py`)

Files: `docs/verification/2026-11-12/linux-measurements.json` and
`windows-measurements.json`, re-measured after the last code change (the N15
ledger entries reference the earlier, equivalent runs).

| | KDE Wayland | Windows RDP |
| --- | --- | --- |
| Startup → backend `app.ready` (5 cold runs) | 114–129 ms (includes systemd-run bootstrap) | 98–106 ms |
| First frame after host process start | 57–61 ms | 87–95 ms |
| Idle CPU, host / backend (22 s) | 0.23 % / 0.28 % of one core (schedstat runtime) | below accounting resolution (<0.07 %) |
| Memory after idle, host + backend | PSS 30.1 MB (summed RSS 40.4 MB double-counts 15.6 MB shared) | private 28.6 MB (working sets 47.3 MB) |
| 400 window create/close cycles, host private | 15.6 → 16.6 MB, ~3.3 KiB/cycle (see limits) | flat (14.76 → 14.83 MB) |
| Executable | 12.7 MB | 4.95 MB |

Run-to-run variation on the shared desktop was about ±20 ms for startup.

## Defects found and fixed during qualification

Host (`crates/ui4me-host`)
1. Killing or interrupting `ui4me-host run` on Linux (Ctrl+C, closed terminal,
   SIGKILL) left the systemd-supervised application running orphaned. The
   supervised host now watches its launcher (pidfd + start time) and shuts
   down boundedly when it disappears, without approving pending decisions.
2. Windows pipe `shutdown()` was not observed by the peer; the handle is now
   closed once no overlapped operation uses it.
3. A launch arriving while the primary instance was starting failed (exit 1)
   and lost the activation; activations are accepted only after `app.ready`
   and the secondary retries within the startup deadline.
4. `window.set minimized:false` reported success on Wayland although nothing
   can un-minimize there; it is now an explicit unsupported error.
5. Windows native dialogs all failed ("handle not available") because winit
   exposes Win32 handles only on the GUI thread; the owner HWND is now
   captured there.
6. Windows dialog cancellation (F7/`request.cancel`) left the dialog open; the
   worker now also presses the dialog's own Cancel.
7. `{app_dir}/…` manifest values were invalid on Windows (verbatim `\\?\`
   prefix), so the installed reference app could not start.
8. A Start Menu launch showed a console window of host logs and an extra
   taskbar button; a console created solely for the host is now released.
9. A clipboard read right after a write could return the previous Wayland
   selection; such reads now wait (≤1 s) for the new selection.
10. Idle polling reduced (500 ms when no deadline is pending; `poll()`-based
    Linux accept instead of 20 ms sleeps): idle host CPU about 0.6 % → 0.23 %
    (schedstat). `doctor` now reports the containment `run` uses; first
    frames are logged.

Vendored dependency patches (exact pins, see `third_party/README.md`)
11. winit-wayland: GTK applications could not paste UI4ME text and GTK text
    arrived ASCII-escaped (`\u2192`); the canonical
    `text/plain;charset=utf-8` type is now offered and accepted.
12. softbuffer: redrawing a covered or minimized window blocked the GUI thread
    forever (input, IPC and Ctrl+Shift+Q dead); buffer acquisition no longer
    blocks.
13. accesskit_atspi_common: AT-SPI objects of closed windows stayed
    registered (12.7 KiB per window); now about 3.3–3.8 KiB.

Tooling and reference client
14. Python client named pipes were synchronous, so a pending read blocked the
    `host.hello` write on Windows (every Python backend failed); overlapped
    I/O now.
15. Python backends crashed printing Unicode on Windows (ANSI pipe encoding);
    the client writes UTF-8 to the host's log pipes.
16. The native clipboard self-test now supplies the real input that Wayland
    requires for clipboard ownership instead of racing it.
17. `start_interactive.ps1` relied on `LastRunTime`, which was stale on this
    server, and timed out on successful jobs; it also gained `-Hidden`
    (headless console) so observation jobs do not steal focus.
18. Windows shortcuts are named after the application (`UI4ME Host
    Reference.lnk`) instead of the reverse-DNS id, so the Start Menu shows and
    finds them; an older owned shortcut is retired safely.
19. `windows_source_check.py` carries the workspace patches; new
    `tools/measure_resources.py`.

Regression tests were added for 1, 3, 4, 7, 13, 15 and 18 and the existing
integration suite covers 2 and 14 on Windows. Native-only fixes (5, 6, 8, 9,
11, 12) are covered by the recorded native scenarios and the scratch
reproductions described in the evidence.

## Install and run

    python3 tools/install.py --example-dir ~/.local/share/ui4me-reference
    python3 tools/create_launcher.py install --app ~/.local/share/ui4me-reference/app.toml
    ui4me-host run --app ~/.local/share/ui4me-reference/app.toml

On Windows (as the normal user, Python 3.11+ on PATH):

    python tools\install.py --example-dir %LOCALAPPDATA%\ui4me-reference
    python tools\create_launcher.py install --app %LOCALAPPDATA%\ui4me-reference\app.toml
    ui4me-host run --app %LOCALAPPDATA%\ui4me-reference\app.toml

`--example-dir` refuses non-empty directories; to refresh an existing copy,
copy `examples/python/client.py` and `demo.py` into it.

## Remaining limits

1. **Residual per-window memory on Linux:** ~3.3–3.8 KiB per closed window remains
   in zbus 5.19, which never prunes the empty `/org/a11y/atspi/accessible/<n>`
   object-path node and offers no API to do so.
2. **KDE folder dialog parent:** the host sends `parent_window` and
   `modal=true`, but xdg-desktop-portal-kde 6.3.5 shows its directory chooser
   unparented. Open and save dialogs are parented.
3. **`desktop.open_external` has no window** in protocol v1, so KDE's
   application chooser is unparented.
4. **AT-SPI application name** is the executable name (`ui4me-host`);
   accesskit_unix has no setter. Window titles carry the app name. Screen
   reader speech (Orca/Narrator) was not evaluated.
5. **Wayland clipboard writes need an input serial:** a backend that writes
   before any key/pointer input reaches its window gets an explicit error.
6. **Probe text rendering** has no font fallback: emoji/CJK render as boxes in
   the diagnostic surface although the text is stored and transferred intact.
7. **Placement/focus obey the compositor;** Wayland placement and
   un-minimize are rejected explicitly.
8. **winit 0.31.0-beta.4** remains an exactly pinned prerelease, now with a
   patched Wayland backend; drop the patches when upstream releases fix them.
9. **Native transfer allocation** is still not a hard memory sandbox (a 2 MiB
   transfer is rejected after it is read; RSS grew ~130 KiB).
10. Windows Search on this Server Evaluation image returned no results, so the
    Start Menu app list was used; physical Windows hardware, high-DPI
    physical monitors, multi-monitor setups, X11 and macOS are untested.
11. Linux per-instance `.lock` files persist in `$XDG_RUNTIME_DIR/ui4me-host`
    (tmpfs, cleared at logout).

## Lab notes for future runs

- Per-user MSI installs are blocked by policy on the VM; Python 3.13.17 was
  installed for all users by the administrator.
- No Proxmox recovery action was needed; the VM stayed responsive.
- Desktop settings changed for tests were restored: KWin input method,
  fcitx5 profile, display scale, the user's clipboard, the test user's
  language list. The user's broken AT-SPI bus socket (unlinked before this
  session) was repaired by restarting `at-spi-dbus-bus.service`.
