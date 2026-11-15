# PowerShell T16 / BIND-PS-02..09 mock-peer tests against the 1.0.1 module (1e49a6a)

Robustness tests (later split into Delivery.Tests.ps1 and Callbacks.Tests.ps1) run in a 1e49a6a worktree with the new MockHost.ps1:

```text
[-] Control notifications under event bursts (T16).retires the window when a close lands in a 1,000-event burst, and drops no control notification 4.25s
[-] Control notifications under event bursts (T16).delivers a click that follows a 300-event burst 1.37s
 Expected 'after-burst', but got $null.
[-] Control notifications under event bursts (T16).never lets unsubscribed events take admission slots 11.08s
 Expected $true, but got $false.
[-] Control notifications under event bursts (T16).keeps the closed event that arrives while a callback runs, beyond the deferral cap (BIND-PS-03) 5.78s
 Expected $true, but got $false.
[-] Control notifications under event bursts (T16).delivers deferred final events before Wait-UI4ME -UntilShutdown returns (BIND-PS-04) 1.65s
 Expected $true, but got $false.
[-] Callback pitfalls and codec edge cases.contains break and continue in event handlers, so the other handlers still run (BIND-PS-08) 5.04s
 Expected $true, but got $false.
[-] Callback pitfalls and codec edge cases.answers an explicit boolean from policies; break, strings and numbers veto (BIND-PS-08/09) 29ms
 Expected the value to have type [bool] or any of its subtypes, but got $null with type $null.
[-] Reserved member names and dialog waits.reads attributes named psbase, PSObject, pstypenames and the empty name (BIND-PS-01) 13ms
[-] Reserved member names and dialog waits.waits for a -NoWait dialog without the 15 second default deadline (BIND-PS-05) 12ms
 Expected '-1', but got $null.
Tests Passed: 1, Failed: 9, Skipped: 0, Inconclusive: 0, NotRun: 0
```

The 'break in a deferred notification callback' test, rewritten to use the deferred path, fails on 1e49a6a: 'Expected $true, but got $null.' (the RPC wait returned nothing).

Native (real headless host) tests in DirectStart.Tests.ps1 on 1e49a6a:
```text
[-] Start-UI4MEScript robustness with the real native host (headless test mode).starts from a helper function in a dot-sourced file and from a module function (BIND-PS-06) 2.37s
 Expected 0, because ui4me-host error: application failed (reason=Backend exited with exit status: 0), but got 1.
[-] Start-UI4MEScript robustness with the real native host (headless test mode).reads attributes PowerShell reserves as member names from the real host (BIND-PS-01) 3.07s
 Expected 0, because Get-UI4MEAttributes: /tmp/Pester_twpj/direct ü d681ad/app.ps1:6
[-] Start-UI4MEScript robustness with the real native host (headless test mode).exits with status 130 when interrupted with SIGINT (BIND-PS-07) 2.28s
 Expected 130, but got 0.
Tests Passed: 0, Failed: 3, Skipped: 0, Inconclusive: 0, NotRun: 11
```

BIND-PS-01 against the real headless host, raw get_attributes on <div id='x' psbase='1'>:
```text
1e49a6a:     ExtendedTypeSystemException: The member name "psbase" is reserved. / ui4me-host error: application failed (reason=Backend connection lost: peer disconnected)
this branch: RAW psbase=1
```
