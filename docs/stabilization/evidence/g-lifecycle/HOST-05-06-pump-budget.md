# HOST-05 / HOST-06 / roadmap 2.4: time-budgeted pump, gate R5(c), request_submit re-measure

## Gate R5(c): longest GUI-thread turn while 1,000 queued commands drain

`tools/headless_backend.py drain 1000 ROWS`: 1,000 pipelined `document.command`s, alternating
`set_text` and `get_rect` (each read forces an update), headless debug host on Linux.
`longest_turn_ms` = `host.diagnostics` `pump.longest_turn_ms` (the longest time frames and
input waited for backend work in one loop turn).

| Pump | rows | longest turn | drain time |
| --- | --- | --- | --- |
| 64 messages only (budget disabled, otherwise this branch) | 20 | 230 ms | 3.5 s |
| 64 messages only | 50 | 486 ms | 7.4 s |
| 8 ms / 64 messages + re-wake (this branch) | 20 | 18 ms | 3.4 s |
| 8 ms / 64 messages + re-wake | 50 | 19 ms | 7.7 s |
| 8 ms / 64 messages + re-wake | 200 | 74 ms (one get_rect costs ~30-50 ms in debug) | 27 s |

Regression test: `tools/test_headless_clock.py::test_r5c_turns_stay_short_while_1000_queued_commands_drain`
(20 rows, < 50 ms), `host::tests::pump_budget_yields_and_rewakes_until_the_queue_is_drained`
(HOST-05: a yield re-wakes the loop). A single command that alone exceeds the budget still runs
to completion; it is logged when it takes >= 100 ms (`backend message took long on the GUI
thread`, with `operation` = method or `document.command <op>`). Native (windowed) frame gaps were
not measured here (headless only); the native loop uses the same pump and the wake proxy.

## HOST-06: request_submit on N empty required inputs (release build)

Release host: `a-transport/target/release/ui4me-host` (eae0ad9, F-02 fix, same renderer form
code as this branch). `tools/headless_backend.py submit N [submit|blocks]`.

| N | load + first layout (inline inputs) | request_submit | load (each input in a `<div>`) |
| --- | --- | --- | --- |
| 250 | 0.30 s | 11 ms | |
| 500 | 0.30 s | 17 ms | |
| 1000 | 0.52 s | 24 ms | 0.42 s |
| 2000 | 2.33 s | 31 ms | 0.48 s |
| 4000 | 17.3 s | 44 ms | 1.94 s |

`request_submit` itself (host dispatch + renderer form validation + N `invalid` events) is
linear and fast in release. The superlinear cost is the **first layout of thousands of inline
`<input>`s in one inline formatting context** (17 s at 4000; 1.9 s when each is block-wrapped).
The audit's debug numbers (1000 -> 12 s) measured it inside `request_submit` because the old
headless host never laid the page out before the first read. With the F-12 frame pump the same
cost now appears in the frame after `document.load` (debug: 1000 -> 4.2 s turn, 2000 -> 33 s;
`request_submit` 0.13 s / 0.27 s). This is renderer layout (inline/line layout), not host or
renderer form code: NOT fixed here; handed to the layout owners.
