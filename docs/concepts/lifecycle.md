# Application lifecycle and events

## Two processes, one app

A UI4ME app always runs as two cooperating processes:

- the **native host** (`ui4me-host`, Rust) owns the windows, the event loop, rendering, text editing and accessibility;
- your **app process** (Python or PowerShell) owns the logic and talks to the host over a private, authenticated local connection.

The host starts and supervises your app process. When you run `python app.py` or `pwsh -File app.ps1`, the start helper (`Application.start()` or `Start-UI4MEScript`) launches the host for you, and the host then runs your script a second time as the app process. See [Architecture](architecture.md) for the details.

## Startup

| Step | Python | PowerShell |
| --- | --- | --- |
| Connect (or start the host and wait) | `with Application.start(name=...) as app:` | `$app = Start-UI4MEScript -Name ...` |
| Create at least one window | `app.open_html(...)`, `app.open_file(...)` or `app.create_window(...)` | `New-UI4MEWindow`, then `Open-UI4MEHtml` or `Open-UI4MEFile` |
| Register callbacks | `document.on(...)` | `Register-UI4MEDocumentEvent` |
| Announce readiness and handle events | `app.run()` | `Set-UI4MEReady`, then `Wait-UI4ME -UntilShutdown` |

The host waits up to `startup_timeout_ms` (30 seconds by default for direct starts) for the app to connect and announce readiness. Readiness requires at least one window. If the deadline passes, the host shows an application-error window.

## Callbacks

```python
def on_submit(event):
    document.set_text("status", "Saved")

document.on("click", on_submit, element="submit")
```

```powershell
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element submit -Callback {
    Set-UI4MEDocumentText -Document $doc -Element status -Text 'Saved'
}
```

Callbacks are **not** browser event listeners. The host handles input natively, so typing, scrolling, focus and editing keep working while a callback is busy. It then sends an event notification to your process. Notifications are delivered **one at a time, in order**:

- **Python** runs them on a single event worker thread. While a callback waits for a reply (for example a document read or a file dialog), separate threads keep answering the host, so close and quit requests are still handled. Protect shared state if you also use your own threads.
- **PowerShell** runs them on your script's runspace while it is pumping (`Wait-UI4ME`, or any command that waits for a host reply).

Because callbacks arrive after the native default action has already happened, they cannot cancel it. To stop a default action, such as a form submission or a link activation, register the policy in advance with `document.prevent_default(...)` or `Set-UI4MEPreventDefault`.

## Documents and stale handles

Loading new content into a window (`window.load_file()`, `window.load_html()`, `Open-UI4MEFile` or `Open-UI4MEHtml` on an existing window) creates a **new document generation**. Handles to the old document and its nodes then become stale. Using one raises `StaleDocumentError` in Python or a stale-object error in PowerShell, rather than silently changing the new page. Use the new document returned by the load call, and register its callbacks again.

## Closing and quitting

- The user closes a window, or your code calls `window.request_close()` / `Close-UI4MEWindow`. A **before-close** policy (`window.on_before_close(callback)` / `Set-UI4MEBeforeClose`) can veto it by returning false.
- Closing the **last** window requests an application quit.
- Your code can request a quit with `app.request_quit()` / `Request-UI4MEQuit`. A **before-quit** policy (`app.on_before_quit(callback)` / `Set-UI4MEBeforeQuit`) can veto it.
- Once a quit is accepted, `app.run()` and `Wait-UI4ME -UntilShutdown` return. Clean up and let the script end. The host waits up to `shutdown_timeout_ms` (5 seconds by default) for your process to exit, then terminates it and anything it started.

Policy callbacks must answer quickly. They run on a separate path from event callbacks, so a slow event callback cannot block a close or quit decision.

## Errors and exit status

- If your app process raises an unhandled exception, or exits or disconnects before an orderly quit, the host stops it and shows an **application-error window** so the failure is visible. Close that window to finish.
- With direct start, the traceback or PowerShell error is printed in the terminal you started the app from. The first process exits with **0** after an orderly quit and a **nonzero** status after a failure. After Ctrl+C or a termination signal it exits with 128 + the signal number.
- A lost connection is final for that app process. Creating a new `Application` does not reconnect to a host that has gone away; start the app again.

## Output, input and environment

- Output that your app process writes (`print()`, `Write-Output`, `Write-Host`, errors) is captured by the host. With direct start it is relayed to the terminal as plain text. Set `UI4ME_VERBOSE=1` to also see the host's own diagnostic records.
- The app process has **no standard input**. Use HTML form controls instead of `input()` or `Read-Host`.
- The connection secret (`UI4ME_TOKEN`) is removed from the app process's environment after connecting, so programs your app starts do not inherit it.

## Second launches

With the default `instance="single"` (`-Instance single`), starting an app that is already running does not open a second copy. The new launch is forwarded to the running instance, which receives an `app.activated` notification, and then exits with status 0. Use `instance="multiple"` to allow independent copies. The identity used for this is `app_id` / `-AppId`; without one, UI4ME derives a stable ID from the script's path.
