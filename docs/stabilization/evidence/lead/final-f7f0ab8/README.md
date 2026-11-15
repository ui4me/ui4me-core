# Final verification of the integration head

The code under test is `f7f0ab8`, the merge of the renderer review fixes. `dcd89a7` afterwards changes only the test
harness's host lookup.

## Linux (local workstation)

`python3 tools/verify.py --keep-going --jobs 6` passed fmt, check, test, headless, renderer and faults.

The python stage failed once: `test_f25_hung_engine_is_ended_after_the_termination_deadline` ran against a stale
`target/release` host built before the host review fixes. The test helpers preferred release over the freshly built
debug host. That was fixed in `dcd89a7`: the stage now builds the host first, and the tests use the newest build. On
the re-run, python passed (250 tests, 4 skipped) and clippy passed (`-D warnings`).

The fuzz harnesses (all 7, 2,000 cases each, `--test-threads=1`) passed with 0 findings. They ran on the renderer fix
branch, whose renderer and assets code is identical to this head.

## Windows Server 2022 (winDEV, workspace stab-374b06f6)

The release host was rebuilt from this head. Results by suite:

| Suite | Session | Result |
| --- | --- | --- |
| Host binary | SSH | PE stack reserve 32 MiB |
| Host tests | SSH | 118 + 1 + 8 pass |
| `stack_depth` | SSH | 8 pass |
| `content_stalls` | SSH | 13 pass |
| `svg_filter_cost` | SSH | 5 pass |
| `svg_depth` | SSH | 13 pass |
| Renderer lib | SSH | 558 pass |
| Pester with the real host | interactive desktop | 74 passed, 0 failed, 0 skipped on Windows PowerShell 5.1 and on PowerShell 7.4.6 |
| Python 3.13 and 3.10 binding suites | interactive desktop | 107 tests OK each |
| Event bursts (100 controls × 6 rounds, PS 5.1 and 7, with and without slow callbacks) | interactive desktop | droppedControl 0, droppedDiscrete 0, every round OK |
| Ctrl+Break (Python, PS 5.1, PS 7) | interactive desktop | exit 130, shutdown hook ran, no debugger prompt |
| BIND-04 | interactive desktop | the pip launcher shim `ui4me-host.exe` is not taken for the host |

Interactive-desktop runs used the `ui4me_audit` session.

The files in this directory are the raw outputs.
