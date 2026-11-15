# BIND (PowerShell half): UI4ME.Core PowerShell binding audit

Audited commit c6fd71d (UI4ME.Core 1.0.1). This covers the PowerShell half only. Python, packaging and the merged parity table belong to the parent BIND report.

## 1. Scope and method

What I read:
- `powershell/UI4ME.Core/UI4ME.Core.psm1` (all 1805 lines)
- `private/UI4ME.Transport.cs` (all 898 lines)
- `UI4ME.Core.psd1`
- `powershell/tests/{TestHelpers,MockHost,Transport.Tests,Unit.Tests,DirectStart.Tests}.ps1` and the headers of `native/Qualify.ps1`
- `docs/reference/powershell-api.md` (lifecycle, callbacks, limits and parity sections)
- `docs/guides/patterns-powershell.md` (§11 and §12)
- `docs/reference/protocol.md` (method and op tables)
- Host code where it decides how the client must behave: `host/mod.rs` decision parsing at :662-700 and `document_commands.rs` `get_attributes` at :829-835.

What I ran (all experiment files are in `<scratch>/exp/BIND/ps/`):
- **Pester 6.2.0 on pwsh 7.7.0 (Linux).** Ran the full `powershell/tests` suite. 54 passed, 0 failed, 0 skipped (`pester.log`). This includes the 4 native headless `Start-UI4MEScript` tests, which ran against the existing `target/debug/ui4me-host`.
- **`mockpeer.py`.** A Python mock peer of my own: a Unix socket that speaks the length-prefixed JSON-RPC and has controllable bursts, policy requests, slow replies and read stalls. Client scripts run against it:
  - `burst_client.ps1`
  - `rate_client.ps1`
  - `policy_client.ps1`
  - `flow_client.ps1`
  - `scope_client.ps1`
  - `closure_client.ps1`
  - `dialog_client.ps1`
  - `shutdown_client.ps1`
  - `listpipe.ps1`
  - `thread_client.ps1`
  - `stall_client.ps1`
- **Real host in headless mode** (`UI4ME_TEST_MODE=1`, `UI4ME_HOST_BINARY=target/debug/ui4me-host`):
  - `reserved_host.ps1`
  - `forever.ps1`: SIGINT, SIGTERM and process-group SIGINT sent to the launcher
  - `dotsrc/app.ps1`
  - `py_forever.py`: a Python launcher comparison
- **Codec probes:** `codec.ps1` and `reserved.ps1`.
- **Lint and manifest:** `errrec.ps1` (error records, verbs, pipeline, manifest) and PSScriptAnalyzer 1.25 (`pssa.txt`).
- **Add-Type compile time:** about 300-530 ms per process on pwsh 7 Linux.

What I did not do:
- I had no Windows PowerShell 5.1 and no Windows machine. Every Windows statement below is reasoned from code and labelled as such.
- I made no repo modifications.

## 2. Strengths to preserve

- **Strict, purpose-built JSON codec** (`Transport.cs:63-384`).
  - No date coercion (the PS 7 `ConvertFrom-Json` trap). Verified: `"2024-01-01T00:00:00Z"` stays a string.
  - No `ConvertTo-Json` depth-2 truncation. A depth cap of 128 applies in both directions.
  - Numeric types are kept: int, then long, then ulong, then double. NaN and Infinity are rejected both ways.
  - Duplicate keys are rejected. Keys that differ only by case are kept in an ordinal Hashtable.
  - Empty and single-element arrays serialize correctly (`@(1)` becomes `[1]`, `@()` becomes `[]`).
  - Unsupported values (DateTime, Enum, ScriptBlock) fail loudly.
- **The reader thread never writes.**
  - Verified every path in `ReadLoop` (`Transport.cs:564-612`). It only parses, calls `incoming.Add` (an unbounded collection, so this never blocks) and calls `rejections.TryAdd` (non-blocking).
  - Rejections are sent by a separate rejecter thread (`:470-477`). If even that bounded queue overflows, the connection fails rather than blocking.
  - Responses are admitted only for registered IDs, so late replies to cancelled requests are discarded on the reader thread (`:577-582`).
  - Lifecycle messages bypass the admission bounds (`:502-505`, `:585`).
- **Fast client-side drain.** The reader absorbed 1000 frames in about 35 ms (mock measurement). The PowerShell client therefore does not cause the host-side outbound-queue saturation behind issue #1. What it does instead is drop, see BIND-PS-02.
- **Single-runspace callback model.**
  - No PowerShell code runs on .NET threads.
  - Notifications run in host order and are never nested inside a running callback. Tested and observed.
  - Lifecycle requests are answered inside nested pumps, so a click handler that waits on a dialog does not block closing.
- **Handle model.**
  - Stale handles fail early with `UI4ME.StaleObjectException`.
  - Documents are retired on replace, on window close and on fatal `document.error`.
  - Bitmaps are released with their document.
  - Typed `RpcException` carries Code, RpcMessage and RpcData.
  - Timeouts send `request.cancel` and treat the outcome as unknown.
- **Direct start design.**
  - Environment scrubbing and a loop guard (`UI4ME_DIRECT_START`).
  - Parent-death handling: `setpriv --pdeathsig` on Linux, a kill-on-close job on Windows.
  - Manifest cleanup plus a sweep of stale manifests.
  - Correct TOML escaping, CommandLineToArgvW quoting for .NET Framework, and a BOM on generated scripts for 5.1.
  - Validated: SIGTERM to the launcher took down the host and the backend, with exit 143.
- **Docs are candid** about the pumping model, the absence of pumping during long work (patterns §12), the `.GetNewClosure()` and `$script:` caveat, and the last-output policy semantics. All of these matched observed behaviour.
- **Hygiene.** All 83 verbs are approved, the manifest and module exports match (tested), versions are consistent at 1.0.1 (psd1, psm1, pyproject), and the helper compiles lazily rather than at import.

## 3. Findings

