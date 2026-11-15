# HOST area audit — ui4me-host (commit c6fd71d, 1.0.1)

> **Coordinator note.** HOST-01 is consolidated as F-02 and re-rated Critical after reproduction with the stock Python and PowerShell 5.1 clients on Windows and Linux windowed. `<scratch>/` paths were session-temporary; key scripts are retained in [../evidence/scripts/host](../evidence/scripts/host/).


## 1. Scope & method

Read (targeted, most files fully): `crates/ui4me-host/src/{transport.rs, protocol.rs, lifecycle.rs, backend.rs, main.rs, config.rs (validation part), logging.rs, accessibility.rs (remap), presentation.rs (canvas)}`, `host/{mod.rs, requests.rs, documents.rs, document_native.rs (events/refresh/poll), native.rs (clipboard, ApplicationHandler), bitmap.rs (transport file)}`, `platform/{linux.rs, windows.rs, preferences.rs, dialogs_windows.rs, external.rs (Windows part)}`; all `unsafe` sites (148: linux 26, windows 65, dialogs_windows 36, preferences 13, external 7, documents 1 test-only). Docs compared: `docs/reference/protocol.md`, `docs/reference/host-cli.md`, `docs/concepts/architecture.md`, `docs/concepts/lifecycle.md`. Clients consulted for end-to-end semantics: `python/ui4me_core/client.py`, `application.py`, `powershell/UI4ME.Core/private/UI4ME.Transport.cs` (ReadLoop). PR #3: `git diff main...origin/fix/drop-saturated-notifications` (681b879). Existing tests: Rust unit tests in transport/protocol/lifecycle/host/platform; `tools/run_faults.py` + `examples/python/faults.py` modes (veto-close, veto-quit, freeze-close, freeze-quit, crash-after-ready, log-flood, managed-tree).

Experiments (all headless, Linux; no desktop windows; Windows only by code reading). Host binary = copy of `target/debug/ui4me-host` built from the clean tree (debug: workspace crates opt-level 0, so GUI-thread CPU numbers are pessimistic). Scripts in `<scratch>/exp/HOST/`:
- `adv_backend.py` — raw-socket adversarial backend (scenarios below), `adv_run.py <scenario> [ENV=..]` — writes manifest, runs `ui4me-host run --app … --headless`, prints evidence + WARN/ERROR lines. Run dirs under `.../exp/HOST/runs/`.
- `sigterm_test.py SIGTERM|SIGINT`, `secondary_test.py`, `fd_test.py <nofile-limit>`.

