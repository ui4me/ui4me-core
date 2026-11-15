# Windows: F-04 / issue #2 and workstream C tests at integration `a02d21a`

winDEV, release host built from `a02d21a`; interactive RDP session of `ui4me_audit`.

## Native tests (release)

| Command | Result |
| --- | --- |
| `cargo test --release -p ui4me-renderer --lib -- font_identity_tests family_resolution_tests append_cliff event_queue_tests geometry_tests interact_tests` | 50 passed, 1 ignored |
| `cargo test --release -p ui4me-renderer --test font_identity_soak` (gate R4a) | passed |

## Windowed issue #2 replay

Command: `issue2_replay_long.py` with `LEAK_CSS=custom`, `LEAK_CSSTEXT=.u4-badge { font-weight: 600; }`, `LEAK_SECS=300`.
The host was sampled with `Get-Process ui4me-host` every 15 s (private bytes / working set, KiB). The first two attempts
were discarded: one used an invalid app id, the other sampled `getppid()`, which is not the host for the 1.0.1 wheel.

| t (s) | 1.0.1 private | 1.0.1 working set | fixed private | fixed working set |
| --- | --- | --- | --- | --- |
| 15 | 22,540 | 46,696 | 23,692 | 47,572 |
| 105 | 23,340 | 47,568 | 23,916 | 47,708 |
| 195 | 23,904 | 29,164 | 23,888 | 28,828 |
| 300 | 24,136 | 18,608 | 24,480 | 18,492 |

**Result:** the issue #2 growth does **not** reproduce on Windows. Both hosts stay flat (+1.6 MB and +0.8 MB of
private bytes in 5 min, comparable to the audit's non-leaking control). The working set shrinks because Windows
trims it. The likely reason is that on DirectWrite the 600-weight badge does not load a separate face from the
ticking text, so parley never reloads a file.

The leak itself is a Linux/fontconfig observation (audit E5, workstream C windowed run: 1.0.1 grows to 296 MiB in
900 s; fixed stays at 44–55 MiB). On Windows the fixed build shows no regression, and the identity unit test and the
counting-allocator soak pass natively.
