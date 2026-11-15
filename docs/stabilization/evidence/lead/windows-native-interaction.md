# Native Windows interaction checks (F-08, F-17, F-17b), 1.0.1 vs integration

winDEV, interactive session of `ui4me_audit` (real desktop, no SSH input). Probe app
[`nat_app.py`](windows-native-interaction/nat_app.py) logs document events; driver
[`job_native.ps1`](windows-native-interaction/job_native.ps1) injects **real input with `SendInput`
using hardware scan codes** (Ctrl 0x1D, A 0x1E, C 0x2E, Tab 0x0F, Enter 0x1C) and real mouse buttons, and reads
geometry/focus through UI Automation. The Russian layout is activated in the app's window with
`WM_INPUTLANGCHANGEREQUEST` and confirmed with `GetKeyboardLayout` of the window thread (0x419) and by the events
(`key` "ф" for `KeyA`). Baseline = the published 1.0.1 wheel (audit venv); fixed = release host at cd4a12e with
`python/` from 29f9f8d. Raw logs: [`base101/`](windows-native-interaction/base101/), [`fixed/`](windows-native-interaction/fixed/).

| Finding | Check | 1.0.1 | Integration |
| --- | --- | --- | --- |
| F-08 | Tab from the link to a button 2,400 px below the fold in a 420 px window | button stays at y≈2,650 (outside the window), `FAR_VISIBLE False` | scrolled to y=610 inside the window, `FAR_VISIBLE True` |
| F-17 | Enter on the focused `<a href>` | no `click`, no `link_activated` | `click` + `link_activated` (`href` https://example.invalid/x) |
| F-17b (INTERACT-06) | Ctrl+A, Ctrl+C in an input with the Russian layout active | clipboard unchanged (`none`) | clipboard = "Привет мир" |
| F-17b (INTERACT-07) | press on the icon `<span>` inside a button, release on the button's padding | no `click` | `click` on the button |
| F-17b control | press on one button, release on a sibling button (common ancestor `<p>` not activatable) | no click | no click (as specified) |
| F-17b control | press and release on the same button / Ctrl+A,C with the US layout | click / copied | click / copied |

The result table is from the final run; earlier runs of the same probe (driver fixes only: a logging function
shadowed by PowerShell's `r` alias, buttons scrolled out of view) agreed on every row they covered.
Not covered here: screen readers (NVDA/Narrator are not installed on winDEV) and IME candidate placement (F-07).
