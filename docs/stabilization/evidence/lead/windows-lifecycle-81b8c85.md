# Windows verification of workstream G (integration `81b8c85`)

winDEV, release and debug hosts built from `81b8c85`.

| Command | Result |
| --- | --- |
| `cargo test --release -p ui4me-host` | lib 109 passed; integration 8 passed, 1 ignored (includes the accept-loop churn test for `ERROR_NO_DATA`/`PIPE_BUSY` and `transient_accept_errors_*`) |
| `python -m unittest test_host_lifecycle` (release host) | 8 tests: HOST-14 and HOST-15 pass; 6 skipped by design (POSIX signals, EMFILE, systemd) |
| `python -m unittest test_headless_clock` | R5(c) drain and T3 virtual-clock transition burst pass; RSS test skipped (reads `/proc`) |

## Console Ctrl+Break ([ctrlbreak_driver.py](ctrlbreak_driver.py) with [ctrlbreak_app.py](ctrlbreak_app.py))

A direct-start app (`UI4ME_TEST_MODE=1`) is started in its own process group, then `CTRL_BREAK_EVENT` is sent.
The backend records its `on_shutdown` callback in a marker file.

| Host | Backend `on_shutdown` (app.shutdown delivered) | Launcher exit |
| --- | --- | --- |
| 1.0.1 published wheel | **not delivered** | 0xC000013A (STATUS_CONTROL_C_EXIT) |
| integration `81b8c85` | **delivered** about 10 ms after the event | 0xC000013A |

The host side of HOST-07 works on Windows. The direct-start **launcher** still dies at once on the console control
event (Python's default handler), and `run()` does not return in the backend. Passed to workstream E (BIND-10).
WM_QUERYENDSESSION / WM_ENDSESSION (sign-out) is not run, because signing out disrupts the shared VM session; it is
planned for a pvey clone or recorded as unverified.
