# BIND: language bindings (Python + PowerShell), client-side protocol and packaging, audit of UI4ME Core 1.0.1 (c6fd71d)

> **Coordinator note.** BIND-01/03 are consolidated as F-13, BIND-17/19 with HOST-01 as F-02, BIND-02 with BIND-PS-02/03 as F-29. **BIND-15 was tested by the coordinator and not confirmed**: on Windows Server 2022 with uv-managed Python 3.13, `pythonw idle.py` runs the host with no console window and no new `conhost` process ([log](../evidence/logs/win-pyw5-result.txt)). Where this report proposes PR #3-style dropping, read it as an allowlisted drop of non-semantic high-frequency types only; the audit's 1.0.2 recommendation is in [../07-roadmap.md](../07-roadmap.md) item 1.2. The full PowerShell fork report is retained as [BIND-powershell.md](BIND-powershell.md). `<scratch>/` and `exp/BIND/` paths were session-temporary; key probes are retained in [../evidence/scripts/bind](../evidence/scripts/bind/).


Two review agents did this audit. The parent agent covered Python, packaging, DX, the parity table and the live headless experiments. A forked sub-agent covered PowerShell. Its findings are merged in section 3b, and its full report is at
`<scratch>/exp/BIND/ps/PS_FINDINGS.md`.

## 1. Scope & method

**Read:** `python/ui4me_core/{application,client,_bootstrap,launcher,live_bitmap,scaffold,__init__,__main__}.py` in full, plus `skills.py` (skimmed). `setup.py`, `pyproject.toml`, `MANIFEST.in`, `.github/workflows/{ci,publish}.y*ml`. `docs/reference/{protocol,python-api}.md`, `docs/concepts/lifecycle.md`, and recipe 12 plus the threading text in `docs/guides/patterns.md`. The parts of `crates/ui4me-host/src/{backend.rs,transport.rs,host/mod.rs,platform/linux.rs}` that decide binding behaviour: log relay, outbound queue, close/quit decisions, systemd supervision.

**Ran (all outputs observed by me):**

* `python3 -m unittest tools.test_application tools.test_launcher tools.test_reference_client tools.test_bootstrap tools.test_live_bitmap`
  * Python 3.13.5: 70 tests OK (1 skipped: the Windows named-pipe test).
  * Python 3.14.10t (free-threaded): 70 OK.
  * Python 3.10.22: `test_bootstrap` cannot be imported (`ModuleNotFoundError: tomllib`), so the direct-start tests never run on the lowest supported Python.
* `python3 -m unittest tools.test_scaffold tools.test_skills tools.test_public_docs tools.test_run_example`: 31 OK.
* Adversarial scripts, all under `<scratch>/exp/BIND/py/`.
  * They ran against a copy of the dev host `target/debug/ui4me-host` (1.0.1, built from HEAD). The copy is at `.../exp/BIND/ui4me-host`.
  * Environment (`env.sh`): `PYTHONPATH=<repo>/python`, `UI4ME_HOST_BINARY=<copy>`, `UI4ME_TEST_MODE=1`. Every app was started with `python3 <script>` through the real `Application.start()` dual-role path, in headless mode. No windows were opened on the desktop.
  * The scripts are `e1_events.py` … `e17_single.py`, `e7_signals.sh`, `e9_drain.py`, `e9b_buffered.py`, `recipe12.py`, `fakevenv/`, `zdir1/`. Each finding below gives its exact observations.
* Startup DX scripts ran with `env -i` and an empty `XDG_RUNTIME_DIR`, which proves no compositor or user bus was reachable: `e8_hello.py` variants.

**Not available to me:** Windows. All Windows statements are reasoned from code and are labelled that way. I did not run Pester; the PowerShell fork did, see 3b.

## 2. Strengths to preserve

* **The transport is never blocked by user code.** `RpcClient._reader` only parses, validates and `put_nowait`s (`client.py:419-466`). `_writer` is a separate thread. Callbacks run on separate workers and may make nested blocking RPC calls (test `test_callback_can_await_nested_host_call`). Because the Python client never applies backpressure to the host, it cannot itself trigger the host's per-connection overflow disconnect; it drops locally instead. Keep this property. Its weaknesses are BIND-02 and BIND-17.
* **Lifecycle requests are separated from ordered application events.** Application events go to `ordered_notifications=True`, which is one ordered worker. `window.before_close`, `app.before_quit` and `app.shutdown` go to a pool of 4 workers. A callback blocked in a file dialog therefore cannot block close or quit. Test `test_blocked_full_event_queue_does_not_block_lifecycle_or_shutdown`.
* **No user callback runs while an `Application` or `Document` lock is held.** Handler lists are snapshotted under the lock and called outside it (`application.py:189-193, 229-253, 471-482`). I found no lock-order deadlock.
* **Facades are generation-bound.** `Document`, `Node`, `LiveBitmap` and subscriptions are rejected locally after a reload or unload. The host double-checks with `-32004`, so a delayed callback can never write into a replacement page.
* **Strict framing.** The 2 MiB limit is checked before the body is read. Duplicate keys, NaN and Infinity, and non-string IDs are rejected. Pending requests (128) and outgoing data (256 frames / 8 MiB) are bounded. Late responses to cancelled requests are ignored. `future.cancel()` sends `request.cancel`.
* **Windows named-pipe client design** (`NamedPipeStream`). Each operation gets its own OVERLAPPED and event, `CancelIoEx` runs before close, and the handle is closed only when idle. This is the correct fix for synchronous-handle I/O serialization. I could not run it.
* **The direct-start bootstrap has many correct details.**
  * Interpreter options and the `-m` form are kept, from `sys.orig_argv`, and the script path is made absolute.
  * A non-UTF-8 argv is refused clearly, and so are `-c`, stdin and REPL use.
  * The connection secret is scrubbed after connecting.
  * `UI4ME_DIRECT_START` guards against looping when the host cannot connect.
  * The temp manifest is removed, and stale manifests are swept with a uid check.
  * `PR_SET_PDEATHSIG` and a kill-on-close Job Object tie process lifetimes together. I verified that there were no leftover host or backend processes after SIGKILL to the launcher, SIGKILL or SIGABRT to the host, SIGTERM to the launcher, and Ctrl+C (`e7_signals.sh`, `e12_harness.py`).
  * Tracebacks are printed before the disconnect (`_report_before_disconnect`).
* **The `launcher.main` POSIX `execv` keeps signals and exit status unchanged.**
* **Wheel tagging is correct:** `py3-none-<platform>`, `root_is_pure=False`, with no CPython ABI coupling.
* **Free-threaded Python 3.14t passes the whole suite.**

## 3. Findings (Python, packaging, DX, cross-binding)

### BIND-01: A `BaseException` in any callback (e.g. `sys.exit()`), or an unencodable request result, silently kills the callback worker; events or close decisions then stop forever
- **Classification:** Confirmed defect (reproduced).
- **Component:** `python/ui4me_core/client.py` `_callback_worker` (l.469-482) and `_dispatch` (l.498-530).
- **Evidence:**
  - `_dispatch` catches only `Exception` around user code (l.506 and l.520). `_callback_worker` has `try: … finally: task_done()` and no `except` (l.478-482).
  - `SystemExit` raised in a worker thread is swallowed by `threading` without any output.
  - `self._enqueue(response)` (l.524) only catches `DisconnectedError` and `RpcError`. A `TypeError` or `ValueError` from `json.dumps(..., allow_nan=False)` in `encode_frame`, or a `ProtocolError` for a frame larger than 2 MiB, propagates and kills the worker before any response is sent.
