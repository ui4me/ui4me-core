# F-12 / RENDER-04 / T3: headless frame pump and virtual clock (Linux, headless, debug build)

Baseline binary: `target/debug/ui4me-host` built from the branch base (db914b2), copied aside.

## T3 burst (`tools/test_headless_clock.py::test_t3_transition_burst_on_the_virtual_clock`)

100 buttons, 3 transitioned longhands each, 6 colour-scheme switches, `test.advance_time {ms:200}`
after each switch, `get_text` round trip after each round.

| Host | Result |
| --- | --- |
| baseline | FAIL: `test.advance_time` -> `-32601 Unknown method` (and headless emitted 0 transition events, coordinator E1) |
| this branch | PASS: every round 300 `transitionrun` + 300 `transitionstart` + 300 `transitionend`, all before the `advance_time` answer, per element/property run < start < end, identical sequence in every round, exit 0 |

Finding for the renderer owners (not changed here): `burst2.py`'s `border-color` shorthand expands
to four longhands, i.e. 6 transitions per button = 600 events of each kind in one tick. The
renderer's `MAX_ANIMATION_EVENTS = 512` (`crates/ui4me-renderer/src/document/css_animation.rs`)
dispatches the first 512 per drain and drops the rest silently (not counted in
`dropped_event_count`). Observed: exactly 512 of each kind per round with the original CSS. The
test therefore uses `outline-color` to assert exact delivery.

## RENDER-04 soak (`test_t3_ten_thousand_writes_without_reads_keep_memory_bounded`)

10,000 `set_text` commands (64 pipelined), no reads; host VmRSS (KiB) sampled every 1,000 writes.

| writes | baseline | this branch |
| --- | --- | --- |
| 1,000 | 47,144 | 52,008 |
| 2,000 | 47,716 | 52,364 |
| 4,000 | 48,664 | 52,896 |
| 6,000 | 49,744 | 52,992 |
| 8,000 | 50,240 | 53,024 |
| 10,000 | 51,124 | 53,032 |

Growth 4,000 -> 10,000: baseline 2,460 KiB (linear, ~0.41 KiB/write), this branch 136 KiB (flat).
Test threshold: < 1,024 KiB. Re-run on baseline: `UI4ME_HOST_BINARY=<baseline> python3 -m
unittest tools.test_headless_clock` -> both tests FAIL (2,464 KiB; -32601).

## Unchanged default behaviour

A backend that never calls `test.advance_time` keeps the real-time document clock. Stages
`headless`, `renderer`, `faults` of `tools/verify.py`: PASS with the frame pump.

## Renderer change

`Document::frame_without_surface()` (crates/ui4me-renderer/src/document.rs, 16 lines): what
`render_into` does minus the raster. No other renderer change; the manual clock
(`use_manual_clock`/`advance_clock`) already existed.
