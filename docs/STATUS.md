# UI4ME Core 0.2 — verification status

**Result: not yet fully qualified.** On KDE Plasma (native Wayland) every
automated gate, all host regressions N01–N15 and all renderer scenarios R01–R12
passed. On Windows Server 2022 (RDP), every automated gate, all host regressions
N01–N15 and eleven of twelve renderer scenarios passed. **R05 fails on Windows**:
an out-of-process UI Automation client calling the text-range `Select()` or
`GetSelection()` methods on a text field can wait indefinitely (details under
[Open defect](#open-defect)). Everything else listed below was observed on the
two real target desktops with the final release executables.

The 0.1 report remains in [BASELINE_0.1_STATUS](BASELINE_0.1_STATUS.md) and
`docs/verification/2026-11-12/`. None of its results are reused here.

## What was tested

| | Linux | Windows |
| --- | --- | --- |
| Desktop | Debian 13 (kernel 6.12.116), KDE Plasma 6.3 / KWin native Wayland, one HDMI output 2048×1080 at scale 1 (1.5 for scale tests), local session | Windows Server 2022 Standard Evaluation 10.0.20348, Desktop Experience, RDP session of the normal user `ui4me_test` (Users + Remote Desktop Users, not elevated), xfreerdp3 1600×1000 at 100 % (150 % for scale tests), RDP clipboard redirection off |
| Toolchain | Rust 1.99.1, Python 3.13.5 | Rust 1.99.1 MSVC, VS 2022 Build Tools, Python 3.13.17 |
| Release host | `~/.cargo/bin/ui4me-host`, 39 769 840 bytes, SHA-256 `81a503fd…1411d3c` | `%USERPROFILE%\.cargo\bin\ui4me-host.exe`, 29 067 776 bytes, SHA-256 `196946ea…9645ce3` |
| Input/observation | uinput keyboard/pointer into KWin, AT-SPI, Orca 48.1 debug log, journald, spectacle | input through the RDP client, UI Automation, NVDA 2026.3.1 (portable, IO log), in-session GDI captures, Win32 queries, `cdb` stacks |
| IME | fcitx5 5.1.12 pinyin + keyboard-us-intl (temporary) | Microsoft Pinyin (temporary) |

Both ledgers are bound to the same executable-source identity (`7f009efe…`,
computed by `tools/native_acceptance.py` over the working tree). That identity is
exactly commit `6f88c9f` ("Fix defects found in v0.2 desktop verification") plus
the two local-only instruction files `AGENTS.md` and `CLAUDE.md`; a clean
checkout of that commit hashes to `136e07ca…`. The final documentation commit
changes only `docs/`, which the identity excludes. The Linux native observations were
recorded on milestone m5; the final m6 change touched only Windows-only code
(`winit-win32`), and the m6 Linux rebuild produced the byte-identical
executable, so those observations were carried into the m6 ledgers with that
justification stated in each evidence record. Windows observations were all
made on the m6 executable.

## Automated gates (final tree)

| Gate | Linux | Windows |
| --- | --- | --- |
| `tools/verify.py --jobs 2` (fmt, check, test, python, headless, renderer, faults) | 7/7 PASS; 1035 Rust tests passed, 0 failed, 11 ignored | 7/7 PASS; 949 passed, 0 failed, 9 ignored |
| `cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings` | PASS | PASS |
| Release install (`tools/install.py`) and launchers | PASS | PASS |

The four host unit tests that were socket-blocked in the preparation
environment ran and passed on both machines. Ignored tests are the pre-existing
long fuzz/measurement harnesses.

## Native results

| Check | KDE Wayland | Windows RDP |
| --- | --- | --- |
| N01 identity, icon, taskbar | PASS | PASS |
| N02 second launch forwarded | PASS | PASS |
| N03 window operations, async close | PASS | PASS |
| N04 clipboard with another app, 1 MiB bound | PASS (GTK3) | PASS (Notepad) |
| N05 dialogs, cancel, parent-close/quit races | PASS (KDE folder chooser unparented, known) | PASS |
| N06 real IME | PASS | PASS |
| N07 accessibility tree/focus/action | PASS | PASS |
| N08 scale change | PASS (1 → 1.5 → 1) | PASS (RDP 150 % → 100 %) |
| N09 frozen/crashed backend, failure UI | PASS | PASS |
| N10 managed child/grandchild cleanup | PASS (systemd) | PASS (Job Object) |
| N11 external app survives host exit | PASS | PASS |
| N12 installed launch outside checkout | PASS | PASS |
| N13 file/text drag and drop | PASS | PASS |
| N14 session isolation / RDP reconnect | PASS (nested KWin realm) | PASS (disconnect/reconnect) |
| N15 host measurements | PASS | PASS |
| R01 local HTML/CSS/assets | PASS | PASS |
| R02 controls and callbacks | PASS | PASS |
| R03 IME and clipboard in input/textarea | PASS | PASS |
| R04 keyboard, default actions, scrolling | PASS | PASS |
| R05 accessibility and screen reader | PASS (combobox value limitation) | **FAIL** (UIA text-range stall) |
| R06 windows, sizes, scale | PASS | PASS |
| R07 resource policy and failures | PASS | PASS |
| R08 pickers, dialog, popover | PASS | PASS |
| R09 covered/minimized updates, blocked callback, crash | PASS | PASS |
| R10 renderer measurements | PASS | PASS |
| R11 installed app via desktop menu | PASS | PASS |
| R12 browser reference (4 fixtures) | PASS | PASS |

Ledgers and per-check summaries:
`docs/verification/v0.2-2026-11-12/{linux,windows}-{host,renderer}-acceptance.json`
and `{linux,windows}-evidence-summary.json`. Raw logs, screenshots, AT-SPI/UIA
dumps and screen-reader logs are in the git-ignored `verification/v0.2/`.

Screen-reader journeys: Orca read the Name label/value/required state, echoed
editing, read the checkbox toggle and announced the status after Add row. NVDA
did the same on Windows and also read the combobox value.

## Defects found and fixed

1. **No keyboard path to submit the reference form** (reference app). `Add row`
   was `type="button"` and two text-like fields block implicit submission, so
   R04's keyboard submission could not happen. It is now the form's submit
   button; clicks, Space/Enter and Enter in a field all reach Python as one
   prevented `submit`, and `required` validation focuses the empty field.
2. **Clipped `<select>` chevron** (renderer). The chevron is drawn in the
   padding but was clipped to the content box, leaving a 2–3 px sliver with
   author padding. It now paints under the box scope. Pixel regression test.
3. **Live regions not exposed** (renderer). `role=status|log|alert` and
   `aria-live` were not mapped to AccessKit, so screen readers never announced
   status updates; generic `aria-live` containers were also flattened away.
   Mapped with implicit ARIA politeness; Orca and NVDA now announce the status.
4. **IME update feedback loop** (host). The host re-sent the full IME state on
   every refresh and redraw. With fcitx5 holding a dead-key composition this
   became an endless update/preedit loop: one dead key produced 9 365 probe
   events and the host dropped the backend; in documents it burned CPU. IME
   state is now sent only when it changes.
5. **Probe editor stayed scrolled after its text shrank** (host probe, reported
   by the user). A stale horizontal offset hid the first glyphs. The scroll is
   now clamped to the content width. Regression test.
6. **Non-text clipboard treated as an error** (host). Reading text while the
   clipboard held only an image returned "Transfer request failed"; it now
   returns empty text, as for an empty clipboard, while still waiting for the
   host's own just-written selection. `docs/PROTOCOL.md` documents this.
7. **Reduced motion ignored after an environment change** (renderer). A
   stylist rebuild adopted running animations without reconciling them with the
   new rules, so `prefers-reduced-motion: reduce` left an infinite animation
   ticking (26.7 % idle CPU in the measurement). Animations no longer listed by
   the element are cancelled after the rebuild; idle CPU is now 0.23 %.
   Regression test.
8. **GUI deadlock on Windows when focus moves during an IME composition**
   (`winit-win32` 0.31.0-beta.4). `request_ime_update` held the window-state
   mutex while calling `ImmAssociateContextEx`; TSF completed the composition
   synchronously and re-entered the window procedure, which locked the same
   mutex. Any click, Tab or UI Automation focus change during a Microsoft
   Pinyin composition froze the window. Vendored the exact package and
   released the lock before the IMM calls (see `third_party/README.md`).
9. **Platform-dependent ledger source identity** (tooling). Windows path
   ordering is case-insensitive, so the same tree hashed differently on the two
   systems. Paths are now ordered by their POSIX form.

Stacks and before/after numbers for 4, 7 and 8 are in
`docs/verification/v0.2-2026-11-12/defects/`.

## Open defect

**Windows UI Automation text ranges (R05).** With a text field in the reference
document, an out-of-process UIA client calling `ITextRangeProvider::Select()`
(when the field is not focused) or `ITextProvider::GetSelection()` can block
until the client is killed. The host stays responsive and keeps rendering; no
host thread was inside the provider while the client waited, and the host's
action handler only queues the request (`try_send`). Value, Toggle, RangeValue,
Invoke, Expand/Collapse state, tree navigation, focus, bounds, and NVDA's
reading/editing journey all work. Reproduction:
`docs/verification/v0.2-2026-11-12/defects/windows-uia-textpattern-repro.ps1`
(run inside the session with the app open; step 4 stalls). The cause is
probably in AccessKit's Windows provider/COM marshalling and still needs to be
isolated and fixed.

## Measurements (installed release builds)

`tools/measure_resources.py --renderer --startup-runs 5 --idle 30 --cycles 400
--dom-cycles 800 --sample-every 50`, plus the probe without `--renderer`.

| Metric | KDE (PSS) | Windows (private bytes) |
| --- | --- | --- |
| Executable size | 39.8 MB | 29.1 MB |
| Renderer startup to document ready (median of 5) | 527 ms | 133 ms |
| First native frame after host start | ~41 ms | ~94 ms |
| Idle host / backend CPU, animation reduced | 0.23 % / 0.28 % | 0.00 % / 0.00 % (below GetProcessTimes resolution) |
| Idle memory host / backend / combined | 47.4 / 12.7 / 60.1 MB | 25.8 / 15.1 / 40.9 MB |
| 800 DOM insert/update/remove cycles | flat (0.02 KiB/cycle) | flat (0) |
| 400 document window cycles | +18.6 KiB/cycle with AT-SPI active | non-monotonic, 30.2–37.7 MB (allocator), no linear trend |
| Probe startup / idle CPU / window cycles | 127 ms / 0.22 % / 3.27 KiB per cycle | 105 ms / 0.00 % / flat |

The Linux startup includes the systemd user-service re-entry. Linux PSS and
Windows private bytes are different metrics and are not compared.

**Window-cycle growth on KDE** is attributed to the accessibility bus: with
`org.a11y.Status.IsEnabled` off the same run plateaus; with it on, growth is
linear and scales with accessible-node count (probe ≈10 objects: 3.27 KiB per
closed window, the known 0.1 zbus residue; ~100-node document: ~19 KiB). DOM
churn shows no growth.

**Animation cost:** while the reference document's CSS pulse animates (motion
not reduced), the focused window repaints continuously and the host uses about
one third of a core on this machine (37 % KDE, focused). With reduced motion
or no running animation it is idle.

## Installation and launch

Linux (installed and registered for the desktop user):

```sh
python3 tools/install.py --example-dir ~/.local/share/ui4me-renderer-reference
python3 tools/create_launcher.py install --app ~/.local/share/ui4me-renderer-reference/app.toml
~/.cargo/bin/ui4me-host run --app ~/.local/share/ui4me-renderer-reference/app.toml
```

Kickoff entries: "UI4ME Desktop Workspace" (`org.ui4me.RendererReference`) and
"UI4ME Host Reference" (probe, `~/.local/share/ui4me-reference`).

Windows (as `ui4me_test`, in the RDP session):

```powershell
python tools\install.py --example-dir "$env:LOCALAPPDATA\ui4me-renderer-reference"
python tools\create_launcher.py install --app "$env:LOCALAPPDATA\ui4me-renderer-reference\app.toml" --icon "$env:USERPROFILE\.cargo\bin\ui4me-host.exe"
& "$env:USERPROFILE\.cargo\bin\ui4me-host.exe" run --app "$env:LOCALAPPDATA\ui4me-renderer-reference\app.toml"
```

Start Menu shortcuts: "UI4ME Desktop Workspace" and "UI4ME Host Reference".

## Limitations

- **Windows R05 open defect** above.
- **AT-SPI combobox value:** the AccessKit 0.21 Linux adapter exposes no text or
  selected child for a combobox, so Orca reads "Priority combo box Priority"
  without the current value (UIA on Windows exposes it).
- **AT-SPI extents at fractional scale** are reported in device pixels.
- **KDE folder chooser** (portal) is not parented to the requesting window
  (0.1 limitation, unchanged).
- **File inputs** render only "Choose File"; the selection is delivered through
  `change`/`get_files`, not shown in the control.
- **Composition on blur:** moving focus during a composition commits the
  displayed preedit into the field that owned it (as browsers do); Microsoft
  Pinyin itself consumes Tab while composing.
- **Fonts:** CJK and emoji rely on installed system fallback fonts; emoji render
  monochrome. The probe's built-in font has no CJK glyphs.
- **Window-cycle memory** with an active AT-SPI bus, and the cost of continuously
  running CSS animations, as measured above.
- **Not covered:** physical Windows hardware, multiple physical monitors, X11,
  macOS. The browser comparison covers only the four fixtures.

## Changes outside the repository

Restored after testing: fcitx5 input method (KWin `[Wayland] InputMethod` and the
temporary `~/.config/fcitx5`), the AT-SPI `IsEnabled` flag (back to off), the
display scale, the clipboard (the image that was on it), and the test user's
Windows language list (Microsoft Pinyin and the default-input override removed).

Left in place: both Linux installs and Kickoff launchers; the refreshed Windows
installs and Start Menu shortcuts for `ui4me_test`; the replaced
`~/.cargo/bin/ui4me-host` and `%USERPROFILE%\.cargo\bin\ui4me-host.exe` (now
0.2, which still runs 0.1-style probe apps); QA-only tools outside the
repository (`~/.local/share/ui4me-qa-venv` and Playwright's Chromium in
`~/.cache/ms-playwright`; `F:\ui4me\qa-venv`, `F:\ui4me\qa-browsers`, portable
NVDA in `F:\ui4me\qa-tools`); the Windows workspace `F:\ui4me` and the private
lab configuration `~/.config/ui4me/windows-lab.json`.

The Proxmox node hosting the VM rebooted on its own during the first Windows
build (its log shows earlier unexplained crash reboots too); the VM was started
again with `tools/windows_lab.py recover` and corrupted incremental build
outputs were cleaned. The Windows "Shutdown Event Tracker" prompt from that
reboot was left untouched in the test session.

## Reproduce

```sh
python3 tools/verify.py --jobs 2 --output verification/<run>
cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings
python3 tools/native_acceptance.py --automated --host ~/.cargo/bin/ui4me-host --output verification/<run>/host
python3 tools/renderer_acceptance.py --automated --host ~/.cargo/bin/ui4me-host --output verification/<run>/renderer
```

Use `python` and the Windows host path inside the RDP session on Windows.
