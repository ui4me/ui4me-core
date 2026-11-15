# F-25: host lifecycle edge cases (HOST-04, -07, -10, -15) — Linux headless, debug build

All integration tests: `python3 -m unittest tools.test_host_lifecycle -v` (real headless host,
raw-protocol backend `tools/headless_backend.py wait`). Baseline = host built from db914b2,
run with `UI4ME_HOST_BINARY=<baseline>`.

## HOST-04 accept loop (`AcceptLoopTests`)

The test lowers the running host's RLIMIT_NOFILE to its current descriptor count (prlimit), so
accept(2) fails with EMFILE while a connection waits in the backlog, then restores the limit.

| Host | Observed |
| --- | --- |
| baseline | endpoint socket removed, accept thread ended, no log line -> `AssertionError: the endpoint must still exist` |
| this branch | WARN `transient IPC accept error; the listener keeps accepting` (rate-limited), endpoint kept, late request answered `-32011` by the live host |

Windows (not runnable here): ConnectNamedPipe `ERROR_NO_DATA` now disconnects and reuses the
instance; `ERROR_PIPE_BUSY`/`ERROR_BROKEN_PIPE`/resource errors are retried. Needs winDEV.

## HOST-07 termination requests (`TerminationSignalTests`)

| Signal | baseline | this branch |
| --- | --- | --- |
| SIGTERM | host exit -15 in ms, backend got nothing, socket left behind (audit) ; test ERROR (no report) | backend receives `app.shutdown {deadline_ms: 3000}`, host exit 143, endpoint removed |
| SIGINT | exit -2 ; test ERROR | `app.shutdown`, exit 130 |
| SIGHUP | killed ; test ERROR | `app.shutdown`, exit 129 |

Unit: `host::tests::termination_request_starts_an_orderly_shutdown_and_a_second_forces_it`,
`termination::tests::*`.

Windows console control + WM_QUERYENDSESSION/WM_ENDSESSION: implemented
(`platform/windows.rs`), type/lint-checked with `tools/windows_source_check.py` only.

## HOST-10 relaunch during primary shutdown (`RelaunchTests`)

Primary (single instance) requests its quit 0.5 s after ready and its backend takes 1.5 s to
exit after `app.shutdown`; a second launch of the same app starts 0.8 s after ready.

| Host | Observed |
| --- | --- |
| baseline | second launch exits 1 at once: `Primary instance is shutting down (RPC -32004)` |
| this branch | second launch logs `primary instance is shutting down; waiting to take over`, acquires the lock when the primary exits (primary exit 0), starts its own backend on the same endpoint, later exits 143 on SIGTERM |

## HOST-15 error answers to policy requests (`PolicyAnswerTests`)

Backend asks to close its only window and answers `window.before_close` with `-32005 Callback
queue is full` twice, then `{"allow": true}`.

| Host | Observed |
| --- | --- |
| baseline | first busy answer = veto; window stays, no `app.before_quit`; the test times out (30 s) |
| this branch | asked again after 100 ms and 200 ms under new IDs; third answer allows; last window -> `app.before_quit` -> `app.shutdown`, exit 0 |

Unit: `host::tests::error_answers_to_policy_requests_are_no_decision` (busy retry, stale old
ID, -32603 = no decision with window kept, retry budget, quit error/malformed answer keep the
app running, explicit false still vetoes), `lifecycle::tests::a_reissued_decision_keeps_its_place_and_stales_the_old_id`.
Data-loss protection kept: no path turns an error, a timeout or a malformed answer into an allow.