- **Reproduction:**
  1. `e2_sysexit.py`: a focus callback calls `sys.exit(0)`. Output: `seen after two focus events: ['a']`, and the thread list no longer contains `ui4me-application-events`. Later events are never delivered, and `run()` then waits out the full 2 s drain (`run returned after 2.21 s`). Same result on 3.10.
  2. `e2b_policy_exit.py`: `on_before_close` calls `sys.exit()`. One pool worker disappears, the host's close decision stays pending, a second `request_close()` produces no new `before_close`, and the window cannot be closed (`closed? False`). In desktop mode the only remaining exit is Ctrl+Shift+Q.
  3. `e15_badresult.py` (socketpair mock host): request handlers returning `{"value": float("nan")}` or `object()` get **no response**, and both workers die (`live callback workers: []`). A subsequent valid request is never answered.
- **Severity / likelihood:** Medium, with medium-high likelihood. `sys.exit()` in a "Quit" button or close handler is a very common beginner pattern, and the symptom is a silently dead UI. The NaN/object case needs the low-level `app.rpc.on_request` API.
- **Impact:**
  - All later document and window callbacks stop. The window keeps rendering and editing natively, so the app looks alive.
  - Close policies stop being answered, so the window cannot be closed.
  - Nothing is printed.
- **Root cause:** User code runs inside a `BaseException`-unsafe loop, and response encoding is treated as infallible.
- **Remediation:**
  - In `_dispatch`, wrap the callback in `except BaseException`. For `SystemExit` or `KeyboardInterrupt` raised in a callback:
    1. Print one clear line to stderr: `sys.exit() called inside a UI4ME callback; requesting application quit`.
    2. Call `self.notify`/`call_async("app.request_quit")`. The `Application` can install the policy, or `app.force_quit` can be used for nonzero codes.
    3. For requests, send an error response, which the host treats as deny.
  - Wrap `encode_frame`/`_enqueue(response)` in `try/except (TypeError, ValueError, ProtocolError)`. On failure, send `{"code": -32603, "message": "Callback result is not JSON-serializable: <type>"}` and print a traceback.
  - Make `_callback_worker` survive any exception: use `except BaseException` and log.
- **Tests:**
  - Ordered worker: a callback raises `SystemExit`, then a second event arrives. Assert that the second callback runs and that `app.request_quit` was sent.
  - Pool: a policy raises `SystemExit`. Assert that a deny response is sent and the worker is still alive.
  - A request handler returns NaN. Assert a `-32603` response and that 4 workers are alive.
- **Alternatives:** Document "never call `sys.exit()` in a callback". That alone is insufficient because the failure is silent.
- **Complexity:** S.
- **Confidence:** High.

### BIND-02: The client drops notifications indiscriminately, including `window.event closed` and `document.error`; `Window` objects are then never retired, and the drop is invisible
- **Classification:** Confirmed defect (reproduced). Also a documentation inaccuracy.
- **Component:** `client.py` `_reader` (l.436-442), ordered queue `maxsize=128` (l.304); `application.py` `_window_event` (l.239-246).
- **Evidence:**
  - When the 128-slot ordered queue is full, every notification is counted in `dropped_notifications` and discarded. That includes `window.event`, which carries the `closed` bookkeeping, `document.error` (fatal retire), `app.activated` and `desktop.preferences_changed`.
  - Docs say "A long-running callback **delays** later events" (`patterns.md:561`, `python-api.md:472`) and "Notifications are delivered one at a time, in order" (`lifecycle.md:38`). Drops are mentioned only as the low-level `dropped_notifications` counter.
- **Reproduction:** `e6_overflow.py`. Window 1's focus callback sleeps 3 s. 80 focus commands produce about 320 events, then window 2 is closed by `request_close()` with the default allow policy. Observed:
  - `dropped so far: 191`
  - `closed events seen by listener: []`
  - `w2.closed = False | host windows: ['w1']`
  - `w2.document.set_text(...)` raises `RpcError -32004: Window does not exist` instead of `StaleDocumentError`
  - `app._windows` still holds `w2`.
