# Windows smoke: native services after moving the engine off the main thread (F-01 / roadmap 1.1a)

Integration `a02d21a`, release host, interactive RDP session of `ui4me_audit` on winDEV. Scripts:
[winsmoke.py](winsmoke.py) (app) and [job_winsmoke.ps1](job_winsmoke.ps1) (UI Automation watcher, run with
`tools/windows/start_interactive.ps1 -Hidden`).

| Check | Result |
| --- | --- |
| Clipboard write/read round trip with non-ASCII text | `CLIPBOARD True` |
| Native open-file dialog (`window.open_files`) created from the `ui4me-engine` thread | found by UIA as `#32770` "UI4ME smoke open", owned by the host process |
| Dialog dismissed through UIA `WindowPattern.Close` | app received `{'cancelled': True, 'paths': []}` |
| UIA tree of the app window | Document; Text "Smoke"; Button "Save changes" (focusable); Edit "User name" (focusable); Hyperlink "Help link" (focusable); Text "ready" |
| Orderly quit | `SMOKE_PASS`, exit 0 |

First attempt: invoking the first element named "Cancel" hit a UIA *Pane*, not the button, so the dialog stayed
open and the app timed out. That was a probe error. The second run closed the dialog with `WindowPattern.Close`.
Not covered here: drag and drop with real input (N13), IME composition (N06), screen readers (NVDA/Narrator).