### BIND-PS-01: A host JSON object whose member name PowerShell reserves (`psbase`, `psobject`, `psadapted`, `psextended`, `pstypenames`) or that is empty kills the backend connection
- **Classification:** Confirmed defect (reproduced against the real headless host).
- **Component:** `UI4ME.Transport.cs`, `Json.Parser.ReadObject`.
- **Evidence:**
  - `Transport.cs:144-146` turns every member into a `PSNoteProperty`: `result.Properties.Add(new PSNoteProperty(member.Key, member.Value))`.
  - PowerShell throws `ExtendedTypeSystemException` ("The member name "psbase" is reserved") for the five names above, case-insensitively, and `PSArgumentException` for `""` (see `reserved.ps1`).
  - `ReadLoop` catches every exception (`:605-611`) and marks the transport failed.
  - The host returns user-controlled keys, for example `get_attributes`, which returns `{"attributes":{name:value}}` (`crates/ui4me-host/src/host/document_commands.rs:829-835`).
- **Reproduction:** `reserved_host.ps1 -Attr psbase` loads `<div id='x' psbase='1'>` and calls `get_attributes`. Result:
  - `UI4ME.DisconnectedException: ExtendedTypeSystemException: The member name "psbase" is reserved.`
  - Every later command fails.
  - The host logs `application failed (reason=Backend connection lost: peer disconnected)` and the launcher exits 1.
  - The same script with `-Attr title` works.
- **Severity:** Medium. **Likelihood:** Low, but content-triggered: any app that inspects attributes of untrusted or user-supplied HTML can be crashed by the document. It could also be triggered by any future result keyed by user data.
- **Impact:** The whole application fails from a data value. Python is unaffected.
- **Root cause:** PSObject note properties cannot use ETS-reserved names. The existing fallback to an ordinal Hashtable (`:137-143`) covers only case collisions.
- **Remediation:**
  - In `ReadObject`, before building the PSObject, check each name: empty, or (case-insensitive) one of `psobject`, `psbase`, `psadapted`, `psextended`, `pstypenames`. In that case use the existing ordinal `Hashtable` path, which means extending the `caseCollision` flag into a `needsDictionary` flag.
  - Defensively, wrap `Properties.Add` in a try/catch that falls back to the Hashtable path.
  - Unit test: `[UI4ME.Json]::Parse('{"psbase":1,"":2,"a":3}')` returns an IDictionary with all three keys, and `Get-UI4MEField` reads them.
  - Mock or native test: `get_attributes` on an element with a `psobject` attribute.