- **Severity / likelihood:** Medium, with medium likelihood. Because the host forwards every DOM event (issue #1), including pointer moves and transitions, any callback that blocks for about 1-2 s (a synchronous HTTP call, a slow file save, a `wait=True` dialog) while the mouse moves can overflow the 128-slot queue.
- **Impact:**
  - Lost clicks and inputs.
  - Lost `closed` events: window listeners never fire, the API contract "stale → `StaleDocumentError`" is broken, and objects leak.
  - A lost fatal `document.error` leaves a facade for an unloaded renderer.
  - No warning is printed.
- **Root cause:** A single bounded FIFO with a drop-newest policy that does not look at message type.
- **Remediation:**
  1. Never drop control notifications (`window.event`, `document.error`, `app.activated`, `desktop.preferences_changed`, `request.cancelled`). Replace `queue.Queue` with a small custom deque that has a lock and condition. When the queue is full and a control message arrives, evict the **oldest droppable** DOM event instead. Prefer high-frequency types such as `pointermove`, `pointerover`/`out`, `mousemove`, `scroll`, and `transition*`/`animation*`. Keep the order of everything that is kept.
  2. Alternatively apply the cheap bookkeeping on the reader thread (mark the window closed, retire the document, both of which involve no user code) and keep only listener invocation queued.
  3. Print a rate-limited stderr warning when dropping starts: `UI4ME: event callbacks are N events behind; dropping pointer/scroll events`.
  4. Expose `app.dropped_events`.
- **Tests:** Block the event worker, push 200 DOM events plus a `window.event closed`, release. Assert that the listener sees `closed`, `win.closed` is `True`, and `StaleDocumentError` is raised.
- **Alternatives:** A host-side subscription filter (issue #1) shrinks the volume but does not remove the need for a typed drop policy.
- **Complexity:** M.
- **Confidence:** High.

### BIND-03: Callback errors are hard to see: one stdout line and no traceback; one failing handler skips the others; failing or `None`-returning policies veto silently
- **Classification:** Confirmed defect (DX). Partly a design tradeoff.
- **Component:** `client.py:506-507, 520-522`; `application.py:478-482` (`Document._dispatch`), `:187`, `:193`.
- **Evidence:**
  - The notification path prints only `Notification handler failed: KeyError: 'missing_key'`, with no file or line, and to **stdout** (`print` without `file=`). The window-event and shutdown paths use stderr.
  - `Document._dispatch` calls handlers in a plain loop, so the first exception aborts the remaining handlers for that event.
  - A request handler exception becomes the response `"Callback failed: ValueError"` and is not logged locally.
  - `bool(callback())` turns `None` into a veto.
- **Reproduction:** `e3_exceptions.py`. Stdout contained only `Notification handler failed: KeyError: 'missing_key'`. The second handler's message `second handler for same event ran` was never printed. A policy raising `ValueError` and a policy with no `return` both silently kept the window open (`window closed? False`). **stderr was empty.**
- **Severity / likelihood:** Medium (DX), with high likelihood.
- **Impact:** Beginners cannot find bugs in callbacks. A forgotten `return True` makes a window un-closable with no hint. One buggy handler silently disables unrelated handlers.
- **Root cause:** Minimal error reporting, plus `Document._dispatch` not isolating handlers.
- **Remediation:**
  - Print through `traceback.print_exc()` to stderr for notification, request and per-handler failures.
  - Isolate each handler in `Document._dispatch` and continue to the next.
  - For policies:
    - Keep "error or false means veto", which is the safe default for unsaved data.
    - When a policy raises, print `close policy for w1 raised …; closing was refused` with the traceback.
    - When a policy returns a non-bool, warn once: `on_before_close callback returned None; treating as "keep open". Return True to allow closing.`
- **Tests:** Capture stderr: assert the traceback contains the user's file and line, assert the second handler runs, and assert the warning appears for a `None` return.
- **Alternatives:** Treat `None` as allow. Rejected: risks data loss, and differs from the docs and from PowerShell.
- **Complexity:** S.
- **Confidence:** High.

### BIND-04: `host_binary()` can resolve to the `ui4me-host` console script itself, causing an infinite exec loop (POSIX) or, by reasoning, unbounded nested processes (Windows)
- **Classification:** Confirmed defect (reproduced on Linux; Windows reasoned).
- **Component:** `python/ui4me_core/launcher.py:19-29` (`shutil.which(name)` fallback) together with `pyproject.toml [project.scripts] ui4me-host = "ui4me_core.launcher:main"`.
- **Evidence:**
  - When `ui4me_core/_bin/ui4me-host` is absent, `host_binary()` falls back to `shutil.which("ui4me-host")`. The first match is the pip-generated Python wrapper for the same package.
  - `main()` then `os.execv`s that wrapper (l.48), which runs `main()` again.
  - `_bin` is absent in these cases:
    - a source checkout used via `PYTHONPATH`, which is the documented dev route, while a wheel's console scripts are on PATH;
    - an editable install, because setuptools editable mode maps the package to `python/` and the host copied into `build_lib` is not visible;
    - any broken install.
- **Reproduction:**
  - `fakevenv/bin/ui4me-host` is a wrapper with the same shape as pip's, plus an invocation counter. `env -u UI4ME_HOST_BINARY PATH=fakevenv/bin:… PYTHONPATH=<repo>/python timeout 5 ui4me-host --version` gave exit 124 with **65 re-executions in 5 s** and no output.
  - `Application.start()` under the same environment gave exit 124 with 56 re-execs. **The app hangs silently forever.**
  - On Windows, `main()` uses `subprocess.call` (l.49) instead of exec. Each level would spawn and wait on the next `ui4me-host.exe` launcher, producing an ever-growing process chain. This is reasoned, not run.
- **Severity / likelihood:** Medium, with low-medium likelihood (developer and contributor setups).
- **Impact:** A hang with 100% CPU and no message. On Windows, process exhaustion.
- **Root cause:** The PATH fallback does not exclude the package's own wrapper, and there is no recursion guard.
- **Remediation:** In `host_binary()`:
  1. Skip a `which()` hit that is not a native executable. POSIX: check the first 4 bytes are `\x7fELF`. Windows: the wrapper is a PE too, so use (2).
  2. Have `launcher.main` set `UI4ME_HOST_WRAPPER=1` before exec/spawn, and refuse to delegate if it is already set: `ui4me-host on PATH is this Python wrapper; install the platform wheel or set UI4ME_HOST_BINARY`.
  3. Or drop the PATH fallback and keep the clear `FileNotFoundError`.
- **Tests:** Put a temporary wrapper named `ui4me-host` on PATH and use a package without `_bin`. Assert that `host_binary()` raises and `main()` returns 1 within 1 s.
- **Complexity:** S.
- **Confidence:** High.

### BIND-05: In direct start, backend `print()` output is relayed through the host's lossy 512-line GUI-thread log ring; output is lost under load
- **Classification:** Confirmed defect (DX and observability).
- **Component:**
  - `crates/ui4me-host/src/backend.rs:12-13, 88-100`: the ring holds `LOG_LINES=512` and drops the oldest line when full.
  - The GUI-thread formatting budget.
  - `_bootstrap.relay_line` / `run_host`: backend stdout and stderr come back only as host JSON log records.
- **Reproduction:** `e11_print.py` with the debug host. The backend prints 20,000 lines; **155 reached the launcher's stdout**. stderr showed `3243 backend log records discarded because the bounded log ring was full` and `ui4me-host warn: backend log batch exceeded the GUI formatting budget (source=backend, omitted_records=481)`.
  - A release host will lose less, but loss is still by design.
  - Partial lines (`print(..., end="", flush=True)`) are held back until the newline.
  - `input()` raises `EOFError`, because stdin is null.
- **Severity / likelihood:** Medium, with medium likelihood. Print debugging and logging are the main way developers observe direct-start apps. A traceback printed during heavy output can be lost too.
- **Root cause:** The relay meant for the diagnostics window doubles as the developer console.
- **Remediation, option A (preferred for direct start):**
  - Add a manifest option such as `[backend] stdio = "inherit"`, which `_bootstrap.write_manifest` sets.
  - The host then spawns the backend with the launcher's own stdout and stderr, which the host inherited, and does not capture them. Order, speed and progress bars become those of a normal terminal.
  - Optionally tee the last N lines for the failure window.
- **Remediation, option B:** Make the drain lossless: block the backend's pipe (natural backpressure) instead of dropping, and format on a non-GUI thread.
- **Tests:** The backend prints 50k numbered lines. Assert that the launcher relays all of them in order within N seconds.
- **Alternatives:** A larger ring only moves the cliff.
- **Complexity:** M. It touches host spawn on Linux and Windows.
- **Confidence:** High.

### BIND-06: A failed first `open_html`/`open_file` cancels the whole app before `run()`; a quit or close before `run()` makes `run()` raise
- **Classification:** Confirmed defect.
- **Component:** `application.py:116-133` (`open_file`/`open_html` cleanup calls `window.request_close()`), `:140-147` (`run()` calls `ready()` unconditionally). The host rule is in `host/mod.rs:906-914`: closing a window before `app.ready` cancels startup.
- **Reproduction:**
  - `e14_fallback.py`: the first `open_html("<p onclick=…>")` raises `-32602` as expected. Then `open_html("<p>fallback</p>")` fails with `RpcError -32008: Application is shutting down or failed`, and `run()` fails the same way. A thread calling `request_quit` gets a third traceback.
  - `e1_events.py`: calling `app.request_quit()` before `run()` makes `run()` raise `-32008` and exit 1.
  - In desktop mode, a user closing the window while `main()` is still in slow initialisation produces the same traceback. This is inferred from the same code path.
- **Severity / likelihood:** Low-Medium, with medium likelihood. An error-page fallback is a natural pattern, and so are "quit during startup" and a fast user.
- **Impact:** An error-page fallback is impossible for the first window. Intentional or user-initiated early quits show a traceback and exit status 1.
- **Remediation:**
  - In `open_*`, when `not self._ready`, keep the empty window on load failure and re-raise. Document that the caller may call `window.load_html(...)` on `exc.window` or reuse it.
  - Alternatively give the host a policy-free `window.close`/destroy that does not cancel startup.
  - In `run()`, skip `ready()` when `rpc.shutdown_requested` is already set. Also treat `-32008` from `app.ready` as an orderly shutdown when `shutdown_requested` becomes set within a short grace period.
- **Tests:** Headless: failing `open_html`, then a fallback `open_html`, then `run()` returns normally. `request_quit()` before `run()` gives exit 0.
- **Complexity:** S.
- **Confidence:** High.

### BIND-07: Single-instance in direct start: a second launch exits 0 silently, its script arguments are lost, and Python has no `on_activated` API
- **Classification:** Confirmed defect (DX). Also an API gap.
- **Component:** `_bootstrap.launch`, which passes the user's argv only inside the temp manifest; the host's `app.activated` params (`protocol.md` "Second launches"); `application.py`, which has no `app.activated` handling.
- **Reproduction:** `e17_single.py`.
  - The second `python3 e17_single.py --open report.txt` exits **0 in 0.05 s with no output, even with `UI4ME_VERBOSE=1`**.
  - The primary receives `app.activated` with `args` = `['…/ui4me-host','run','--app','/tmp/ui4me-start-wtzuoh5w/app.toml','--headless']`. That is the host's argv, pointing at a temp manifest that has already been deleted. `--open report.txt` is lost.
  - The Python facade offers no handler; it only works through `app.rpc.on_notification("app.activated", …)`.
  - A real developer trap seen while testing: an earlier hung instance made every later `python app.py` exit 0 instantly with no message.
- **Severity / likelihood:** Low-Medium, with medium likelihood.
- **Impact:**
  - "Open file in the running instance" is impossible for direct-start apps.
  - Developers think the app "does nothing".
  - Combined with BIND-08, unrelated apps can capture each other's launches.
- **Remediation:**
  1. Have the host include the secondary's `[backend].args` and `cwd` in `app.activated` (for example `backend_args`). This is an additive field.
  2. Add `Application.on_activated(callback)`, with `callback({"args": script_args, "cwd": …})`.
  3. The secondary launcher prints `UI4ME: forwarded to the running instance of <name> (app id …). Use instance="multiple" to allow several.` on stderr when stderr is a TTY or verbose is on.
- **Tests:** A headless primary plus a secondary with args. Assert the notice is printed and the args are received.
- **Complexity:** S-M. It needs host cooperation for (1).
- **Confidence:** High.

### BIND-08: Default app identities collide: zipapps and directories share `__main__`, scaffolded apps share the `org.ui4me.examples.*` namespace, and `-m` identity depends on cwd
- **Classification:**
  - Confirmed for zipapp/directory and scaffold (proven from code).
  - Plausible for Windows console-script launchers; needs verification.
- **Component:** `_bootstrap.backend_command` l.137-143; `scaffold.py:40`.
- **Evidence and reproduction:**
  - `zdir1/`: `python3 tool.pyz`, `python3 appA` and `python3 appB`, all run from the same directory, all yield `identity module:__main__:<cwd>`, `label __main__` and **the same** `app_id local.ui4me.main.he1466a47a741`. With the default `instance="single"` they forward to each other, and the default window and app name is `__main__`.
  - pip's Windows `.exe` launchers run an appended zip as `__main__`. If so, every pip-installed UI4ME console app started from the same cwd would share one identity. This is reasoned from CPython and distlib behaviour and was **not run**.
  - `-m` identity is `module:<name>:<cwd>`, so `python -m myapp` from two directories gives two "single" instances.
  - Every `ui4me-new` project gets `org.ui4me.examples.<slug>`. Two unrelated "Notes" projects therefore share an app ID, desktop identity and single-instance endpoint. The ID also sits in the project's own reverse-DNS namespace.
- **Severity / likelihood:** Low-Medium.
- **Remediation:**
  - For the `__main__` spec (zipapp or directory), use `script:` with `abspath(sys.argv[0])`, and take the label from its stem.
  - For `-m`, use `spec.origin`, resolved, instead of cwd.
  - In the scaffold, use `default_app_id(project_dir, slug)` or a required `--app-id`, and drop the `org.ui4me.examples` prefix.
- **Tests:**
  - Two directory apps get distinct IDs, and the label is not `__main__`.
  - `-m` from two cwds gives the same ID.
  - Two scaffolds with the same name get distinct IDs.
- **Complexity:** S.
- **Confidence:** High for zipapp and scaffold; Medium for Windows.

### BIND-09: Calling `Application.start()` twice in one backend process recursively spawns hosts (`instance="multiple"`) or hangs (`"single"`)
- **Classification:** Confirmed defect (misuse, but with a severe consequence).
- **Component:** `_bootstrap.forget_host_environment` (l.64-68) also removes the `UI4ME_DIRECT_START` loop guard. A second `start()` then sees `managed() == False` and calls `launch()` from inside a backend.
- **Reproduction:** `e10_twice.py`.
  - `multiple`: **129 processes started in 8 s** (a host-and-backend chain) until it was killed. The PDEATHSIG chain cleaned everything up afterwards.
  - `single`: a hang, which ends only through the host startup timeout.
- **Severity / likelihood:** Low-Medium, with low likelihood. It can happen when a helper or library also calls `start()`, or in tests.
- **Remediation:** Set a process-local flag in `_bootstrap` when `start()` connects or launches. A second call raises `StartupError("Application.start() was already called in this process")`. Optionally return the existing `Application` instead.
- **Tests:** Call `start()` twice in one backend. Assert `StartupError` and exactly one host.
- **Complexity:** S.
- **Confidence:** High.

### BIND-10: Launcher exit status and diagnostics are wrong when the host dies by a signal; Ctrl+C and SIGTERM give no orderly shutdown
- **Classification:** Confirmed defect (Low) plus a design tradeoff.
- **Component:** `_bootstrap.run_host` l.316 (`return process.wait()` returns `-signum` on POSIX, then `SystemExit(-9)`); the l.317-329 interrupt path.
- **Reproduction:** `e7_signals.sh`.
  - Host SIGKILL gave launcher exit **247**; host SIGABRT gave **250**. The conventional values are 137 and 134. **Nothing was printed** for these.
  - Launcher SIGTERM gave 143 and launcher SIGKILL gave 137. Both are correct, and there were no leftovers.
  - `e12_harness.py`: Ctrl+C to the process group gave exit 130 within 0.01 s. `on_shutdown` did not run and nothing was printed. The backend is killed, not shut down.
- **Severity / likelihood:** Low.
- **Impact:**
  - A host crash, such as SIGSEGV in vendored code, looks like an unexplained exit 245 or 250.
  - Apps that save state in `on_shutdown` lose it on Ctrl+C or a service stop.
- **Remediation:**
  - Map `rc < 0` to `128 + (-rc)` and print `ui4me-host terminated by signal SIGSEGV (this is a UI4ME bug; …)`.
  - Optionally, on SIGINT or SIGTERM, ask the host for an orderly quit. That needs the host to handle SIGTERM by calling `begin_shutdown`, which sends `app.shutdown` with a deadline. Otherwise document that Ctrl+C is a hard stop.
- **Tests:** Kill the host with SIGKILL. Assert exit 137 and the message.
- **Complexity:** S for the mapping; M for the graceful path.
- **Confidence:** High.

### BIND-11: The stale-handle contract leaks as `RpcError -32004` instead of `StaleDocumentError`
- **Classification:** Confirmed inconsistency.
- **Component:** `application.py` `Document.command` and `Window.*`. The docs (`python-api.md:142`) say "every later call on it raises `StaleDocumentError` locally".
- **Evidence:**
  - Until the `closed`/retire notification has been processed (a race), or when it was dropped (BIND-02), host-side staleness surfaces as `RpcError(-32004)`. `e6` observed `RpcError instead of StaleDocumentError: -32004: Window does not exist`.
  - Code that follows recipe 12 (`except StaleDocumentError: return`) then falls into its generic `except Exception` path.
- **Severity / likelihood:** Low, with medium likelihood in multi-threaded apps.
- **Remediation:** Map `-32004` from `document.*`, `window.*` and `bitmap.*` calls on a facade to a new `StaleDocumentError` subclass that also derives from `RpcError` (for example `class HostStaleError(StaleDocumentError, RpcError)`), and mark the facade retired at that point.
- **Tests:** The host returns `-32004` for a command. Assert the exception is an instance of `StaleDocumentError` and that the facade is now retired.
- **Complexity:** S.
- **Confidence:** High.

### BIND-12: Python 3.10 is claimed but not tested: `except TimeoutError` does not catch `call()` timeouts, and `test_bootstrap` cannot run there
- **Classification:** Confirmed (ran on 3.10.22).
- **Component:**
  - `client.py:391-400` raises `concurrent.futures.TimeoutError`, which is a separate class before 3.11.
  - `tools/test_bootstrap.py:16` uses `import tomllib`.
  - `.github/workflows/ci.yml:23-24` tests 3.12 only.
  - `pyproject.toml` claims 3.10-3.14.
- **Reproduction:**
  - `e16_timeout.py` on 3.10 prints `NOT a TimeoutError: concurrent.futures._base.TimeoutError`; on 3.13 it is caught.
  - The docs say "a timeout raises `TimeoutError`" (`python-api.md:452, 587`).
- **Severity / likelihood:** Low.
- **Remediation:**
  - Raise the builtin: `except concurrent.futures.TimeoutError as e: future.cancel(); raise TimeoutError(...) from e`.
  - Make `test_bootstrap` use `tomllib` only when it is available, with a `tomli` fallback or a skip.
  - Add a CI job with a 3.10 and 3.14 matrix that runs the pure-Python tests plus `test_bootstrap` against the built host.
- **Complexity:** S.
- **Confidence:** High.

### BIND-13: No `py.typed` marker, so the type hints are invisible to mypy and pyright for installed users
- **Classification:** Confirmed (packaging).
- **Component:** `python/ui4me_core/` and `pyproject.toml [tool.setuptools.package-data]`.
- **Evidence:** No `py.typed` file exists, and package-data lists only `_bin/*` and skills. The public API is already annotated with `from __future__ import annotations`. Many returns are a bare `dict`; consider `TypedDict`s for `window.state()`, dialog results and `capabilities()` later.
- **Severity:** Low.
- **Remediation:** Add an empty `python/ui4me_core/py.typed` and list it in package-data. Add a `verify_package.py` check that the wheel contains it.
- **Complexity:** S.
- **Confidence:** High.

### BIND-14: Startup DX on Linux: raw systemd errors, an undocumented escape hatch, misleading hints and a stale "v0.1" text
- **Classification:** Confirmed (DX).
- **Component:**
  - `crates/ui4me-host/src/platform/linux.rs:416-461` (`bootstrap_supervised`).
  - `host/requests.rs:251`.
  - The X11 message comes from winit.
  - `docs/guides/troubleshooting.md:52`.
- **Reproduction (`env -i` with an empty `XDG_RUNTIME_DIR`, `e8_hello.py`):**
  - No user bus prints `Failed to connect to user scope bus via local transport: No such file or directory`, exit 1. That is systemd's text with no UI4ME context. The troubleshooting page says starting "fails with a message about systemd user supervision", which only happens when `systemd-run` is missing.
  - No `systemd-run` binary prints `cannot start systemd user supervision; run doctor, or explicitly choose --allow-unsupervised for diagnostics`. A Python or PowerShell direct-start user cannot pass that CLI flag. The working equivalent, `UI4ME_ALLOW_UNSUPERVISED=1`, is documented nowhere in `docs/`.
  - X11-only (`DISPLAY=:99`) prints `… note: enable the winit/x11 feature to support X11`. That is a Cargo-feature hint shown to end users.
  - App with no window prints `RpcError: -32008: v0.1 requires at least one application window`.
  - The good case: with no Wayland and unsupervised, the message `cannot create native event loop (Wayland or interactive Windows desktop required)…` is clear.
- **Severity / likelihood:** Low, with medium likelihood (SSH, containers, WSL, CI without test mode).
- **Remediation:**
  - Before calling `systemd-run`, probe the user manager (`$XDG_RUNTIME_DIR/systemd/private`, or `systemctl --user is-system-running`). Print a UI4ME message naming `UI4ME_ALLOW_UNSUPERVISED=1` and `UI4ME_TEST_MODE=1`.
  - Document both variables in the Python and PowerShell env-var tables.
  - Wrap the winit error: "X11 sessions are not supported; log in to a Wayland session".
  - Fix the "v0.1" wording.
- **Complexity:** S.
- **Confidence:** High.

### BIND-15 (Windows, not run): `pythonw app.py` or a `.pyw` launch would open a console window for `ui4me-host.exe`
- **Classification:** Plausible concern; needs verification on Windows.
- **Component:** `_bootstrap.spawn_host` (l.278-284) uses `subprocess.Popen` without `creationflags`. The host has no `#![windows_subsystem = "windows"]`, so it is a console-subsystem exe. Only the *backend* is spawned with `CREATE_NO_WINDOW` (`platform/windows.rs:882`).
- **Reasoning:** A GUI-subsystem parent (pythonw) that starts a console-subsystem child without a console gets a new console window allocated for that child. Running `.pyw` through pythonw is the standard way to ship Windows Python GUI apps.
- **Severity / likelihood:** Low-Medium (cosmetic but very visible), with medium likelihood on Windows.
- **Remediation:** On `os.name == "nt"`, pass `creationflags=subprocess.CREATE_NO_WINDOW` when `GetConsoleWindow()` returns 0 or `sys.stdout is None`. stderr is piped anyway. Verify on winDEV with an interactive session.
- **Complexity:** S.
- **Confidence:** Medium.

### BIND-16: API surface gaps and inconsistencies (Python)
- **Classification:** Design and maintenance.
- **Evidence:**
  - `create_window` has no `resizable` (l.98-114), although the protocol and PowerShell (`New-UI4MEWindow -Resizable`, psm1:528/543) do. A Python app must create the window and then call `window.set(resizable=False)`.
  - `open_file(path, **window_options)` cannot pass `asset_root`, while `open_html` and `load_file` can.
  - Listener semantics differ:
    - `on_window_event` supports many listeners and returns an unsubscribe function.
    - `on_desktop_preferences` silently *replaces* a single slot and returns `None`. It also uses `rpc.on_notification`, which shares the namespace the docs warn about.
    - `on_before_quit` replaces.
    - `on_shutdown` appends and returns `None`.
  - There is no `on_activated` (BIND-07).
  - No typed API exists, in either binding, for `desktop.paths`, `host.diagnostics`, `app.force_quit`, `window.list` (Python only; PowerShell has `Get-UI4MEWindowList`), or the text-selection ops (`get_selection`, `select_text`, `clear_selection`, `copy_selection`).
  - There is no common base exception. `RpcError`, `ProtocolError`, `DisconnectedError(ConnectionError)`, `StaleDocumentError(RuntimeError)` and `StartupError(RuntimeError)` are unrelated, so `except UI4MEError` is impossible.
  - `doc.on("clik", …)` accepts any string, so a typo silently never fires.
  - `app.run()` called again, or from a callback, runs the `on_shutdown` hooks twice. In the callback case, the drain waits 2 s for itself.
- **Severity:** Low.
- **Remediation:**
  - Add `resizable` and `open_file(asset_root=)`.
  - Make every `on_*` multi-listener and return an unsubscribe function, keeping one-slot policies for `before_*`.
  - Add `UI4MEError` as a base class for all of them.
  - Optionally warn on unknown event types using the list in `html-css-scope.md`.
  - Add `on_activated` and `desktop_paths()`.
- **Complexity:** S-M.
- **Confidence:** High.

### BIND-17: Issue #1 from the client side (socketpair microbenchmark): Python drains fast when idle, about 25x slower under GIL contention, and no client can absorb an instantaneous burst. Windowed measurements are in BIND-19.
- **Classification:** A reproducible risk (measured) plus analysis.
- **Component:** `client.py` `_reader`/`read_exact`, which does two `recv` calls per frame and decodes on the reader thread.
- **Measurements** (`e9_drain.py`, `e9b_buffered.py`, socketpair, 360-byte `pointermove` frames):
  - Uncontended: about 80,000 frames/s. Even then, a trivial ordered callback dropped 150 of 20,000 because the event worker is slower than the reader.
  - With one CPU-bound Python thread: about 2,800-3,400 frames/s, with 22-28% dropped.
  - A 64 KiB buffered reader improved this only to about 4,100 frames/s, so GIL contention dominates.
- **Host-side arithmetic:**
  - The Windows host creates the pipe with 64 KiB buffers (`platform/windows.rs:269-270`), and the host's per-connection queue holds 128 messages (`transport.rs:17`). `Host::send` disconnects on `try_send` failure (`host/mod.rs:442-447`).
  - A burst of about 300 × 400 B (120 KB) produced in one GUI turn fills the pipe (about 160 frames) and then the queue (128) in microseconds. That is faster than *any* client can read.
  - So issue #1 can disconnect a Python backend on Windows too, regardless of client speed.
  - *Correction after measuring (BIND-19):* on Linux the AF_UNIX buffer holds only about 104 of the host's two-write frames, not about 600, so Linux fails at about 240-frame bursts as well.
- **Severity / likelihood:** Medium for Windows apps with many transitioned elements (host-side defect). Low for the Python client itself.
- **Remediation:** The fix must be host-side (not PR #3 as written; see HOST-02): an allowlisted drop/coalesce of non-semantic high-frequency types, and better per-document subscription filtering. On the client side, keep the non-blocking reader (do not change it), and add the typed drop policy (BIND-02). Optionally parse on the reader only enough to classify and defer the full `json.loads` work. That has low value.
- **Complexity:** Host M; client S.
- **Confidence:** Medium-High. The arithmetic follows from the code; I could not run it on Windows.

### BIND-18: sdist fallback on unsupported platforms
- **Classification:** Proven from code (Low).
- **Component:**
  - `setup.py` `BuildWithHost.run`.
  - `publish.yaml`, which publishes the sdist alongside `manylinux_2_34_x86_64` and `win_amd64` wheels.
- **Evidence:** `pip install ui4me-core` on aarch64 Linux, musl, glibc older than 2.34 (Debian 11, Ubuntu 20.04, RHEL 8) or macOS falls back to the sdist.
  - macOS fails with a clear `RuntimeError`, but only *after* copying the skill.
  - Linux without cargo shows a raw `FileNotFoundError: 'cargo'` inside pip's build traceback.
  - Linux with cargo silently starts a heavy release build (Stylo) with `CARGO_BUILD_JOBS=2` of an unqualified host, for example for ARM64.
  - `troubleshooting.md:46` mentions this, which is good.
- **Remediation:**
  - At the top of `run()`, check `platform.machine()` and `shutil.which("cargo")` and fail fast with a single clear message.
  - Gate unqualified architectures behind `UI4ME_BUILD_UNSUPPORTED=1`.
- **Complexity:** S.
- **Confidence:** High.

## 3b. PowerShell findings (fork, full text in `exp/BIND/ps/PS_FINDINGS.md`)

How the fork worked:
- Pester 6.2 on pwsh 7.7 for Linux: 54 of 54 tests passed, including 4 native headless direct-start tests.
- It used a Python mock peer (`exp/BIND/ps/mockpeer.py`) and the real headless host.
- There was no Windows PowerShell 5.1 and no Windows machine, so Windows claims are reasoned from code.
- I re-checked the key evidence for PS-01 (`Transport.cs:144-146`, `new PSNoteProperty(member.Key, …)`) and PS-03 (`psm1:283-291`); both match the code.

| ID | Severity | Title | Classification |
| --- | --- | --- | --- |
| BIND-PS-01 | Medium | A host JSON member named `psbase`/`psobject`/`psadapted`/`psextended`/`pstypenames`, or an empty name, makes `PSNoteProperty` throw on the reader thread, so the connection is lost. Example: an element with a `psbase` attribute plus `get_attributes` kills the app. | Confirmed (real host) |
| BIND-PS-02 | Medium | Admission (128 messages) is first-come. The PowerShell pump costs about 0.94 ms per subscribed event and 0.65 ms per unsubscribed one. A 300-event burst dropped 173, *including the following click* (issue #1, PowerShell side). Fix: filter by interest on the reader thread, and drop coalescible types first. | Confirmed (mock) |
| BIND-PS-03 | Medium-Low | The module's own deferral cap (`psm1:286`, 128) can drop `window.event closed`, which the transport had deliberately admitted, so the window is never marked closed (the same class of bug as Python BIND-02). | Confirmed (mock) |
| BIND-PS-04 | Low-Medium | If shutdown arrives during a callback, `Wait-UI4ME -UntilShutdown` returns with the final deferred events not delivered. Python drains them for up to 2 s. | Confirmed |
| BIND-PS-05 | Low-Medium | `Show-UI4MEFileDialog -NoWait` followed by `Wait-UI4MERpc` with its default 15 s timeout cancels the user's open dialog. | Confirmed (mock) |
| BIND-PS-06 | Low-Medium | `Start-UI4MEScript` uses `$MyInvocation.PSCommandPath`. Called from a helper or module function, it re-runs the wrong file and fails with "Backend exited with exit status: 0". | Confirmed (real host) |
| BIND-PS-07 | Low | Ctrl+C makes the launcher exit 0, not the documented 130. The Python launcher returns 130. | Confirmed (Linux) |
| BIND-PS-08/09 | Low | `break`/`continue` in a callback escapes into the module's loops: it skips the other handlers, and in a policy it sends `result:null`, which the host treats as a veto. A policy's last output is coerced with `[bool]`, so the string `'false'` allows. | Confirmed |
| BIND-PS-10 | Low | Writes are synchronous on the runspace thread. A request with a 1 s timeout blocked for 6 s when the peer stopped reading. | Reproducible risk |
| BIND-PS-11…19 | Low | 11: an AllUsers Linux install gets `chmod u+x` only, so other users cannot run it (plausible). 12: `[switch]` and `[xml]` serialization. 13: lists come back as one pipeline object; errors are plain `throw`. 14: Get/Set pairs don't line up, some nouns are plural, `Register-UI4MENotification` keeps one callback per method. 15: no guard against calls from a thread job ("Duplicate request ID"). 16: `Add-Type` costs about 0.3-0.5 s per process, twice per direct start. 17: callbacks resolve variables through the pumping call stack. 18: Windows output encoding (unverified). 19: `Start-UI4MEHost` prints raw JSON logs. | Mixed |

**Cross-binding pattern.** Both bindings independently have:
- A bounded admission or deferral queue that is blind to message type and can drop `closed` bookkeeping (BIND-02 / PS-03).
- An indiscriminate drop policy under DOM-event bursts (BIND-02 / PS-02).
- Silent or terse failure in user callbacks (BIND-01/03 / PS-08/09).

Fix them once, as a shared design rule written into `protocol.md` "Writing a binding: checklist": control notifications are never dropped, bookkeeping is applied eagerly, coalescible events are dropped first, and drops are logged.

### BIND-19: Issue #1 measured in a real windowed session (nested `kwin_wayland --virtual`, socket `ui4me-audit-0`): both readers drain at about 35-45k frames/s, yet bursts of about 240 or more messages disconnect the backend. The host's two writes per frame halve the kernel buffering.
- **Classification:** Confirmed defect (host-side) with measured client behaviour.
- **Setup:** `exp/BIND/burst/`.
  - Probes: `burst_py.py` (Python, with a reader-thread timestamp hook) and `burst_ps2.ps1` (PowerShell, using a *scratch copy* of the module with one `Stopwatch` timestamp line in `ReadLoop`).
  - Both used the published 1.0.1 release host (`exp/main/venv/.../_bin/ui4me-host`), the repo bindings, and `WAYLAND_DISPLAY=ui4me-audit-0`.
  - The page has N buttons, each with 3 transitioned properties, and the colour scheme is switched once per round.
- **Measurements:**
  - **Burst shape.** Each switch emits 18 `document.event` frames per button. The first cluster is 12 per button (N=20 gives 240 frames); the second follows about 107-125 ms later.
  - **Python reader:** 120 frames in 3.4 ms (about 35k/s) at N=10, and 240 frames in 5.2-6.1 ms (about 39-46k/s) at N=20. No client-side drops (`dropped=0`) in passing runs.
  - **PowerShell C# reader:** 120 frames in 2.8-3.9 ms (about 31-42k/s) at N=10. So the C# reader is as fast as Python's. Parsing happens on the reader thread (`Json.Parse`, which builds a `PSObject`), and the hand-off is an unbounded `BlockingCollection` plus first-come admission at 128. Neither reader blocks on user code.
  - **Thresholds (Python):**
    - N=20 (240-frame cluster): passed 12/12, then 6/12 (failed at round 6), then 4/4. **Intermittent.**
    - N=20 with **no subscriptions at all**: 12/12 passed. The host sends the events anyway, so subscriptions don't matter.
    - N=30 and N=50: failed on the **first** burst, with or without subscriptions.
    - The failure is host-side: `peer cannot receive bounded output (connection outgoing queue unavailable: sending on a full channel)`, then `application failed (Backend connection lost: closed by host)`.
    - After that, desktop mode keeps an error window open until the launcher times out.
  - **Thresholds (PowerShell):**
    - N=10: 4/4 passed.
    - N=20: failed on the first burst in the first attempt (`ps20.out`: `ROUND_OK 0`, then `application failed (Backend connection lost: closed by host)`).
  - **Kernel buffering** (`python3` socketpair experiment, recorded in this file): the host's writer thread does `write_all(header)` and then `write_all(body)` (`transport.rs:301-305`). On Linux each write is a separate skb, and AF_UNIX accounts the send buffer per skb, so only **104 frames** fit before EAGAIN. A single write of header plus body fits **167**. Both counts are independent of frame size from 300 to 600 B, with `wmem_default=212992`.
- **Burst capacity:** before `Host::send` disconnects, the burst capacity is about 104 (kernel) + 128 (`MAX_OUTBOUND_MESSAGES`) plus whatever the client reads while the GUI thread is still producing. That is about 230-250 frames, which matches the N=20 edge exactly.
  - Windows uses a 64 KiB pipe buffer (`platform/windows.rs:269-270`), which is the observed Windows failure. I did not run Windows.
- **Conclusion for the bindings:**
  - Neither client is the bottleneck, and no client-side speed-up can absorb a burst that the GUI thread produces in about 1-5 ms into a 232-slot pipeline.
  - PowerShell's later pump cost (PS-02) causes *drops*, not the disconnect.
- **Remediation (host, in priority order):**
  1. Never disconnect on notification overflow. Drop or coalesce notifications (an allowlisted drop of non-semantic high-frequency types only, as in HOST-02's quick-patch note; not PR #3 as written), keep responses and lifecycle requests, and emit one `document.error` or diagnostic counter.
  2. Make the outbound cap byte-based for notifications (the 8 MiB already exists). The 128-message count is what binds for 400-B events.
  3. Send one write per frame, as one buffer or `write_vectored`. This raises Linux kernel slack from 104 to 167 frames. It is trivial and also halves syscalls.
  4. Add subscription filtering: the bindings already know their event types (Python `Document._handlers`, PowerShell `Register-UI4MEDocumentEvent`) and could send `document.subscribe`. That is a protocol addition, so bump `capability_schema`.
- **Remediation (client side):** Keep the readers non-blocking. Add the typed drop policy (BIND-02 / PS-02). Do not add client backpressure.
- **Test:** A windowed CI smoke test (nested compositor) with N=50 transitioned buttons and 4 scheme switches must stay connected. A host unit test fills the outbound queue with notifications and asserts no disconnect.
- **Severity / likelihood:** High, with high likelihood for theme-aware apps with 20 or more transitioned controls. This is the coordinator's issue #1, now quantified.
- **Confidence:** High on Linux. On Windows the failure was reproduced by the coordinator (experiments E3/E4: N=20 fails with Python 3.13 and Windows PowerShell 5.1.20348).

## 4. Parity table (protocol ↔ Python ↔ PowerShell)

Legend: **T** = typed API; **G** = only through the generic call (`app.rpc.call` / `doc.command(op)` / `Invoke-UI4MERpc` / `Invoke-UI4MEDocumentCommand`); **—** = not reachable in a natural way.

| Protocol method / op | Python | PowerShell | Notes |
| --- | --- | --- | --- |
| `host.hello` | T `from_environment`/`start` | T `Connect-UI4ME`/`Start-UI4MEScript` | |
| `host.capabilities` | T `app.capabilities()` | T `Get-UI4MECapabilities` | |
| `host.diagnostics` | G | G | |
| `app.ready` | T `ready()`/`run()` | T `Set-UI4MEReady` | |
| `app.request_quit` | T | T `Request-UI4MEQuit` | |
| `app.force_quit` | G | G | |
| `request.cancel` | T `future.cancel()` | T `Stop-UI4MERpc` | PS `Wait-UI4MERpc` defaults to 15 s, Python waits indefinitely (PS-05) |
| `window.create` | T `create_window` (**no `resizable`**) | T `New-UI4MEWindow -Resizable` | default title "UI4ME" vs `''`; default height 600 vs host 560 |
| `window.get` / `window.set` | T `state()` / `set(**)` | T `Get-`/`Set-UI4MEWindow` | |
| `window.list` | G | T `Get-UI4MEWindowList` | |
| `window.request_attention` / `window.request_close` | T | T | |
| `window.before_close` (host→) | T `on_before_close` (per window) | T `Set-UI4MEBeforeClose` | error or falsy means veto in both; Python bool(); PS [bool] of last output (PS-09) |
| `app.before_quit` (host→) | T `on_before_quit` (replace) | T `Set-UI4MEBeforeQuit` | |
| `app.shutdown` (host→) | T `run()` returns, `on_shutdown` | T `Wait-UI4ME -UntilShutdown` | PS drops final deferred events (PS-04) |
| `app.activated` (host→) | G `rpc.on_notification` | G `Register-UI4MENotification` | args are the host's argv, not the script's (BIND-07) |
| `request.cancelled` (host→) | ignored | G | |
| `window.event` (host→) | T `on_window_event` (many + unsubscribe) | G `Register-UI4MENotification` (one callback) | both can drop `closed` (BIND-02 / PS-03) |
| `desktop.preferences_changed` (host→) | T `on_desktop_preferences` (single slot, replaces) | G | |
| `document.load` / `unload` | T `load_*`/`open_*`/`unload` | T `Open-UI4MEHtml`/`File`, `Remove-UI4MEDocument` | Python `open_file` lacks `asset_root` |
| `document.get` | — | — | |
| `document.event` / `document.error` | T `doc.on(type, element=)` / `doc.on("error")` | T `Register-UI4MEDocumentEvent` | PS shares one mutated event object across handlers; Python gives each a copy |
| text/value/checked/attribute/class ops | T | T | |
| `get_attribute`, `get_attributes`, `has_element`, `get_html` | G | G | PS-01 risk on `get_attributes` |
| `replace_children_html`, `insert_html`, `remove_element` | T | T | |
| `append_text`, `move_element`, `trim_children` (id form) | G (typed on `Node`) | G (typed via node op) | |
| `batch` / `node_batch` | T | T | |
| `query` / `query_nodes` / `node_of` / `get_node` / `node_children` | T | T | |
| `element_at_point` | T | G | |
| `text_at_point`, `text_rects`, `get_rect` | G | G | |
| stylesheets (`add/replace/remove/list`), `get_computed_style` | T | T | |
| `css_profile`, `explain` | G | G | |
| `focus`, `blur`, `get_focused_element` | T | T | |
| `get/set_control_selection`, `replace_control_range` | T / T / T | T / T / G | |
| `edit_command` | T `edit()` | T `Invoke-UI4MEEditCommand` | |
| `get_selection`, `clear_selection`, `select_text`, `copy_selection` | G | G | text selection is untyped in both |
| forms: validity/custom validity/check | T | G | |
| forms: `reset_form`, `request_submit`, `get_form_data`, `get_options`, `set_selected_values`, `get_files` | T | T (except `get_files` G) | |
| `set_default_action` | T `prevent_default` | T `Set-UI4MEPreventDefault` | |
| `get_scroll`, `scroll_to` / `scroll_by`, `scroll_into_view`, `set_scroll_follow` | T, T / G, T, G | T, T / G, G, G | |
| `show/close_dialog`, popovers | T | T | `get_dialog`/`get_popover`: G in both |
| `set/get_pointer_capture` | G | G | |
| `set/get_environment` | T | T | |
| resource bundles / stats / purge | T | T | |
| `get_accessibility_tree`, `get_diagnostics` | G, T | G, T | |
| `bitmap.create/present/close` | T `LiveBitmap` | T `New-`/`Send-`/`Remove-UI4MELiveBitmap` | |
| `clipboard.read_text`/`write_text` | T | T | |
| `dialog.*` | T `open_files`/`save_file`/`choose_folder` (`wait=False` gives a Future) | T `Show-UI4MEFileDialog -Kind` (`-NoWait`) | |
| `desktop.open_external` / `reveal_file` | T | T `Open-UI4MEExternal [-Reveal]` | |
| `desktop.paths` | — (G) | — (G) | not typed anywhere |
| `desktop.preferences` | T | T | |
| Direct start | `Application.start()` | `Start-UI4MEScript` | Python fails when `start()` is called twice (BIND-09); PS fails when called from a helper (PS-06) |
| Ctrl+C exit status | 130 | 0 (PS-07) | |

The parity table in `python-api.md:556-585` is accurate as far as it goes, except that it calls `Get-`/`Set-UI4MESelection` the text selection; it is the *control* selection. It omits the gaps listed above.

## 5. Test gaps and highest-value tests to add

1. **Callback robustness (BIND-01/03).** A unit test using the `FakeRpc`-style harness in `tools/test_application.py`:
   - `SystemExit` in an event callback; the next event is still delivered and a quit is requested.
   - `SystemExit` or an exception in a policy; a deny response is sent and the workers stay alive.
   - A NaN or `object()` result gives a `-32603` response.
   - Two handlers for one event where the first raises: the second still runs.
   - The traceback appears on stderr.
2. **Typed drop policy (BIND-02, PS-02/03).** Block the worker, then 300 DOM events plus `window.event closed` plus a `click`. `closed` and `click` must be delivered, the drops logged, and `win.closed` set. Run it in both bindings against `MockHost`.
3. **`host_binary` self-recursion (BIND-04).** Put a wrapper named `ui4me-host` on PATH and use a package without `_bin`. Expect a fast, clear error.
4. **Direct-start output fidelity (BIND-05).** 50k printed lines must all be relayed in order (headless).
5. **Early-failure ergonomics (BIND-06).**
   - A failing first `open_html`, then a fallback, then `run()` returns.
   - `request_quit()` before `run()` gives exit 0.
6. **Single instance (BIND-07/08).**
   - A secondary prints a notice and passes its script args.
   - Zipapp and directory apps get distinct identities.
   - Scaffold app IDs are unique per project.
7. **`start()` twice (BIND-09).** Expect `StartupError` and exactly one host process.
8. **Signal exit mapping (BIND-10).** Host SIGKILL gives launcher exit 137 plus a message.
9. **`-32004` gives `StaleDocumentError` (BIND-11).**
10. **Python version matrix (BIND-12).** CI on 3.10 and 3.14 (incl. 3.14t) for the pure-Python tests and `test_bootstrap`. Make `tomllib` optional in tests.
11. **Packaging.** `verify_package.py` asserts `py.typed` (BIND-13) and the console-script/host separation.
12. **Windowed burst regression (BIND-19).** Run in the nested compositor, plus a host unit test that notification overflow never disconnects.
13. **Windows-only, to run on winDEV.**
    - The `pythonw`/`.pyw` console window (BIND-15).
    - The identity of pip console scripts (BIND-08).
    - Named-pipe burst capacity (BIND-19).
    - `NamedPipeStream` under burst. `test_real_named_pipe_unicode_and_cancellation_of_blocked_read` is the only named-pipe test; check whether CI's `windows-2022` job actually runs it through `verify.py`.

## 6. Things I recommend NOT changing

- **The non-blocking reader with no backpressure into the host.** Adding backpressure, for example a blocking `put` on the callback queue, would turn every slow callback into a host-side disconnect (BIND-19).
- **The separate lifecycle-request pool next to the single ordered event worker.** It is the reason a dialog-waiting callback can't block close or quit. Do not merge them.
- **Fail-safe policy semantics** (error or false means veto). Make them *loud* (BIND-03), not permissive.
- **Generation-bound facades and local stale checks.** Extend them (BIND-11); don't remove them.
- **Strict framing and codec rules** (duplicate keys, NaN, size before body, string IDs) in both clients. The PowerShell custom codec rather than `ConvertFrom-Json` belongs here too.
- **The dual-role `Application.start()` / `Start-UI4MEScript` design.** It is the right DX. Add guards (BIND-04/09, PS-06) rather than replacing it with a mandatory manifest.
- **Scrubbing `UI4ME_ENDPOINT`/`TOKEN` after connecting**, so child UI4ME apps work. Add a process-local flag for BIND-09 instead of keeping the env marker.
- **The `launcher.main` POSIX `execv`**, and Python's 130/143 mapping for launcher signals.
- **`py3-none-<platform>` wheels with the host as data.** There is no reason to build per-ABI wheels.

## 7. Open questions

- **Windows behaviour I could not run:**
  - BIND-15: console window under pythonw.
  - BIND-08: identity of console-script launchers.
  - `NamedPipeStream` throughput.
  - Whether NPFS byte-mode quota charges per write, which would make the host's two writes per frame worse there too.
- **Should a second launch's script args be carried in `app.activated`?** That needs a host and protocol decision: either an additive field or a `capability_schema` bump.
- **Should the host handle SIGTERM/SIGINT as an orderly quit** (`begin_shutdown`) so that `on_shutdown` runs? Today it is a hard stop in both bindings.
- **What is the intended policy for `print()` in direct start:** a lossless console, or a capture for the failure window? This decides between BIND-05 option A and option B.
- **Is the 18-events-per-button-per-switch burst shape expected?** It is 12 immediately, then 6. If the host emits duplicate transition events, for example per pseudo or per phase, that is another host-side multiplier worth checking (HOST area).
