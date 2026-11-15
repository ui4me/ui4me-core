# F-14 / HOST-06 follow-up: first layout of many inline `<input>`s (Linux, release)

Reported by workstream G (release re-measure of HOST-06): 1,000 / 2,000 / 4,000 inline inputs
0.52 / 2.33 / 17.3 s; each in its own `<div>` 0.42 / 0.48 / 1.94 s; `request_submit` itself linear.

Reproduction (`parse_hosted` + first render, 800x600, `UI4ME_PROFILE=1`; a scratch test since
replaced by `content_stalls.rs` `many_inline_inputs_lay_out_in_linear_time`):

| case | before: render ms (layout `compute`) | after: render ms (layout `compute`) |
|---|---|---|
| 1,000 inline inputs, one paragraph | 627 (225) | 383 (3.7) |
| 2,000 | 2,522 (1,957) | 567 (13.7) |
| 4,000 | 16,960 (16,037) | 950 (52) |
| 1,000 / 2,000 / 4,000 inputs, one per `<div>` | 455 / 706 / 1,238 (3 / 6 / 18) | unchanged (no long paragraph) |

The rest of the time is linear: ~180 ms first-use text set-up per document and ~160 us per input
in `tree.add` (shaping each control's value; text services, workstream C).

Root cause (gdb samples: `SnapTable::widths` under `Breaker::next` / `Breaker::set_line`):
`text/paragraph/snap.rs` `choose_with` (line choice) and `resync` (re-synchronising with the line
Parley committed) measured every candidate line end up to the end of the paragraph with a fresh
sum over the line: O(candidates x line) per line, cubic per paragraph. Fix: one running sum per
line from its start, in the same left-to-right order and from the same start value as `widths`,
so every measured width is bit-identical (`SnapTable::running` / `widths_in`).

Regression test: `many_inline_inputs_lay_out_in_linear_time` (4x the inputs may cost at most 8x;
400 px wide warm document): after 1,000 / 4,000 inputs ~0.2 / ~0.8 s; baseline 0.87 / 48.4 s FAILED.

The wrapped case (one input per `<div>`) was never in the line breaker; its growth (1.9 s at 4,000 in
G's measurement, 1.24 s here) is the per-control shaping above plus the per-document set-up.