Results (exact observations):
| Scenario | Observed |
|---|---|
| `auth` | Pre-hello `window.create`/`app.ready`/`host.diagnostics` → -32011. Bad version → -32010. Wrong/short/non-string token → -32011, connection kept. `host.hello` with an extra unknown param accepted. Second connection: `app.activate` before ready → -32002; after the client authenticated, second `hello` → -32005 BUSY; non-client `window.create` → -32011; `app.activate` with 40 KB args → -32602; accepted activation delivered `app.activated` to client. Idle unauthenticated connection closed after 5.01 s. 12 simultaneous extra connections → 5 closed immediately (8 total admitted). 200 token-less `app.activate` connections → 200 `app.activated` delivered in 1.67 s. Exit 0. |
| `malformed_secondary` | zero length, 2 MiB+1, 0xFFFFFFFF header, invalid UTF-8, invalid JSON, truncated header/body, 200-deep array, 100000-deep object → that connection closed immediately, client healthy. Batch array / numeric id / NUL in method → JSON-RPC error reply, connection kept until 5 s expiry. ~2 MiB legal frame (`host.diagnostics` with a 2 MiB junk param) accepted and answered. Exit 0. |
| `malformed_client` | invalid JSON on the authenticated connection → "Backend connection lost: invalid/failed frame…", exit 1 in 0.22 s (documented). |
| `invalid_response` | unknown-id response ignored; response with both result+error / without id → WARN, ignored; error answer to `window.before_close` = veto; re-close coalesced; `app.request_quit` superseded close with `request.cancelled`; `window.create` during quit preflight → -32005; stale close answer ignored; veto quit → Running; after allow: `app.shutdown`, `window.create` → -32008, `host.diagnostics` OK; `app.force_quit` → exit 1 with log "backend exceeded shutdown deadline". |
| `flood_no_read` | 519 pipelined `host.diagnostics` without reading (40 ms) → "connection outgoing queue unavailable: sending on a full channel" → "Backend connection lost: closed by host", exit 1 after 0.23 s, then 214 WARN "connection is closed" lines. |
| `slow_reader_burst` | 600 `request_submit` notifications → host disconnected backend while it was still sending (exit 1). |
| `fast_reader_single_command_burst` (one `request_submit` on a form with N empty `required` inputs → N `invalid` events + focus/focusin, ~595 B each) | Dedicated reader, no delay: N=100/200/400/1000 all delivered (debug host produces events slowly). Reader at 2 ms/message: N=150 OK, N=250 and N=400 → "closed by host", exit 1. Command latency on the GUI thread (debug): N=100 0.39 s, 400 1.15 s, 1000 11.96 s, 2000 ≈100 s (host noticed the backend's disconnect only after 112 s; then 1025 WARN lines). |
| `window_churn` (N=1000) | create+load+request_close(allow) cycles: RSS 45.4→46.5 MiB flat; 1000 `document.load` on one window: 47.1→47.7 MiB; stale `document_id` → -32004. |
| `no_hello` (startup 3 s) | fail "did not authenticate…" at 3.07 s, exit 1. |
| `shutdown_ignore` | backend ignores `app.shutdown` → killed at 5 s deadline, exit 1. |
| `quit_reentrancy` | 3 closes allowed, last-window close → `app.before_quit`; veto keeps window; force quit exit 1. |
| `endpoint_token_relation` | instance=multiple: endpoint basename == first 24 hex chars of UI4ME_TOKEN (True); single: unrelated. |
| `sigterm_test.py` | SIGTERM → host exit -15 in 3 ms, backend received nothing; SIGINT → -2. Socket file left behind each time. |
| `secondary_test.py` | Second launch while primary is in `quitting` → exit 1 in 0.01 s: "Primary instance is shutting down (RPC -32004)". |
| `fd_test.py 19` | RLIMIT_NOFILE 19, flood connections → `accept()` EMFILE → accept thread exits, socket file removed, no log line; later connect → ENOENT while host+backend keep running. (Limits 20 and 1024: listener survives.) |
| systemd env (non-headless, nonexistent manifest so no window can open) | `env 'BASH_FUNC_foo%%=…' ui4me-host run --app /nonexistent` → "Cannot assign environment variable BASH_FUNC_foo%%: Invalid argument", exit 1 (baseline: "Cannot locate application manifest"). Non-UTF-8 value `LATIN=caf\xe9` → "Failed to create bus message: Invalid argument", exit 1. systemd 257. |

Coordinator evidence incorporated (not run by me): release 1.0.1 wheel, `exp/main/burst2.py` (N buttons × 3 transitioned properties, colour-scheme toggle): Windows RDP fails for N≥20; Linux windowed (nested kwin) fails N=20 on 3rd switch (360+ events/burst), N=100 on 1st; then 61–351 WARN "connection is closed" lines; headless emits 0 transition events.

Cleanup: removed the 4 stale sockets my killed hosts left in `$XDG_RUNTIME_DIR/ui4me-host`; no host processes left; repo `git status` clean; no worktrees created.

## 2. Strengths to preserve

- Framing: length checked before allocation (`protocol.rs:71-82`), EOF-in-frame is an error, serde recursion limit stops deep nesting, strict JSON-RPC subset (string IDs, method charset, no batches). All malformed frames on a secondary connection only close that connection (reproduced).
- Authentication: 256-bit `getrandom` token, length-and-content constant-time compare (`host/mod.rs:830-833`), exactly one client, BUSY on re-hello, UNAUTHENTICATED for pre-hello requests, 5 s expiry of unauthenticated connections, 15 s first-frame timeout, admission limit counting pending terminal events (`transport.rs:146-156, 276`).
- Linux endpoint hygiene: runtime dir verified (not symlink, owner = euid, no group/other bits), lock file `O_NOFOLLOW|O_CLOEXEC`, stale socket removed only after proving `ECONNREFUSED`, nonblocking connect with deadline, per-call I/O deadlines.
- Process control: `waitid(WNOWAIT)` before group SIGKILL to avoid PID-reuse races (`linux.rs:658-694`), async-signal-safe `pre_exec` with the `getppid` check, reaper thread so the GUI never waits, pidfd + start-time launcher watch.
- Windows: job assigned atomically via `PROC_THREAD_ATTRIBUTE_JOB_LIST`, explicit inherit handle list, `KILL_ON_JOB_CLOSE`, `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`, protected logon-SID DACL, `Pipe` in-flight accounting so no handle is used after close, every cancelled overlapped op waited for (`windows.rs:527-584`). Drop orders in the COM dialog/STA code are correct. The unsafe inventory is sound as far as I can tell (no lifetime/alignment/NUL issues found).
- Poisoned mutexes are tolerated everywhere (`unwrap_or_else(into_inner)`); dialog/external workers `catch_unwind`.
- Lifecycle/decision logic is a pure, unit-tested state machine; decisions are cancelled with their window, superseded with `request.cancelled`, duplicate pending IDs fail the session.
- Bounded, lossy, non-blocking logging and backend log ring.
- No host-level growth in headless window/document churn (see table).
- Accessibility ID remapping validates every reference before indexing.

## 3. Findings

### HOST-01: Healthy backend is disconnected (app fails) whenever host output bursts past an instantaneous 128-message depth (issue #1 root cause)
- Classification: Confirmed defect (reproduced headless with a 2 ms/message reader and a pipelined client; reproduced by coordinator windowed on Linux and Windows with the stock Python client).
- Component: `transport.rs` `Server::reply` (`:195-222`), `host/mod.rs` `Host::send` (`:442-449`), `document_native.rs` `emit_document_events` (`:214-242`); renderer root capture listeners for every event type (`crates/ui4me-renderer/src/document.rs:~360-404`).
- Evidence: `try_send` on a `sync_channel(128)` fails → `server.disconnect(connection)` → `Disconnected` → `fail("Backend connection lost…")`. The renderer registers capture listeners on the root for pointermove/mousemove/wheel/scroll/transition*/animation*/…, so every DOM event is a host notification regardless of what the app subscribed to (subscription exists only client-side: `application.py:450-481`). The producer is the GUI thread emitting a whole burst in one turn; the writer thread can only move data as fast as the kernel buffer accepts: Linux ~100 frames (each frame is two `write` calls — see HOST-09 — so skb overhead dominates the 208 KiB buffer), Windows 64 KiB pipe quota (`windows.rs:269-270`). Thresholds observed: 2 ms/msg reader: 152 events OK, 252 fail. Release build (coordinator): ~360 transition events per theme switch already fail on Linux windowed with a trivial Python handler. Pipelined requests without reading: 519 requests → fail in 0.23 s.
- Reproduction: `python3 adv_run.py fast_reader_single_command_burst ADV_N=250 ADV_READ_DELAY=0.002`; `python3 adv_run.py flood_no_read`; coordinator `exp/main/burst2.py`.
- Severity: High. Likelihood: High for any page with CSS transitions/animations on many elements, theme switches, pointer-heavy UIs, or Windows/PowerShell backends.
- Impact: Application failure ("application error" window, exit 1) caused by normal UI activity; data loss of unsaved state.
- Root cause: (1) unconditional emission of every DOM event; (2) depth-based (not stall-based) disconnect policy; (3) no separation between droppable/coalescable events and reliable traffic; (4) small effective kernel buffering.
- Remediation (target design; can be staged):
  1. Reliable lane vs event lane in `transport.rs`. Replace the single `SyncSender<Vec<u8>>` per connection with a `Mutex<Outbound>{ reliable: VecDeque, events: VecDeque/IndexMap, bytes }` + `Condvar`. Reliable = responses, host→backend requests (`window.before_close`, `app.before_quit`), and lifecycle notifications (`app.shutdown`, `window.event` `closed`/`close_requested`/`focused`, `request.cancelled`, `document.error`, `app.activated`). Writer drains reliable first.
  2. Never disconnect on depth. Disconnect only on stall: writer made no progress for N s (keep the 10 s write deadline) or the reliable lane exceeds a hard byte cap (e.g. 16 MiB). For pipelining peers apply flow control instead: when a connection's reliable lane is above a high-water mark, its reader thread stops reading (same mechanism as the inbound backpressure in `emit`), so a client that pipelines without reading slows down instead of dying (fixes `flood_no_read`).
  3. Host-side subscription filtering: protocol capability negotiated in `host.hello` (client sends `"event_subscriptions": true`), plus `document.command {op:"set_event_types", types:[…]}` (or per-element). Bindings already know their handler set (`Document._handlers`, PowerShell equivalent) and update it on `on()`/unsubscribe. Without the capability keep v1 behaviour. Ideally pass the set into the renderer so root listeners are only installed for subscribed types (saves GUI CPU too).
  4. Coalescing for continuous types in the event lane: latest-wins per (document, type, target) for pointermove/mousemove/wheel/scroll, `window.event resized/scale_changed`, `desktop.preferences_changed`; transition*/animation* only if subscribed.
  5. Explicit loss signalling: add a monotonically increasing `seq` to `document.event`; if discrete events must be dropped (event-lane byte cap, e.g. 4 MiB), send a reliable `document.events_lost {document_id, from_seq, to_seq, count}` so apps can resync (`get_form_data`, re-read values). Surface client-side drops (`RpcClient.dropped_notifications`, PowerShell `DroppedNotifications`) the same way.
  6. Coalesce frame writes (HOST-09) and raise the Windows pipe buffer quota (e.g. 1 MiB) as cheap mitigations.
  7. Rate-limit logs (HOST-08) and expose counters in `host.diagnostics` (`events_coalesced`, `events_lost`, `max_outbound_depth`, `max_outbound_bytes`).
  - Tests: transport unit test with a reader thread consuming at a fixed rate (e.g. 1 frame/ms) while the test enqueues 10 000 events + 1 response in one call burst → no disconnect, response delivered, all reliable messages delivered, coalesced/lost counters exact; controller test (`observed_notifications`) that unsubscribed types are not emitted; headless integration test equivalent to my `fast_reader_single_command_burst` with a slow reader; a headless way to produce transition events (advance the document clock in headless, or a test-only `advance_time` command) because headless currently emits 0 transition events (coordinator) so CI cannot see this class; `burst2.py` as a native acceptance test.
- Alternatives/tradeoffs: Only raising `MAX_OUTBOUND_MESSAGES` (e.g. 4096 within the 8 MiB byte cap) is a valid 1.0.x stopgap but just moves the threshold. Dropping without signalling (PR #3) is not acceptable (HOST-02).
- Complexity: L for the full design; S for stopgap (bigger queue + single-write frames + log rate-limit); M for lanes + stall policy.
- Confidence: High.

### HOST-02: PR #3 (drop saturated notifications) is not a correct fix
- Classification: Design defect in the proposed change (proven from code; not built/run, because building a worktree would rebuild Stylo).
- Component: PR #3 `Host::notify` → `Server::offer`; `Host::send`/`respond` unchanged.
- Evidence: (a) `notify` is the path for `app.shutdown` (`host/mod.rs:1001-1004`), `window.event closed` (`:1136`), `request.cancelled` (`:981-984`), `document.error` incl. fatal unload, `app.activated` (`:754`). PR drops them when saturated → e.g. dropped `app.shutdown` means the backend never learns to exit, is killed at the deadline, exit 1, no save; dropped `closed` leaves the binding with a live `Window` object. (b) Silent loss of `click`/`input`/`change`/`submit`/`keydown`: app state diverges from the UI with no signal (no seq numbers). (c) After a burst the queue sits at 128; the next response or `before_close`/`before_quit` request still goes through `send` → disconnect. In the issue's own scenario (theme switch at startup while the backend awaits `document.load`/commands) the fatal path remains. (d) The PR test only exercises `Server::offer`, not the host paths. Its claim that a non-reading peer is still ended by the writer's 10 s deadline is correct (Linux `NativeStream::write` per-message deadline; Windows `wait_pending` deadline).
- Reproduction: not reproduced (code reasoning). A test: saturate with 200 notifications, then `respond()` → observe disconnect on PR branch.
- Severity: High if merged as is. Likelihood: High under the same bursts.
- Impact: Replaces a loud failure with silent misbehaviour plus a residual loud failure.
- Root cause: drop policy is type-blind and responses have no reserved capacity.
- Remediation: Do not merge as is. If a quick patch is needed: restrict `offer` to an allowlist of non-semantic high-frequency event types (pointermove, mousemove, mouseover/out/enter/leave, wheel, scroll, transition*, animation*), route everything else through a reliable path with headroom (separate small queue or reserved slots), send a `document.events_lost` marker, and rate-limit the WARN. Then implement HOST-01.
- Complexity: M. Confidence: High.

### HOST-03: Linux supervised launch fails if the environment contains a non-identifier variable name or a non-UTF-8 value
- Classification: Confirmed defect (reproduced).
- Component: `platform/linux.rs` `bootstrap_supervised` (`:448-454`).
- Evidence: every env name is forwarded as `--setenv=NAME`; systemd-run rejects names that are not shell identifiers and values that are not UTF-8. `export -f` produces `BASH_FUNC_name%%`; Lmod/Environment Modules export `BASH_FUNC_module%%`/`BASH_FUNC_ml%%` by default (common on scientific workstations). Result: "Cannot assign environment variable BASH_FUNC_foo%%: Invalid argument" / "Failed to create bus message", exit 1, before any manifest handling.
- Reproduction: `env -u UI4ME_TEST_MODE 'BASH_FUNC_foo%%=() { echo; }' ./ui4me-host run --app /nonexistent/app.toml` (also `LATIN=$'caf\xe9'`).
- Severity: Medium. Likelihood: Medium (environment-dependent, deterministic when present).
- Impact: The application cannot start at all under supervision; message does not mention UI4ME or the variable's origin.
- Root cause: forwarding the whole environment without validating systemd's constraints.
- Remediation: in `bootstrap_supervised`, forward only names matching `^[A-Za-z_][A-Za-z0-9_]*$` whose value is valid UTF-8 (`std::env::vars_os` + `to_str`); log skipped names (names only) at debug; document that such variables are not inherited under supervision. Unit test: build the argument list with a synthetic env containing `A-B`, `BASH_FUNC_x%%`, non-UTF-8 value and assert they are excluded; optional integration test with `systemd-run` when available.
- Alternatives: pass the environment through a private `EnvironmentFile` (same validation applies), or `systemd-run --scope` (keeps the caller's environment natively; different supervision semantics).
- Complexity: S. Confidence: High.

### HOST-04: Listener dies permanently and silently on any accept error
- Classification: Confirmed defect on Linux (EMFILE reproduced); plausible on Windows (not testable).
- Component: `transport.rs` accept thread (`:165-173`), `host/mod.rs` `pump` handling of `Disconnected(0, …)` (`:500-511`); `windows.rs` `NativeListener::accept` (`:280-302`).
- Evidence: `Err(error) => { emit(Disconnected(0, …)); break; }` ends the thread, dropping the listener (socket file removed). The host ignores `Disconnected(0, …)` without logging. On Windows, `ConnectNamedPipe` → `ERROR_NO_DATA` (client connected and closed before the call) or `CreateNamedPipeW` → `ERROR_PIPE_BUSY` (instance churn beyond `nMaxInstances=9`) take the same path.
- Reproduction: `python3 fd_test.py 19` → "socket file still exists: False", late connect ENOENT, no log line.
- Severity: Low–Medium. Likelihood: Low on Linux (needs fd exhaustion); unknown on Windows (any same-session process that connects and drops quickly may trigger it).
- Impact: Secondary launches of a running single-instance app fail (exit 1) for the rest of the session; if it happens before `host.hello`, the app fails its startup deadline.
- Root cause: treating transient accept/instance errors as fatal.
- Remediation: in the accept loop, classify errors: `EMFILE/ENFILE/ENOBUFS/ENOMEM/ECONNABORTED/EPROTO` (Linux) and `ERROR_NO_DATA/ERROR_PIPE_BUSY/ERROR_BROKEN_PIPE` (Windows) → log (rate-limited), sleep 50–100 ms, continue; Windows: on `ERROR_NO_DATA`, `DisconnectNamedPipe` + reuse the instance. Host: log `Disconnected(0, reason)` at error level. Tests: Linux unit test with a lowered `RLIMIT_NOFILE` in a child process; Windows test that connects/closes a client before `ConnectNamedPipe`.
- Complexity: S. Confidence: High (Linux), Medium (Windows).

### HOST-05: Native loop can leave queued backend messages unprocessed for up to ~500 ms
- Classification: Plausible defect, proven from code; not reproduced (native mode not run).
- Component: `host/mod.rs` `pump` (64 transport events per call, `:489-513`), `host/native.rs` `about_to_wait` (`WaitUntil` 500 ms when idle, `:609-624`).
- Evidence: one coalesced `proxy_wake_up` → `pump` (64) + `about_to_wait` → `pump` (64). Nothing re-wakes the loop when the budget was exhausted; readers wake only when enqueuing. With >128 messages already queued and no other activity, the rest waits for the 500 ms timer (shorter only with document timers).
- Reproduction: not run; test: native or loop-simulated host, enqueue 300 notifications at once, measure time until the last is dispatched.
- Severity: Low–Medium. Likelihood: Medium for pipelined/notification-style batch updates (e.g. PowerShell or `call_async` batches).
- Impact: Visible UI lag/stutter; response latency spikes.
- Remediation: in `pump`, if the 64-event loop ended because of the budget (not `Empty`), call `(self.wake)()` (or `set_control_flow(Poll)` for the next iteration). Unit test: fake wake counter asserted after a pump with >64 queued events.
- Complexity: S. Confidence: Medium.

### HOST-06: Backend commands run unbounded on the GUI thread
- Classification: Design tradeoff with a measured pathological case (reproduced, debug build).
- Component: `requests.rs` dispatch → `documents.rs` `dispatch_document`/`load_document` (parse, `read_html` file I/O, `canonicalize`), renderer.
- Evidence: one `request_submit` on a form of N empty required inputs blocked the GUI thread 0.39 s (N=100), 1.15 s (400), 12 s (1000), ~100 s (2000); the host only noticed the backend disconnect after 112 s. Growth is superlinear (renderer area; cause not isolated). Docs: "Rendering, input and accessibility keep working while the backend is busy" (architecture.md) — true for waiting on the backend, not for work the backend requests.
- Reproduction: `python3 adv_run.py fast_reader_single_command_burst ADV_N=1000`.
- Severity: Low (debug numbers; release faster) but the superlinearity is worth a renderer ticket. Likelihood: Low–Medium.
- Impact: Frozen windows ("not responding"), delayed close/quit, deadlines (transfers, unauth expiry) evaluated late.
- Remediation: (a) renderer: find the superlinear path in interactive validation (`request_submit` / `invalid` dispatch); (b) host: measure per-command duration and log commands > 100 ms with op name; (c) longer term, a per-pump time budget (process messages until ~8 ms elapsed, then yield and re-wake) instead of a fixed count of 64. Document the tradeoff.
- Complexity: S (logging/budget), M–L (renderer). Confidence: Medium (cause), High (observation).

### HOST-07: No signal or session-end handling; abrupt termination skips `app.shutdown` and leaks the endpoint
- Classification: Confirmed behaviour (reproduced headless), design gap.
- Component: whole host (no `SIGTERM`/`SIGINT` handling, no `WM_QUERYENDSESSION`/`WM_ENDSESSION` handling found in host or vendored winit-win32).
- Evidence: SIGTERM → exit -15 in 3 ms; backend received no `app.shutdown` (PDEATHSIG kills it); `.sock` left behind. Under systemd (`KillMode=control-group`, `TimeoutStopSec=5s`), `systemctl --user stop`, logout or shutdown SIGTERM all processes simultaneously, so the app gets no chance to save. `$XDG_RUNTIME_DIR/ui4me-host` on this machine held 148 stale sockets (137 with 24-hex keys from instance=multiple runs by other test processes) and 360 lock files.
- Reproduction: `python3 sigterm_test.py SIGTERM`.
- Severity: Low–Medium. Likelihood: Medium (logout/shutdown/process managers).
- Impact: Unsaved data loss without before-quit prompt; stale socket files accumulate (harmless but untidy; multiple-instance keys are never reused).
- Remediation: Linux: `signalfd`/self-pipe thread for SIGTERM/SIGINT/SIGHUP → set a flag + wake → `begin_shutdown()` (or `request_quit("session_end")` with a short deadline) — note the backend receives SIGTERM directly under `control-group`; consider `KillMode=mixed` so only the main process (host) gets SIGTERM and the host drives the backend shutdown within `TimeoutStopSec`. Windows: handle `WM_QUERYENDSESSION`/`WM_ENDSESSION` (needs winit hook or a hidden window like the preferences observer) and console control events. Document the semantics. Tests: headless SIGTERM test asserting `app.shutdown` delivered and exit status documented.
- Complexity: M. Confidence: High.

### HOST-08: Disconnect paths flood the log and mislabel force quit
- Classification: Confirmed (reproduced: 214 and 1025 WARN lines; coordinator: 61–351).
- Component: `Host::send` (`host/mod.rs:442-449`), `pump` quit deadline branch (`:648-657`).
- Evidence: after the first failure every further send in the same pump logs "peer cannot receive bounded output (connection is closed)" and calls `disconnect` again. `app.force_quit`/Ctrl+Shift+Q set `quit_deadline = now` and then log "backend exceeded shutdown deadline; managed workers are being terminated" with exit 1.
- Severity: Low. Likelihood: High when HOST-01 triggers.
- Impact: Log ring/sink overflow hides the real cause (and costs GUI time formatting JSON).
- Remediation: in `send`, on first failure set `self.client_dead = true` (or check `server.is_connected`) and skip further sends silently; log once per connection with a counter. Give force quit its own log message and document its exit status (currently 1).
- Complexity: S. Confidence: High.

### HOST-09: Each frame is written with two syscalls
- Classification: Confirmed from code; contributes to HOST-01.
- Component: `transport.rs` writer (`:303-307`), same for Windows (`WriteFile` twice).
- Evidence: `write_all(len)` then `write_all(body)`. On Linux each write becomes its own skb, so a 4-byte header costs as much buffer accounting as a body; effective socket buffering roughly halves (consistent with the ~150–250 message threshold measured at 2 ms/message).
- Severity: Low. Likelihood: always.
- Remediation: build `[len|body]` in one `Vec` (the body is already serialized in `reply`; reserve 4 bytes up front) or batch all queued frames up to ~64 KiB per write. Test: count writes with a mock stream.
- Complexity: S. Confidence: High.

### HOST-10: Secondary launch during primary shutdown fails instead of becoming the new primary
- Classification: Confirmed (reproduced).
- Component: `host/mod.rs` `forward_activation` (`:261-283`), `message` app.activate branch (`:740-744`).
- Evidence: primary in `quitting` answers `-32004 closed`; the launcher returns that error, exit 1. The same applies in the window between `server.close()` and process exit (connect retries until the 5 s clamp, then fails).
- Reproduction: `python3 secondary_test.py` → "exit 1 after 0.01 s; Primary instance is shutting down (RPC -32004)".
- Severity: Low. Likelihood: Medium (user closes and immediately reopens an app, shutdown window up to `shutdown_timeout_ms`).
- Remediation: on `CLOSED` or connect failure, poll `InstanceLock::acquire` (Linux flock, Windows mutex) until `shutdown_timeout_ms + margin`; if it becomes primary, continue a normal start. Test: `secondary_test.py` expecting the second launch to run its backend.
- Complexity: M. Confidence: High.

### HOST-11: Multiple-instance endpoint name is derived from the auth token
- Classification: Confirmed (reproduced: basename == token[:24]); low practical risk.
- Component: `host/mod.rs:369-372`.
- Evidence: `instance_key = token[..24]` for `instance = "multiple"`. 96 of 256 secret bits appear in the socket path / pipe name; Windows pipe names are enumerable by every user, endpoints appear in env (`UI4ME_ENDPOINT`), diagnostics and logs. 160 bits remain, so no brute-force risk, but secrets should not be split into public identifiers.
- Severity: Low. Remediation: generate an independent random 12-byte key. Test: assert endpoint key ≠ token prefix. Complexity: S. Confidence: High.

### HOST-12: Windows pipe client trusts whatever owns the name; cross-session squatting can block the app
- Classification: Plausible concern (Windows not testable).
- Component: `windows.rs` `NativeStream::connect` (`:402-437`), `NativeListener::create_pipe`.
- Evidence: pipe namespace is global and listable; single-instance names are deterministic. Another session's user who has seen the name can create it while the app is not running → victim's host fails `FIRST_PIPE_INSTANCE` (DoS, fails closed, good). A concurrent secondary launch would connect to the squatter and send `args`/`cwd`; `CreateFileW` is called without `SECURITY_SQOS_PRESENT`, so the server may impersonate the client (identification level without SeImpersonatePrivilege).
- Severity: Low. Likelihood: Low (multi-user terminal servers).
- Remediation: connect with `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`; after connect verify the server via `GetNamedPipeServerProcessId` + process token user/logon SID (or the pipe's owner SID via `GetSecurityInfo`) before sending anything. Log a clear error on bind failure due to a foreign owner. Complexity: S–M. Confidence: Medium.

### HOST-13: Predictable `/tmp` fallback runtime directory and non-atomic creation
- Classification: Proven from code; DoS only (fails closed).
- Component: `linux.rs` `runtime_base`/`ensure_private_directory` (`:68-133`).
- Evidence: without `XDG_RUNTIME_DIR` the base is `<tmp>/ui4me-runtime-<uid>`; another local user can pre-create it → ownership check fails → every UI4ME app of the victim fails to start. `create_dir` then `set_permissions(0700)` leaves a window at `0777 & ~umask`.
- Severity: Low. Remediation: `DirBuilder::new().mode(0o700).create`; error message naming the directory owner; consider refusing single-instance mode without `XDG_RUNTIME_DIR`. Complexity: S. Confidence: High.

### HOST-14: Host-private variables leak into the backend; a UI4ME app started by a backend is bound to its parent
- Classification: Plausible concern (not reproduced).
- Component: `linux.rs` `spawn_backend` (env inheritance), `is_systemd_managed`, `watch_launcher`, group kill in `NativeChild::try_wait`.
- Evidence: backend inherits `UI4ME_SYSTEMD_UNIT`, `UI4ME_LAUNCHER`, `UI4ME_DESKTOP_REALM`. A host launched by that backend sees itself as "systemd managed" (same cgroup) → skips supervision, watches the parent's launcher, and sits in the parent backend's process group which is SIGKILLed when that backend exits.
- Severity: Low. Remediation: `env_remove` these in `spawn_backend` (both platforms), reserve them in config validation; document how an app should start an independent app. Complexity: S. Confidence: Medium.

### HOST-15: Error answers to policy requests are treated as a veto, including "busy"
- Classification: Design issue (code + client reading); interplay with HOST-01.
- Component: `host/mod.rs:666-672` (`allow` defaults to false); Python `client.py:436-440` answers `-32005 Callback queue is full` when its 128-slot callback queue is full; PowerShell answers "PowerShell callback queue is full".
- Evidence: under an event flood the user's close/quit is silently vetoed; in a document window the probe status text "Close cancelled" is not visible.
- Severity: Low–Medium. Remediation: treat `-32005`/`-32603` answers to `before_close`/`before_quit` as "no decision" (re-ask after a short delay, or show a host-level notice), keep explicit `{"allow": false}` as the only veto; bindings should never queue policy requests behind notifications (separate queue). Complexity: S. Confidence: Medium.

### HOST-16: Panic handling and diagnostics
- Classification: Design gap / optional improvement.
- Component: `main.rs`, `logging.rs`; `panic = "unwind"` in release.
- Evidence: no panic hook; logs and panic messages go only to stderr, which `detach_owned_console` nulls for Explorer/Start-menu launches on Windows. A panic on the GUI thread ends the whole host (Wayland: unwinds out of `run_app`; Windows: winit catches and resumes after the loop); Drop of `Host` then kills the backend, so no orphan. No `catch_unwind` around per-document engine calls (renderer has some internally). IO threads have no panic path that I found.
- Severity: Low. Remediation: install a panic hook that emits a `tracing::error!` with location and thread name; optionally a per-app log file in the state dir on Windows GUI launches; optionally `catch_unwind` around `document.command`/`handle_input` that unloads the failed document with a fatal `document.error` instead of killing all windows. Complexity: S–M. Confidence: High.

### HOST-17: Doc/implementation mismatches and dead code
- Classification: Maintenance problem (reproduced where noted).
- Evidence: protocol.md "Every params object is decoded strictly" — `host.hello` accepts unknown keys (reproduced), `host.diagnostics`/`host.capabilities`/`app.ready`/`app.request_quit`/`app.force_quit` ignore params (2 MiB junk param accepted). host-cli.md lists the systemd-run flags without `--property=ExitType=main`, and does not mention HOST-03's env restriction or force-quit exit 1. architecture.md "The first request must be host.hello" (other requests are answered -32011, connection kept). `protocol::MAX_OUTGOING_BYTES` duplicates `transport::MAX_OUTBOUND_BYTES` and is unused. protocol.md "a peer that does not read its messages is disconnected" understates HOST-01 (a reading peer is disconnected too).
- Remediation: decode `{}`-param methods with a `deny_unknown_fields` empty struct (or fix the doc); allow-list `token`, `protocol_version`, `client` in hello; update docs; delete the dead constant. Complexity: S. Confidence: High.

## 4. Test gaps and highest-value tests

1. Burst/backpressure (HOST-01/02/05/09): deterministic transport test with a rate-limited reader and a 10 000-message burst plus interleaved responses; host controller test that `app.shutdown`/`closed`/`request.cancelled` are never dropped; headless test with slow reader (`fast_reader_single_command_burst` with `ADV_READ_DELAY=0.002`) expecting no failure; headless transition-event generation so CI sees issue #1; native acceptance `burst2.py`.
2. Authentication/admission (all currently passing, but no tests exist): pre-hello -32011, wrong token, wrong version, second backend -32005, 5 s unauth expiry, admission limit, unauth `app.activate` — port `adv_backend.py auth` and `malformed_secondary` into `tools/` as a pytest against the headless host.
3. Accept resilience (HOST-04): EMFILE child-process test; Windows ERROR_NO_DATA test.
4. Bootstrap env filtering (HOST-03): unit test of the argument builder.
5. Signals/session end (HOST-07) and second launch during shutdown (HOST-10).
6. Pipelined client without reading (`flood_no_read`) — should slow down, not fail, after the fix.

## 5. Recommend NOT changing

- Fail-fast on malformed frames from the authenticated backend; strict 2 MiB pre-allocation check; strict JSON-RPC profile.
- Inbound backpressure (readers block while the GUI queue is full) — do not convert it to dropping input.
- No automatic decision timeout that auto-allows close/quit (data-loss risk); Ctrl+Shift+Q is the right escape. (HOST-15 only changes how error answers are interpreted.)
- Token-less `app.activate` on the private endpoint (needed for secondary launch); only document that `args`/`cwd` are untrusted input.
- `waitid(WNOWAIT)` + group kill, PDEATHSIG with `getppid` check, Windows job-at-creation, Pipe in-flight accounting.
- systemd-run supervision with names-only env forwarding (just filter invalid names).
- Lossy non-blocking logging.

## 6. Open questions

- Windows: does `ERROR_NO_DATA`/`ERROR_PIPE_BUSY` actually kill the listener in practice (HOST-04)? Needs a winDEV test.
- Native latency of HOST-05 under winit Wayland/Win32 (wake coalescing) — needs measurement.
- Release-build cost and root cause of the superlinear `request_submit` path (HOST-06) — renderer area.
- Whether older systemd versions (pre-257) reject the same names/values (HOST-03); RHEL/Ubuntu LTS checks.
- Whether `KillMode=mixed` + host-driven shutdown fits the qualified hard-host-death guarantees (HOST-07).
