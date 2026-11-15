# F-18 (HOST-03): supervised start with environment entries systemd rejects (Linux, systemd 257)

Command (non-headless so `bootstrap_supervised` runs; the manifest does not exist, so the
re-executed host stops at manifest loading):

    env -u UI4ME_TEST_MODE 'BASH_FUNC_foo%%=() { echo; }' ui4me-host run --app /nonexistent/app.toml
    env -u UI4ME_TEST_MODE LATIN=$'caf\xe9'               ui4me-host run --app /nonexistent/app.toml

| Host | `BASH_FUNC_foo%%` | non-UTF-8 value |
| --- | --- | --- |
| baseline | `Cannot assign environment variable BASH_FUNC_foo%%: Invalid argument` (systemd-run, start refused) | `Failed to create bus message: Invalid argument` |
| this branch | `Cannot locate application manifest /nonexistent/app.toml` (supervised start succeeded) | same |

Regression tests: `platform::linux::tests::supervised_environment_skips_what_systemd_rejects`
(unit, argument builder) and `tools/test_host_lifecycle.py::...test_t15_supervised_start_with_rejected_environment`
(integration, skipped without a systemd user manager).