- **Alternatives:** Always return an ordinal Hashtable for objects. This is simpler but changes `.PSObject.Properties` enumeration and dot-access ergonomics for every result. Not recommended.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-02: Bursts of events the app never subscribed to fill the 128-message queue, and later user events (clicks) are dropped silently (issue #1, PowerShell side)
- **Classification:** Confirmed defect (reproduced with the mock peer). The drop policy is a deliberate design, but its consequences are not covered by any test.
- **Component:** `Transport.cs` `ReadLoop` admission (`:583-603`) and the psm1 pump.
- **Evidence:**
  - Admission is first-come: `queuedMessages < MaximumQueuedMessages && queuedBytes + size <= MaximumQueuedBytes`. Anything else that is not lifecycle is counted and dropped (`:602`).
  - The host sends every DOM event (issue #1, and the list in `html-css-scope.md:118-133`: `pointermove`, `mousemove`, `transition*` and so on).
  - Every queued event costs a full PowerShell pump: `Invoke-UI4MEPump`, then `Invoke-UI4MENotification`, then `Get-UI4MEField`, then `Invoke-UI4MEDocumentHandlers`.
  - Measured (`rate_client.ps1`, warmed up): about 0.94 ms per subscribed event (about 1000/s) and about 0.65 ms per event with no handler.
- **Reproduction:** `python3 mockpeer.py burst_client.ps1 -N 300 -Mode toplevel` sends a 300-event `transitionrun` burst, then a `click`, then `window.event closed`. Result:

  ```
  handled transitionrun=128 clicks=0 ... transportDropped=173
  ```

  The click was lost and `closed` (lifecycle) survived. With `-N 1000`, 872 events were dropped. The existing Pester test `bounds queued notifications at 128 and counts drops instead of failing` asserts 272 drops out of 400 as expected behaviour.
- **Severity:** Medium. **Likelihood:** Medium. Theme switches (about 300 transition events, as in issue #1), pointer movement during any callback longer than about 0.3-0.5 s, and animations all qualify.
- **Impact:** Clicks, input, change and submit events are lost without any user-visible signal. The counter `$app.Transport.DroppedNotifications` exists but nothing logs it.
- **Root cause:**
  - The host sends events nobody subscribed to.
  - The client admits by arrival order only.
  - PowerShell's per-event dispatch cost is about 1000 times that of a C# filter.
- **Remediation:**
  1. Do client-side interest filtering on the reader thread.
     - Add `Transport.SetInterest(string documentId, string[] eventTypes)`, backed by a lock-free snapshot such as an immutable dictionary swapped atomically.
     - Have `Register-UI4MEDocumentEvent`, `Unregister-UI4MEDocumentEvent` and `Remove-UI4MEDocumentState` maintain it.
     - In `ReadLoop`, discard a `document.event` whose type has no interest before admission. Count it separately as `FilteredNotifications`, not as dropped.
     - Keep `document.error`, `window.event` and anything with a `Register-UI4MENotification` callback.
  2. Add drop priority. When the queue is full, prefer dropping coalescible types (`pointermove`, `mousemove`, `mouseover`/`out`, `wheel`, `scroll`, `transition*`, `animation*`, `selectionchange`) over discrete ones. For example, evict the oldest coalescible message, or reserve about 32 slots for discrete events.
  3. Log once per burst to stderr when drops occur.
  4. Tests:
     - Burst of 1000 unsubscribed events plus a click: the click is delivered.
     - Burst of 300 subscribed events plus a click: the click is delivered and the drops are logged.
- **Alternatives:** Host-side subscription (the backend declares its interest and the host filters). This is the real fix for issue #1 and helps both clients and the host's queue, but it is a protocol change. Client filtering is still worth doing because it is compatible and local.
- **Complexity:** M. **Confidence:** High.

### BIND-PS-03: A lifecycle `window.event` "closed" that the transport deliberately admitted can still be dropped by the module's deferral cap
- **Classification:** Confirmed defect (reproduced).
- **Component:** `UI4ME.Core.psm1` `Invoke-UI4MEPump`, lines 283-291.
- **Evidence:** Line 286: `if ($Application.Deferred.Count -ge $script:MaximumDeferred) { $Application.DroppedNotifications++ } else { ...Enqueue... }`. There is no lifecycle exemption, while `Transport.IsLifecycle` (`Transport.cs:502-505`) exempts exactly this message.
- **Reproduction:** `mockpeer.py burst_client.ps1 -N 300 -Mode inrpc`. A click handler issues RPCs while 300 events plus `closed` arrive. Result: `closedCallback=False window.Closed=False deferredDropped=4`. The window handle stays "open" and stays in `$app.Windows`, and the `window.event` callback never sees `closed`.
- **Severity:** Medium-Low. **Likelihood:** Low-Medium. It needs more than 128 notifications during one callback dispatch, which issue #1 bursts make plausible.
- **Impact:**
  - The stale window object is not retired, so later commands fail with `RpcException -32004` instead of `StaleObjectException`.
  - Apps that track open windows can misbehave.
- **Root cause:** Two independent bounds with inconsistent lifecycle rules.
- **Remediation:**
  - In the `default` branch, if `$message.Method -eq 'window.event'` and its type is `closed`, always enqueue, bypassing the cap. The count is bounded by the number of windows.
  - Optionally, apply the bookkeeping (mark the window closed and retire the document) eagerly and defer only the user callback. Python applies it on the ordered worker; eager is safe because the callbacks would see a retired document anyway.
  - Test: the same scenario in Transport.Tests with the mock host's flood plus a closed event.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-04: `Wait-UI4ME -UntilShutdown` returns without delivering deferred final events when shutdown arrives during a callback
- **Classification:** Confirmed defect and parity gap (reproduced).
- **Component:** psm1 `Wait-UI4ME` (lines 480-491) together with `Invoke-UI4MEPump` (284) and `Close-UI4ME` (515).
- **Evidence:**
  - `app.shutdown` sets `ShutdownRequested` immediately (284), even while it and the preceding `closed` event are deferred.
  - The `-UntilShutdown` loop `while (-not $Application.ShutdownRequested)` then exits.
  - `Close-UI4ME` clears `Deferred`.
  - Python's `run()` drains ordered notifications for up to 2 s (`application.py:151-153`).
- **Reproduction:** `mockpeer.py shutdown_client.ps1`. A click handler is doing RPCs while the host sends `window.event closed` and `app.shutdown`. Result: `closed-callback-ran=False window.Closed=False deferred-left=2`.
- **Severity:** Low-Medium. **Likelihood:** Medium. The documented scenario "closing the window while a click handler waits for a native dialog" produces exactly this ordering.
- **Impact:** Final `closed` and `window.event` callbacks (for example "save state on close") silently never run.
- **Remediation:**
  - After the `-UntilShutdown` loop (and when `-not $Application.Dispatching`), drain `Deferred`, then `Transport.Receive(0)` until empty or about 2 s have passed, dispatching notifications and answering requests. This mirrors Python.
  - Test with the mock host: send a notification after `app.shutdown`, and a deferred notification before it.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-05: `Show-UI4MEFileDialog -NoWait` followed by `Wait-UI4MERpc` cancels the user's dialog after 15 s
- **Classification:** Confirmed defect (API trap; reproduced with the mock).
- **Component:** psm1 `Wait-UI4MERpc` (:402 default `TimeoutMilliseconds = 15000`) versus `Invoke-UI4MERpc` (:449, where `dialog.*` defaults to -1).
- **Evidence:** The `dialog.*` no-deadline rule lives only in `Invoke-UI4MERpc`. `Show-UI4MEFileDialog -NoWait` returns a request (:1194), and its help says "Wait-UI4MERpc awaits it". The API reference (`powershell-api.md:235-237`) says the same, without `-TimeoutMilliseconds -1`. Only `patterns-powershell.md:421` mentions `-1`.
- **Reproduction:** `mockpeer.py dialog_client.ps1`. Result:

  ```
  Wait-UI4MERpc threw after 15008 ms: RpcTimeoutException ... cancellation was requested
  ```

  The mock received `request.cancel` for the dialog.
- **Severity:** Low-Medium. **Likelihood:** Medium for users of `-NoWait`.
- **Impact:** The native dialog closes under the user after 15 s. Python's `future.result()` waits indefinitely.
- **Remediation:**
  - Store `DefaultTimeout` on the `UI4ME.Request` object in `Start-UI4MERpc`: -1 for `dialog.*`, else 15000.
  - Have `Wait-UI4MERpc` use it when `-TimeoutMilliseconds` is not bound.
  - Update the cmdlet help and the API reference.
  - Test with the mock host.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-06: `Start-UI4MEScript` re-runs the wrong file when called from a helper function (dot-sourced script or the user's own module)
- **Classification:** Confirmed defect (reproduced with the real headless host).
- **Component:** psm1 `Start-UI4MEScript`, line 1472: `$ScriptPath = $MyInvocation.PSCommandPath`.
- **Evidence:** `PSCommandPath` is the file containing the calling statement, not the entry script. The backend command becomes `pwsh -File <that file>` (:1491). Argument recovery (`Get-UI4MEScriptArguments`) also fails to match.
- **Reproduction:** `dotsrc/app.ps1` dot-sources `helper.ps1`, which defines `function Start-MyApp { Start-UI4MEScript ... }`. Result: `ui4me-host error: application failed (reason=Backend exited with exit status: 0)`, exit 1, with no hint about the cause. A user module (`.psm1`) is worse: the backend would be `-File X.psm1`.
- **Severity:** Low-Medium. **Likelihood:** Low-Medium. Factoring startup into a module or helper is common in larger PowerShell apps.
- **Remediation:**
  - Default to the entry script. In -File mode, take the script path that follows `-File`/`-f` (or the first positional argument) from `[Environment]::GetCommandLineArgs()`. Otherwise use the outermost frame of `Get-PSCallStack` that has a `ScriptName` ending in `.ps1`.
  - Fail with a clear message when the resolved file is not a `.ps1`, or when it differs from `PSCommandPath`, and suggest `-ScriptPath`.
  - Document `-ScriptPath` (it is currently undocumented in the help).
  - Test: a helper function and a module function both call `Start-UI4MEScript`.
- **Complexity:** S-M. **Confidence:** High.

### BIND-PS-07: Ctrl+C makes `pwsh -File app.ps1` exit 0, not the documented 130
- **Classification:** Confirmed defect: docs and behaviour disagree (reproduced on Linux pwsh 7.7).
- **Component:** psm1 `Start-UI4MEScript` :1527-1537, and `HostProcess.Stop` (`Transport.cs:785-789`).
- **Evidence:**
  - `powershell-api.md:113` and `:126-128` say "Ctrl+C in the launcher stops the host (exit code 130)".
  - On Ctrl+C, PowerShell stops the pipeline: the `finally` block kills the host, and `exit $code` (:1537) is never reached.
- **Reproduction:** `forever.ps1` with job control. `kill -INT <launcher>` gave exit 0. `kill -INT -<pgid>` (a true Ctrl+C) also gave exit 0. The Python launcher (`py_forever.py`) gave exit 130. `kill -TERM` gave 143, and the host and backend were gone in all cases.
- **Severity:** Low. **Likelihood:** High whenever Ctrl+C is used.
- **Impact:** Scripts, CI and process managers see "success" after an interrupt. Windows behaviour is untested.
- **Remediation:**
  - Track `$finished = $true` after the wait loop. In `finally`, if not finished, stop the host and call `[Environment]::Exit(130)`. Alternatively, register a `Console.CancelKeyPress` handler in `HostProcess` that sets `e.Cancel = true`, terminates the host and records 130.
  - Fix the docs if the behaviour is intentionally different on Windows.
  - Test in `DirectStart.Tests` (Linux): SIGINT the launcher and assert exit 130.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-08: `break`/`continue` inside user callbacks escape into the module's loops
- **Classification:** Confirmed defect, a reproducible PowerShell pitfall.
- **Component:** psm1 `Invoke-UI4MECallback` (:148, `return ,@(& $Callback @Arguments)`) and its callers. These are the `foreach` in `Invoke-UI4MEDocumentHandlers` (:166-173), the `switch` in `Invoke-UI4MEHostRequest` (:237-251), and the `while` loops in `Wait-UI4ME`/`Wait-UI4MERpc` for deferred notifications (:266 is outside any `switch`).
- **Reproduction:**
  - `flow_client.ps1 -Case break`: an `if (...) { break }` in the first click handler silently skipped the second handler for that event (`h1-1-break h1-2 h2-2`, where `h2-1` is missing). `continue` happened to behave like `return`.
  - `policy_client.ps1 -Case break`: the before-close reply became `{"result": null}`. The host treats this as a veto (`host/mod.rs:666-672`, `unwrap_or(false)`).
  - From code, not run: `break` in a `Register-UI4MENotification` callback that is dispatched from the deferred queue can break out of `Wait-UI4ME -UntilShutdown` (ending the app early) or out of `Wait-UI4MERpc` (making an RPC return no value).
  - `exit` in a callback ends the backend immediately with that code (observed exit 7). That is normal PowerShell behaviour, but undocumented.
- **Severity:** Low. **Likelihood:** Low-Medium. Using `break` as an early exit is a common mistake, and in some loops it is copied from C#.
- **Remediation:**
  - Invoke callbacks inside a sentinel loop so that `break`/`continue` terminate there: `foreach ($__ in 1) { $out = @(& $Callback @Arguments) }`. Then log a warning ("use return, not break") when the loop exited early without output. A labelled loop is another option.
  - Test: `break` and `continue` in event, notification and policy callbacks do not affect other handlers or the pump.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-09: A policy decision is `[bool]` of the last output object; strings and numbers decide too
- **Classification:** Design tradeoff with a reproducible pitfall.
- **Component:** psm1 `ConvertTo-UI4MEDecision` (:225-229).
- **Reproduction:**
  - `policy_client.ps1 -Case stringfalse`: `'false'` gave `{"allow": true}`.
  - `-Case listadd`: `$false; $list.Add('x')` gave a decision made by the `ArrayList.Add` index (0 = veto; 1 or more = allow).
  - No output gives a veto (documented).
  - A throw gives `-32603`, which the host treats as a veto (documented).
- **Severity:** Low. **Likelihood:** Medium for new PowerShell users.
- **Impact:** A window that unexpectedly cannot close (only Ctrl+Shift+Q works), or unexpectedly closes, losing data.
- **Remediation:**
  - Accept only `[bool]` (or `[System.Management.Automation.SwitchParameter]`) as the last output. Otherwise log `UI4ME: before-close policy returned <type> '<value>'; treated as veto` and veto (fail-safe).
  - Keep "no output means veto". Document both.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-10: Writes happen synchronously on the runspace thread and are not covered by RPC timeouts
- **Classification:** Reproducible risk (design difference from Python).
- **Component:** `Transport.Send` (`Transport.cs:635-660`, `stream.Write` under `writeLock`). It is called from `Start-UI4MERpc` (:366) before the timed wait.
- **Reproduction:** `mockpeer.py stall_client.ps1`. The peer stops reading for 6 s, and the client sends a 1.5 MB request with `-TimeoutMilliseconds 1000`. Result: `call 0 returned after 6008 ms (timeout was 1000)`. Ctrl+C cannot interrupt a blocked .NET write promptly.
- **Severity:** Low. **Likelihood:** Low. The host reads continuously. It matters only if the host's IPC intake stalls.
- **Impact:** The script hangs past its stated timeout. Python queues writes (256 frames or 8 MiB) and fails fast with `-32005`.
- **Remediation (choose one):**
  - A C# writer thread with a bounded outgoing queue. `Send` enqueues, or throws `RpcException(-32005)` when the queue is full.
  - `stream.WriteAsync(...).Wait(timeout)` with cancellation, marking the transport failed on timeout.
  - At minimum, document that `-TimeoutMilliseconds` covers only waiting for the reply.
- **Complexity:** M. **Confidence:** High.

### BIND-PS-11: On Linux, a module installed for all users can never be executed by non-owners (`chmod u+x`)
- **Classification:** Plausible defect. Proven from code and POSIX semantics. Not reproduced, because that needs a second user or root.
- **Component:** psm1 `Get-UI4MEHostPath`, :1316-1319: `test -x "$1" || chmod u+x -- "$1"`.
- **Evidence:** The publish workflow chmods after `Expand-Archive` (`.github/workflows/publish-powershell.yaml:169`), which confirms that NuGet extraction drops +x.
  - Under `-Scope AllUsers`, the file is owned by root. For a non-root user, `test -x` fails and `chmod` fails with EPERM, so the function throws.
  - Even after root runs it, `u+x` sets only the owner bit.
  - The docs recommend `-Scope CurrentUser` (`install-powershell.md:24,35`), which works.
- **Severity:** Low. **Likelihood:** Low to Medium, depending on how often admins use AllUsers.
- **Remediation:**
  - When the file is not executable and the current user cannot chmod it, copy the host to a per-user cache (for example `$XDG_CACHE_HOME/ui4me/<version>/ui4me-host`), `chmod 0755` the copy, verify its hash, and use it.
  - When the current user does own the file, use `chmod a+rx`.
  - Error message: "installed for all users without execute permission; run `sudo chmod a+x <path>` or install with -Scope CurrentUser".
- **Complexity:** S-M. **Confidence:** Med.

### BIND-PS-12: JSON serializer edge cases: `[switch]` refused, `[xml]` and other IEnumerables silently become nested arrays, lone surrogates give a cryptic error
- **Classification:** Confirmed behaviour (`codec.ps1`). Low-impact defects.
- **Component:** `Json.Write` (`Transport.cs:278-352`) and `Transport.Send` (`:637`, a UTF8Encoding that throws on invalid input).
- **Reproduction:**
  - `@{ v = $SwitchParam }` fails with `Cannot send a System.Management.Automation.SwitchParameter`.
  - `[xml]'<a/>'` becomes `[[]]`.
  - `"a$([char]0xD800)b"` serializes, but `GetBytes` throws "Unable to translate Unicode character \uD800". This surfaces from `Set-UI4MEDocumentText` as a `MethodInvocationException` from `Send`.
  - `[single]0.1` becomes `0.10000000149011612`.
  - BigInteger and Guid are refused, which is fine.
- **Severity:** Low. **Likelihood:** Medium for `[switch]` in hand-built `-Parameters` hashtables.
- **Remediation:**
  - Serialize `SwitchParameter` as `IsPresent`.
  - Refuse `XmlNode` and other non-collection IEnumerables explicitly (allow only `IList`, `ICollection` and arrays) rather than guessing.
  - Catch `EncoderFallbackException` in `Send` and rethrow `ArgumentException("Text contains an unpaired UTF-16 surrogate at index N")`.
  - Format `float` with `((float)v).ToString("R")`.
  - Add codec unit tests.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-13: Collection and pipeline semantics and error records are not idiomatic PowerShell
- **Classification:** Design or maintenance issues, confirmed.
- **Component:** Exported functions.
- **Evidence:**
  - `Get-UI4MEWindowList` (:592, `return ,@(...)`) emits one array object: `Get-UI4MEWindowList | Measure-Object` gives a count of 1 for 2 windows (`listpipe.ps1`). The same pattern appears in `Get-UI4MEStylesheets` (:860) and `Get-UI4MESelectOptions` (:972).
  - `Invoke-UI4MERpc` unrolls array results (`Wait-UI4MERpc` :418 `return $message.Result`): a single-element array becomes a scalar, an empty array becomes `$null`.
  - No parameter accepts pipeline input (0 of 83 commands), and no `[OutputType()]` is declared.
  - Errors are plain `throw`, so FullyQualifiedErrorId is the message text, the category is `OperationStopped`, there is no TargetObject, and `-ErrorAction` cannot suppress them (`errrec.ps1`).
- **Severity:** Low.
- **Impact:** Surprising results in pipelines. Errors are hard to filter in `try`/`catch` except by .NET type, which does work: `catch [UI4ME.RpcException]`.
- **Remediation:**
  - Emit list items normally (`$items` without the comma), or keep the current behaviour and document "returns an array as one object".
  - Make `Invoke-UI4MERpc` return results unchanged with `Write-Output -NoEnumerate`.
  - Add `[OutputType()]`.
  - Use `$PSCmdlet.ThrowTerminatingError([ErrorRecord]::new($ex, 'UI4ME.<Code>', <category>, $target))` for typed errors.
  - Pipeline binding (for example `-Document` with `ValueFromPipeline`) is optional.
  - Changing return shapes after 1.0 breaks callers, so batch these into a 2.0 or add new switches.
- **Complexity:** M. **Confidence:** High.

### BIND-PS-14: Naming and API-surface inconsistencies
- **Classification:** Maintenance and DX.
- **Evidence:**
  - Get/Set pairs do not share nouns: `Get-UI4MEValue`/`Set-UI4MEDocumentValue`, versus `Get-`/`Set-UI4MEDocumentText`, versus `Get-`/`Set-UI4MEChecked`. Also `Set-UI4MEDocumentAttribute` versus `Set-UI4MEClass`.
  - Plural nouns (PSScriptAnalyzer `PSUseSingularNouns`): `Get-UI4MENodes`, `Get-UI4MEStylesheets`, `Get-UI4MESelectOptions`, `Register-`/`Unregister-UI4MEResources`, `Clear-UI4MEUnusedResources`, `Set-UI4MESelectedValues`.
  - `Show-UI4MEPopover -Action hide` and `Show-UI4MEHtmlDialog -Close` use a "Show" verb for hiding.
  - `-EventType` (`Set-UI4MEPreventDefault`) versus `-Type` (`Register-UI4MEDocumentEvent`).
  - `-Form`/`-FormNode` versus `-Element`/`-Node`.
  - `Get-UI4MEWindow` returns a state description, not a window handle.
  - `Register-UI4MENotification` allows one callback per method (:645-658), so a second registration for `window.event` silently replaces the first. Python's `on_window_event` supports multiple listeners and unsubscribe.
- **Severity:** Low.
- **Remediation:**
  - Add aliases or new consistent names (for example `Get-UI4MEDocumentValue`) without removing the old ones.
  - Add `Register-UI4MEWindowEvent` with multiple listeners and an unsubscribe handle.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-15: No runspace or thread-affinity guard
- **Classification:** Reproducible risk.
- **Component:** psm1 `Start-UI4MERpc` (:358 `$Application.NextId++`, not atomic) and the shared `Pending`/`Replies`/`Deferred` hashtables.
- **Reproduction:** `thread_client.ps1`. A `Start-ThreadJob` using `$using:doc` and the main thread each did 300 `Set-UI4MEDocumentText`. The job saw 2 failures: `Exception calling "Register" ... "Duplicate request ID"`. Callbacks can also run on the job's runspace when it pumps.
- **Severity:** Low. **Likelihood:** Low. The docs forbid it (`powershell-api.md:582`, patterns §12), but they also recommend thread jobs for long work, which tempts users to pass `$doc`.
- **Remediation:**
  - Record `[Runspace]::DefaultRunspace.InstanceId` (or `[Threading.Thread]::CurrentThread.ManagedThreadId`) in `Connect-UI4ME`.
  - In `Assert-UI4MEApplication`, throw `InvalidOperationException('UI4ME commands must run on the runspace that connected; use Wait-UI4ME polling and job output instead')`.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-16: Runtime `Add-Type` cost and constraints; helper compatibility check is presence-based
- **Classification:** Design tradeoff with maintenance risk.
- **Component:** psm1 `Initialize-UI4MEHelper` (:25-38).
- **Evidence:**
  - Measured 307-530 ms per process on pwsh 7 Linux. A direct start compiles twice, once in the launcher (for `HostProcess`) and once in the backend.
  - Windows PowerShell 5.1 uses CodeDOM csc, which is typically slower. Not measured.
  - `Add-Type` is blocked under Constrained Language Mode (AppLocker/WDAC). Inferred, not tested.
  - The "incompatible helper" check (:34) tests only for the presence of `Register`, `MappedBitmap` and `HostProcess`. A newer module imported into a session that already holds older compiled types silently reuses the old types.
- **Severity:** Low.
- **Remediation:**
  - Add `public const string HelperVersion = "1.0.1"` in C# and compare it with the module version in `Initialize-UI4MEHelper`.
  - Optionally ship a signed precompiled netstandard2.0 assembly (it loads on .NET Framework 4.7.2 and on .NET 6+) with source fallback. This is a supply-chain tradeoff.
  - In the backend, consider skipping work that is not needed.
- **Complexity:** S (version check), M (precompiled). **Confidence:** Med.

### BIND-PS-17: Callbacks resolve unqualified variables through whatever call stack is pumping
- **Classification:** Reproducible PowerShell pitfall (docs are incomplete).
- **Component:** psm1 `Invoke-UI4MECallback` (:148), which follows PowerShell's dynamic scoping.
- **Reproduction:** `scope_client.ps1`. The same top-level handler printed `status=function-local` when the event was pumped inside a function's RPC wait, and `status=script-level` when pumped at script level. `$counter++` without `$script:` updated a local copy, as documented. The `.GetNewClosure()` and `$script:` behaviour matched the docs (`closure_client.ps1`).
- **Severity:** Low.
- **Impact:** Heisenbugs that depend on where pumping happens.
- **Remediation:** Docs (`powershell-api.md` "Scriptblocks and variables"): state that unqualified reads in callbacks can see the locals of whichever function is waiting, and recommend `$script:name` for shared reads too, or a state hashtable. No code change is needed.
- **Complexity:** S. **Confidence:** High.

### BIND-PS-18: On Windows, launcher output may lose non-ASCII text (unverified)
- **Classification:** Plausible concern. Windows only; reasoned from code, not run.
- **Component:** `HostProcess.Relay` (`Transport.cs:796-801`) writes through `Console.Out`/`Console.Error`. The backend role sets `[Console]::OutputEncoding = UTF8` (psm1 :1455), but the launcher role does not.
- **Evidence:** On a Windows console, .NET writes through `Console.OutputEncoding` (the OEM code page by default). Python's launcher writes through `sys.stdout`, which uses `WriteConsoleW` on consoles.
- **Severity:** Low. **Likelihood:** Medium on non-UTF-8 Windows consoles.
- **Remediation:** Verify on winDEV with `Write-Output 'Grüße 😀'` from the backend. If it is broken, write to the console with `WriteConsoleW` when stdout is a console, or set `Console.OutputEncoding` to UTF-8 for the duration of the launcher wait (and restore it).
- **Complexity:** S. **Confidence:** Low.

### BIND-PS-19: `Start-UI4MEHost` (wait mode) prints raw JSON host log records
- **Classification:** DX inconsistency, confirmed from code.
- **Component:** psm1 :1340 `& $hostBinary run --app $full`. The host's stderr goes straight to the console. `Start-UI4MEScript` humanizes it through `HostProcess.RelayLine`.
- **Severity:** Low.
- **Remediation:** Reuse `UI4ME.HostProcess` in `Start-UI4MEHost` (with the same Ctrl+C and exit-code handling as BIND-PS-07).
- **Complexity:** S. **Confidence:** High.

## 4. Test gaps and highest-value tests to add

1. **Codec:** reserved and empty member names (PS-01). Unit test plus mock or native `get_attributes`.
2. **Burst survival:** 1000 unsubscribed events plus a click, and 300 subscribed events plus a click. The click must be delivered and the drops logged (PS-02). Replace or extend the current "272 dropped" assertion.
3. **Lifecycle under deferral:** more than 128 notifications plus `closed` during a dispatching callback. Assert `$win.Closed` and that the `window.event` callback ran (PS-03).
4. **Shutdown drain:** `closed` and `app.shutdown` during a callback. Assert that callbacks ran before `Wait-UI4ME -UntilShutdown` returns (PS-04).
5. **`-NoWait` dialog with `Wait-UI4MERpc` default:** no timeout or cancel within 20 s (PS-05).
6. **`Start-UI4MEScript` from a helper function and from a module function (PS-06).** Native headless; add to DirectStart.Tests.
7. **Ctrl+C:** SIGINT to the launcher, assert exit 130 (PS-07). Linux, in DirectStart.Tests.
8. **Flow control:** `break`/`continue` in event, notification and policy callbacks (PS-08).
9. **Policy coercion:** non-bool last output (PS-09).
10. **Write stall:** peer stops reading, and an RPC with a timeout returns within the timeout (only if PS-10 is fixed).
11. **Runspace guard:** a ThreadJob call fails with a clear error (PS-15).

Coverage notes:
- The 5.1 path (CodeDOM compile, .NET Framework pipes, `QuoteWindowsArgument` in real use) was not exercised here. `native/Qualify.ps1` plus `app-ps51.toml` exist for winDEV.
- The burst tests should also run on Windows named pipes, where issue #1 was observed.

## 5. Things I recommend NOT changing

- **Keep the single-runspace callback model.** Do not run user scriptblocks on .NET threads or runspace pools: cross-runspace scriptblock invocation is unsafe, and serial in-order delivery is easy to reason about.
- **Keep the custom JSON codec.** Do not switch to `ConvertFrom-Json`/`ConvertTo-Json`: date coercion, depth truncation, 5.1/7 differences and single-element unrolling are exactly what it avoids.
- **Keep the transport structure:** reader-only parsing, lifecycle bypass, the separate rejecter thread, fatal protocol violations, and discarding responses to unregistered IDs.
- **Keep "no output or throw means veto"** as the fail-safe policy default. Only tighten the non-bool case (PS-09).
- **Keep `Close-UI4ME` deferring the disconnect in a direct-start backend while an error unwinds.** It keeps the error visible, and the process exits right after. Documenting that the windows stay until process exit is enough.
- **Keep `-ExecutionPolicy Bypass` for the backend.** The user has already run the script, and a machine-policy GPO still overrides it.
- **Keep compiling the helper lazily** at Connect or Start time, not at import.

## 6. Open questions

- Windows PowerShell 5.1 and pwsh on Windows were not run:
  - Ctrl+C exit code
  - Console encoding of relayed output (PS-18)
  - Add-Type time with csc
  - Named-pipe Dispose with a pending overlapped read on .NET Framework
- Is Constrained Language Mode a supported target? If so, `Add-Type` rules it out.
- Does PSResourceGet extraction always drop +x on Linux, so that AllUsers installs are always affected (PS-11)?
- Should the drop policy (PS-02) be fixed host-side (subscriptions) as part of issue #1/PR #3, with the client pre-filter as defence in depth? I recommend both.

## Appendix A: Exported PowerShell commands, what they do, and the protocol they map to

All `document.command` ops are listed as `op`.

| Command | One-line description | Protocol |
| --- | --- | --- |
| Connect-UI4ME | Connect to the host that launched this process, using env endpoint and token | `host.hello` |
| Close-UI4ME | Retire all handles and close the transport (deferred in a direct-start backend before shutdown) | none (disconnect) |
| Wait-UI4ME | Pump: run callbacks and answer lifecycle requests; `-UntilShutdown` loops | answers `window.before_close`, `app.before_quit`, `app.shutdown` |
| Invoke-UI4MERpc | Call any method; 15 s default, -1 for `dialog.*` | any |
| Send-UI4MENotification | Send a notification | any notification |
| Start-UI4MERpc / Wait-UI4MERpc | Async request and await; Wait defaults to 15 s for every method (PS-05) | any |
| Stop-UI4MERpc | Cancel a request | `request.cancel` (notification) |
| New-UI4MEWindow | Create a window (`-Resizable`, which Python lacks) | `window.create` |
| Set-UI4MEWindow | Set window state and constraints | `window.set` |
| Get-UI4MEWindow | Window description | `window.get` |
| Get-UI4MEWindowList | All windows (returns one array object) | `window.list` |
| Close-UI4MEWindow | Graceful close request | `window.request_close` |
| Request-UI4MEAttention | Flag the window (`none` maps to null) | `window.request_attention` |
| Set-UI4MEBeforeClose | Per-window close policy (local) | answers `window.before_close` |
| Set-UI4MEBeforeQuit | Quit policy (local) | answers `app.before_quit` |
| Set-UI4MEReady | Startup finished | `app.ready` |
| Request-UI4MEQuit | Orderly quit | `app.request_quit` |
| Register-UI4MENotification | One callback per notification method (local) | any notification (`window.event`, `desktop.preferences_changed`, `request.cancelled`, ...) |
| Open-UI4MEHtml / Open-UI4MEFile | Load a document (30 s timeout) | `document.load` |
| Remove-UI4MEDocument | Unload | `document.unload` |
| Register-/Unregister-UI4MEDocumentEvent | Event subscription (local, 256 per document) | consumes `document.event` |
| Invoke-UI4MEDocumentCommand | Any op | `document.command` |
| Invoke-UI4MEBatch | Atomic id-addressed batch | `batch` |
| Get-/Set-UI4MEDocumentText | Text | `get_text` / `set_text` |
| Get-UI4MEValue / Set-UI4MEDocumentValue | Control value | `get_value` / `set_value` |
| Get-/Set-UI4MEChecked | Checked state | `get_checked` / `set_checked` |
| Set-UI4MEDocumentAttribute [-Remove] | Attribute | `set_attribute` / `remove_attribute` |
| Set-UI4MEClass [-Remove] | Class | `add_class` / `remove_class` |
| Set-UI4MEDocumentHtml [-Position] | Trusted HTML | `replace_children_html` / `insert_html` |
| Remove-UI4MEElement | Remove by id | `remove_element` |
| Find-UI4MEElement | Selector query (ids) | `query` |
| Add-UI4MEStylesheet | Add CSS | `add_stylesheet` |
| Get-UI4MEStylesheets | List sheets | `list_stylesheets` |
| Set-UI4MEStylesheet [-Remove] | Replace or remove a sheet | `replace_stylesheet` / `remove_stylesheet` |
| Get-UI4MEComputedStyle | Computed values | `get_computed_style` |
| Get-/Set-UI4MEEnvironment | Color-scheme and motion overrides | `get_environment` / `set_environment` |
| Get-/Set-UI4MEScroll | Scroll position | `get_scroll` / `scroll_to` |
| Get-UI4MEFocusedElement | Focused element | `get_focused_element` |
| Set-UI4MEFocus [-Blur] | Focus or blur | `focus` / `blur` |
| Get-/Set-UI4MESelection | Control selection (UTF-16) | `get_control_selection` / `set_control_selection` |
| Invoke-UI4MEEditCommand | Native editing command | `edit_command` |
| Get-UI4MEFormData | Ordered form entries | `get_form_data` |
| Submit-UI4MEForm | Request submit | `request_submit` |
| Reset-UI4MEForm | Reset | `reset_form` |
| Get-UI4MESelectOptions | Select options | `get_options` |
| Set-UI4MESelectedValues | Select values | `set_selected_values` |
| Show-UI4MEPopover -Action | Show, hide or toggle | `show_popover` / `hide_popover` / `toggle_popover` |
| Show-UI4MEHtmlDialog [-Close] | HTML `<dialog>` | `show_dialog` / `close_dialog` |
| Set-UI4MEPreventDefault | Default-action rule | `set_default_action` |
| Get-UI4MEDiagnostics | Diagnostics | `get_diagnostics` |
| Get-UI4MEResourceStats | Resource stats | `get_resource_stats` |
| Clear-UI4MEUnusedResources | Purge | `purge_unused_resources` |
| Register-/Unregister-UI4MEResources | `ui4me-res://` bundles | `register_resource_bundle` / `unregister_resource_bundle` |
| Get-UI4MENode | id or root to a node, or wrap `-Reference` | `node_of` |
| Get-UI4MENodes | Selector query (nodes) | `query_nodes` |
| Get-UI4MENodeInfo | Node description | `get_node` |
| Get-UI4MENodeChildren | Children page | `node_children` |
| Invoke-UI4MENodeBatch | Atomic node batch | `node_batch` |
| Invoke-UI4MENodeOperation | One node op (move, trim, append_text, ...) | `node_batch` (1 op) |
| Get-UI4MECapabilities | Host capabilities | `host.capabilities` |
| Get-UI4MEPreferences | Desktop preferences | `desktop.preferences` |
| Get-/Set-UI4MEClipboard | Clipboard text | `clipboard.read_text` / `clipboard.write_text` |
| Open-UI4MEExternal [-Reveal] | Open a URL or file, or reveal it (60 s) | `desktop.open_external` / `desktop.reveal_file` |
| Show-UI4MEFileDialog [-NoWait] | Native dialogs (no deadline unless `-NoWait` plus `Wait-UI4MERpc`) | `dialog.open_files` / `dialog.save_file` / `dialog.choose_folder` |
| New-UI4MELiveBitmap | Mapped RGBA8 bitmap | `bitmap.create` (plus `bitmap.close` on failure) |
| Send-UI4MELiveBitmap [-Try] | Present a frame | `bitmap.present` |
| Remove-UI4MELiveBitmap | Release | `bitmap.close` |
| Get-UI4MEHostPath | Bundled host path (x64 Linux and Windows) | none |
| Start-UI4MEHost | Run a manifest (raw log output) | host CLI `run --app` |
| Start-UI4MEScript | Dual-role direct start | launcher, then `host.hello` |
| New-UI4MEProject | Scaffold | none |
| Install-UI4MESkill | Agent skill install, status, uninstall | none |

**Gaps.**
- Protocol methods with no typed PowerShell command: `host.diagnostics`, `app.force_quit`, `document.get`, `desktop.paths`.
- Ops Python has typed but PowerShell lacks: `element_at_point`, `replace_control_range`, `get_files`, `get_validity`, `check_validity`, `set_custom_validity`, `scroll_into_view`.
- Ops with no typed wrapper in either binding:
  - `get_attribute`, `get_attributes`, `has_element`, `get_html`, document-level `append_text`/`move_element`/`trim_children`
  - `text_at_point`, `text_rects`, `get_rect`, `css_profile`, `explain`
  - `get_selection`, `clear_selection`, `select_text`, `copy_selection`
  - `scroll_by`, `set_scroll_follow`, `get_dialog`, `get_popover`, `set_pointer_capture`/`get_pointer_capture`, `get_accessibility_tree`
- Semantics that differ from Python:
  - One `window.event` callback only, versus Python's listener list (PS-14).
  - `Wait-UI4MERpc` default 15 s, versus Python `future.result()` with no limit (PS-05).
  - No shutdown drain (PS-04).
  - Event objects are shared and mutated across handlers (PowerShell adds `window_id`/`document_id` note properties to the same object), whereas Python passes each handler a copy.
  - `Show-UI4MEFileDialog -Directory` is resolved to a full path; Python passes it as given.
  - The `New-UI4MEWindow` default title is `''`; Python's is `"UI4ME"`.
