# UI4ME Core 1.0.2

UI4ME Core 1.0.2 is a stabilization release. It fixes the crashes, disconnects, memory growth, stalls and
accessibility defects found by the engineering audit of 1.0.1. It adds no features and does not change the IPC
protocol. Existing 1.0.x apps run unchanged, except for the two behaviour changes listed under
[Compatibility](#compatibility).

The package is now labelled **Beta** (PyPI classifier `Development Status :: 4 - Beta`; the PowerShell Gallery
description says "beta"). The API is the same as in 1.0.1. The label stays until the stable-release gates
recorded in `docs/stabilization/RELEASE-READINESS.md` pass.

## Fixed

### Crashes and disconnects

- **Windows: deep documents no longer crash the host.** 1.0.1 ran the engine on the 1 MiB Windows main-thread
  stack and aborted at about 40 nested `<div>`s, or fewer `<fieldset>`s. The engine now runs on its own 32 MiB
  thread, and layout uses about 11 KiB of stack per level instead of 23–35 KiB. Documents are limited to 256
  levels, and that limit now also holds for `move_element` and fragments inserted with `insert_html`.
- **Crafted SVG images no longer crash the host.** Images with 700 nested groups or 2,000 chained `clip-path`
  references overflowed the image worker. SVG nesting, reference chains, reference cycles and `<use>` expansion
  are now bounded, and the image worker has an 8 MiB stack.
- **Event bursts no longer disconnect a healthy backend.** The host treated more than 128 queued messages as a
  dead peer, which a colour-scheme switch over 100 animated buttons could produce. The outbound queue is now
  bounded by 8 MiB only, and each frame goes out in one write. A backend is disconnected only after 8 MiB queue
  up unsent, or when one frame cannot be written for 10 seconds.
- **The bindings never drop window `closed`, `document.error` or `app.shutdown`.** Python and PowerShell both sort
  incoming messages into three classes: control, coalescible (pointer, scroll, animation) and discrete (click,
  input, key). When callbacks fall behind, coalescible events are evicted first. A discrete event is refused only
  when there is nothing coalescible left to evict. Control messages are always kept. Every drop is counted and
  logged once per burst. Events nobody subscribed to are discarded on arrival and never take a slot.

### Memory and speed

- **Issue #2: memory growth with repeated font use is fixed.** The font file was copied again into the engine on
  each font reload. In a windowed 15-minute replay of the reported app, memory grew from 58 to 296 MiB on 1.0.1
  and stays at 44–55 MiB now. Font identity is now owned by the engine.
- **Long lists stay fast.** After eight unrelated updates, each append to a long list took 691–750 ms; it now
  takes 54–55 ms. A cold first render of the audit's test document went from 828–930 ms to 273–334 ms.
- **Content can no longer stall the GUI thread.** Times are release builds on Linux, 1.0.1 → 1.0.2:

  | Content | 1.0.1 | 1.0.2 |
  | --- | --- | --- |
  | 26 nested inline-blocks | 7.7 s | 0.2 s |
  | 250 nested inline-blocks | never finished | 0.3 s |
  | 40,000 unclosed `<div>`s | 5.0 s | 2 ms (refused) |
  | 4,000 inputs in one paragraph | 17 s | 0.95 s |
  | SVG `feTurbulence` with 10,000 octaves | 47 s | 0.2 s |
  | 64 elements with `filter: blur(50px)` | 6.2 s | 0.19 s |

  An SVG or CSS filter estimated to need more than about 100 ms is drawn without its expensive parts. Gradients
  with more than 1,024 stops, more than 64 filter functions, too many mask layers, and nesting beyond 128
  compositing levels are drawn degraded with a `BUDGET_EXCEEDED` diagnostic. In 1.0.1 they failed the document.

### Keyboard and accessibility

- Moving focus with Tab, from the backend (`focus`) or from assistive technology scrolls the focused element into
  view.
- Enter on a focused link activates it: `click`, then `link_activated`.
- Editing and clipboard shortcuts (Ctrl+A, Ctrl+C, …) work on non-Latin keyboard layouts, matched by physical key.
- A click whose press and release land on different parts of one control (for example a button's icon, then its
  padding) clicks the control. Their nearest activatable common ancestor receives it.
- Caret, IME, selection and accessibility geometry follow CSS transforms and `position: fixed` ancestors.
- The accessibility tree is no longer rebuilt on every repaint tick (caret blink, animations).
- Window focus loss cancels a pointer press in progress.

### Lifecycle and headless mode

- SIGTERM, SIGINT and SIGHUP on Linux, and console events and session end on Windows, start an orderly shutdown,
  so `app.shutdown` is delivered and `on_shutdown` runs. A repeated request is not treated as a force quit.
- The direct-start launchers (`python app.py`, `Start-UI4MEScript`) survive Ctrl+C and Ctrl+Break. They wait for
  the app to shut down and exit with 130. In PowerShell, Ctrl+Break no longer drops into the script debugger.
- A failed `accept` on the IPC listener is retried instead of ending it. A launch while the primary instance shuts
  down waits for it, then takes over. A full callback queue no longer silently vetoes a close.
- The host handles backend messages for at most 8 ms per turn before it paints and reads input, and it logs
  commands that take more than 100 ms.
- Headless mode runs transitions and animations on a virtual clock (`test.advance_time`) and produces frames
  without waiting for a read, as a window does.
- Linux: exported shell functions (`export -f`, Lmod) and non-UTF-8 environment values no longer stop the launch.

### Bindings

- **Python:**
  - Each callback is isolated: an exception in one handler does not skip the others, and its full traceback is
    printed.
  - `sys.exit()` or `KeyboardInterrupt` in a callback quits the app with that status.
  - A result that cannot be encoded as JSON gets an error answer.
  - A second `Application.start()` in one process raises `StartupError`.
  - pip and uv launcher shims named `ui4me-host` are not mistaken for the host.
  - `HostStaleError` is both a `StaleDocumentError` and an `RpcError`.
  - Timeouts are catchable as `TimeoutError` on Python 3.10.
  - The wheel ships `py.typed`.
- **PowerShell:**
  - Objects with member names PowerShell reserves (`psbase`, `psobject`, …) no longer kill the connection; they
    become hashtables, readable with the new `Get-UI4MEAttributes`.
  - `break` and `continue` in a callback stay inside it, with a warning.
  - Native dialogs no longer get the 15-second default request deadline.
  - `Start-UI4MEScript` identifies the right script when it is called from a helper.
  - `Wait-UI4ME -UntilShutdown` delivers deferred final events before it returns.
- **AllUsers install on Linux:** a PowerShell module installed for all users works for users who cannot make the
  bundled host executable. They run a hash-verified per-user copy of it.

## Compatibility

- **PowerShell policy callbacks** (`Register-UI4MEBeforeClose`, `Register-UI4MEBeforeQuit`) must output an
  explicit `$true` or `$false` as their last value. Anything else vetoes, with a warning. In 1.0.1 the output was
  converted with `[bool]`, so the string `'false'` allowed the close.
- **Python policy callbacks** that raise, or that return `None` (a forgotten `return`), keep the window open, with
  a warning. Other values are tested for truth, as before.
- **Event order:** the interaction fixes change a few default actions and their event order (Enter on links, click
  targets, focus scrolling). They are listed in the [protocol reference](../reference/protocol.md).
- **New public names:**
  - Python: `HostStaleError`, `Application.on_activated`, `RpcClient.dropped_by_kind`,
    `RpcClient.filtered_notifications`.
  - PowerShell: `Get-UI4MEAttributes`, and the transport counters `DroppedCoalescible`, `DroppedDiscrete`,
    `DroppedControl` and `FilteredNotifications`.
- The protocol version stays 1. A 1.0.1 host and a 1.0.2 binding interoperate, but the lifecycle fixes need the
  1.0.2 host.

## Release engineering

- Publishing is gated on the CI jobs required for the exact commit. The Windows wheel that is published can be the
  exact wheel that passed native qualification. The artifact verifier requires the Windows host's 32 MiB stack
  reserve and refuses debug-build hosts.
- CI covers Python 3.10 to 3.14, real-host Pester on PowerShell 7 (Linux and Windows) and on Windows PowerShell
  5.1, clean-machine installs, `cargo deny`, pinned actions, SBOMs and provenance. A nightly job runs the ignored
  and fuzz tests, the vendored-patch checks and a windowed smoke test.
- The version has a single source.
- See [CHANGELOG.md](../../CHANGELOG.md), [SECURITY.md](../../SECURITY.md) and the
  [versioning policy](versioning.md).

## Verification

Each fix was first shown failing on the 1.0.1 baseline, then covered by a regression test. All of it was verified
on Linux: the full verifier, the fuzz targets, the Python 3.10–3.14 suites, and windowed tests under a nested KDE
compositor. It was also verified on Windows Server 2022, natively in an interactive desktop session:
- release-host tests;
- the stack, SVG and stall suites;
- Pester with the real host on Windows PowerShell 5.1 and PowerShell 7.4;
- Python 3.10 and 3.13;
- event bursts at 100 controls;
- Ctrl+Break;
- the file dialog, clipboard and UI Automation tree;
- real keyboard and mouse input for focus scrolling, link activation, Russian-layout shortcuts and click
  targeting.

Evidence, per-finding status and commits are in `docs/stabilization/` (`MATRIX.md`, `tracker.json`, `evidence/`).

## Known open issues

These were found by the audit and did not land in 1.0.2:

- **Unverified natively:**
  - IME candidate-window placement and screen-reader output (NVDA, Narrator, Orca). The geometry and accessibility
    fixes are covered by tests and by UI Automation and AT-SPI checks.
  - Windows logoff and shutdown (`WM_ENDSESSION`).
  - Ctrl+C typed into a real console window. Ctrl+Break was sent through the console API.
- **Performance (Medium):** a single text update in a very large document is still proportional to the document
  size, about 27 ms at 2,000 rows (RENDER-07). `insert_html` validation dominates inserts into documents of about
  40,000 nodes (RENDER-09).
- **Direct start:**
  - `print()` output of a directly started app still goes through the host's 512-line log ring, so a very chatty
    app can lose lines (BIND-05).
  - A second launch of a single-instance app does not forward its script arguments (BIND-07).
- **Windows hardening:** DLL search-order hardening of the host (static CRT, `DEPENDENTLOADFLAG`) is not applied
  yet (INPUTSEC-10).
- **Lifecycle:** the systemd `KillMode=mixed` option and a pipe-server SID check are deferred (HOST-12).
- **Maintenance:** the vendored Taffy fork still carries about 1,800 added lines. Patch verification and dependency
  scans now run nightly (F-27).
- **Low severity:** API ergonomics and diagnostics items are deferred to a later release. They are listed with
  their reasons in `docs/stabilization/MATRIX.md`.
