# BIND-04/-06/-09/-10/-12: new tests against python/ at 1e49a6a (real headless host from this branch)

Command: python3 -m unittest tools.test_bootstrap tools.test_launcher (new test files copied into a 1e49a6a worktree)

```text
ERROR: test_sys_exit_in_a_close_policy_quits_instead_of_hanging (tools.test_bootstrap.DirectStartIntegrationTests.test_sys_exit_in_a_close_policy_quits_instead_of_hanging)
ERROR: test_host_killed_by_a_signal_maps_to_the_shell_status (tools.test_bootstrap.RoleTests.test_host_killed_by_a_signal_maps_to_the_shell_status)
ERROR: test_second_start_in_one_process_is_refused (tools.test_bootstrap.RoleTests.test_second_start_in_one_process_is_refused)
FAIL: test_failed_first_page_can_fall_back (tools.test_bootstrap.DirectStartIntegrationTests.test_failed_first_page_can_fall_back)
FAIL: test_host_killed_by_signal_gives_status_137_and_a_message (tools.test_bootstrap.DirectStartIntegrationTests.test_host_killed_by_signal_gives_status_137_and_a_message)
FAIL: test_quit_requested_during_startup_is_orderly (tools.test_bootstrap.DirectStartIntegrationTests.test_quit_requested_during_startup_is_orderly)
FAIL: test_second_start_raises_instead_of_launching_another_host (tools.test_bootstrap.DirectStartIntegrationTests.test_second_start_raises_instead_of_launching_another_host)
FAIL: test_console_script_on_path_is_refused_quickly (tools.test_launcher.HostDiscoveryTests.test_console_script_on_path_is_refused_quickly)
FAIL: test_native_host_later_on_path_is_found (tools.test_launcher.HostDiscoveryTests.test_native_host_later_on_path_is_found)
Ran 32 tests in 75.072s
FAILED (failures=6, errors=3)
```

Python 3.10.22 timeout probe (scratch timeout_probe.py):
```text
1e49a6a:     NOT a TimeoutError: concurrent.futures._base TimeoutError
this branch: caught by except TimeoutError: ui4me_core.client _CallTimeout
```
