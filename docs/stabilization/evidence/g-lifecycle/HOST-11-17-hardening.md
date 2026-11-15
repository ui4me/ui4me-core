# HOST-11, -12, -13, -14, -16, -17 hardening batch

| Item | Change | Test | Baseline |
| --- | --- | --- | --- |
| HOST-11 | `instance = "multiple"` endpoint key = independent 12 random bytes | `host::tests::multiple_instance_endpoint_key_is_independent_of_the_token` | key was `token[..24]` (audit `endpoint_token_relation`) |
| HOST-12 (partial, Windows) | pipe client: `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`; refuses a server in another session (`GetNamedPipeServerSessionId`) | type/lint check only | no SQOS, no server check |
| HOST-13 | runtime dir created 0700 atomically (`DirBuilder::mode`); foreign owner named in the error | `platform::linux::tests::private_directories_are_created_private_and_wide_ones_refused` | `create_dir` then `chmod` |
| HOST-14 | backend env without `UI4ME_SYSTEMD_UNIT`, `UI4ME_LAUNCHER`, `UI4ME_DESKTOP_REALM`; reserved in manifests | `tools/test_host_lifecycle.py BackendEnvironmentTests`, `config::tests::transport_environment_cannot_be_overridden` | baseline backend saw all three |
| HOST-16 | panic hook: `error` record `host thread panicked` (thread, location, message) | `logging::tests::panic_payloads_become_bounded_text` | stderr text only |
| HOST-17 | zero-param methods reject unknown keys (-32602); `host.hello` stays tolerant; `protocol::MAX_OUTGOING_BYTES` deleted | `host::requests::tests::zero_parameter_methods_are_strict_but_hello_negotiates` | 2 MiB junk param accepted (audit) |

Deferred with reason:
* HOST-12 server user-SID verification: needs Windows runs with elevated/non-elevated pairs;
  a false refusal would break every secondary launch. Session check + SQOS cover the
  cross-session squatting case in the audit.
* HOST-16 `catch_unwind` around document commands: changes failure semantics across the
  renderer boundary; separate review.
* HOST-07 `KillMode=mixed`: would delay SIGKILL of managed grandchildren after hard host death
  by up to `TimeoutStopSec`; the qualified cleanup guarantee needs re-qualification first.
