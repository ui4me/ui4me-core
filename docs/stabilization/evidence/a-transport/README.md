# Workstream A (a-transport): F-02 / issue #1 evidence

Branch `stab/a-transport`. Baseline: `1e49a6a` (1.0.1 + audit docs). Fix commits: `c90062f` (HOST-01,
byte-only outbound bound), `78df633` (HOST-09, one write per frame), `8c8cab3` (HOST-08, log once,
force-quit label). Linux only; Windows was type-checked, not run.

## Files

| File | What it shows |
| --- | --- |
| `unit-tests-baseline.txt` | New transport tests against the baseline `transport.rs`: T2 fails at message 236 of 10,221 ("sending on a full channel"); the byte-cap test is refused by the 128-message count after 233 frames. The write-deadline test passes on both. |
| `unit-tests-fixed.txt` | The same tests plus the framing and HOST-08 tests on the fix: all pass. T2 moved 10,221 frames (4,458,257 wire bytes) to a 1 frame/ms reader, in order, without a disconnect. |
| `kernel-slack-linux.txt` | Frames the Linux kernel absorbs before the writer blocks: 104 (two writes per frame) vs 167 (one write). |
| `burst2-linux-windowed.txt` | `burst2.py` N=20 and N=100, 6 scheme switches, 3 runs each, baseline vs fixed release host in a private nested `kwin_wayland --virtual`. |
| `burst_measure.py` | Wrapper that runs `burst2.py` unchanged and counts the frames and bytes the backend's reader receives. |

## Results (Linux windowed, release host, Python client from the repo)

| Host | N | Runs passed | Frames / bytes received per run | Host WARN lines per failed run |
| --- | --- | --- | --- | --- |
| baseline | 20 | 1 / 3 (failed at round 5, round 1) | 1,823 / 693,585 (passing run) | 37, 3 |
| baseline | 100 | 0 / 3 (all failed after round 1) | n/a (backend killed) | 160, 254, 209 |
| fixed | 20 | 3 / 3 | 1,823 / 693,585 | 0 |
| fixed | 100 | 3 / 3 | 5,143 / 1,980,215 | 0 |

The baseline failure is always `peer cannot receive bounded output (... sending on a full channel)`, then
`application failed (Backend connection lost: closed by host)`, followed by one
`peer cannot receive bounded output (error=connection is closed)` line for every further message (HOST-08).

With the fixed host the backend was never disconnected, but the Python binding still dropped
notifications on its side in 4 of 6 runs (`client_dropped_notifications` 61 at N=20; 22, 377 and 169 at
N=100). That is F-29 (workstream E), not the host: the frame counts received by the reader are the same in
every run. The acceptance for this workstream is "no disconnect".

## Reproduce

```bash
export CARGO_BUILD_JOBS=3
cargo test -p ui4me-host -j 3 --lib transport:: -- --nocapture
cargo test -p ui4me-host -j 3 --lib refused_output
cargo build --locked --release -p ui4me-host -j 3
env -u WAYLAND_DISPLAY -u DISPLAY kwin_wayland --virtual --socket ui4me-wsa-0 --width 1280 --height 900 --no-lockscreen &
env -u DISPLAY WAYLAND_DISPLAY=ui4me-wsa-0 PYTHONPATH=python UI4ME_HOST_BINARY=target/release/ui4me-host \
  timeout 120 python3 docs/stabilization/evidence/a-transport/burst_measure.py \
  docs/audit/evidence/scripts/burst2.py 100 6 env
```

A passing run prints `ROUND_OK 0` to `ROUND_OK 5`, `PROBE_PASS` and a `MEASURE` line.
