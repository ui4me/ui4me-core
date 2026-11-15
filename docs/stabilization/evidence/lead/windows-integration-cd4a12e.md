# Windows verification of the integrated tree at cd4a12e (A–H merged)

winDEV (Windows Server 2022), workspace `F:\agent-workspaces\ui4me-core\stab-374b06f6`, Rust 1.99.1 MSVC,
release profile, driven by [`test_h.cmd`](test_h.cmd) over SSH (headless: these are cargo tests, no window).

| Check | Result |
| --- | --- |
| `ui4me-host.exe` PE `SizeOfStackReserve` | 33554432 (32.00 MiB) |
| `cargo test --release -p ui4me-host` | 109 lib + 1 + 8 integration passed (1 ignored) |
| T1 `ui4me-renderer --test stack_depth` (now incl. inline-block and button, depth 256) | 8 passed, 2 ignored |
| F-14/F-15 `ui4me-renderer --test content_stalls` | 13 passed, 1 ignored (measurement) |
| INPUTSEC-03 `ui4me-assets --test svg_filter_cost` (weights calibrated on Linux) | 5 passed, 1 ignored (calibration tool) |
| T1 `ui4me-assets --test svg_depth` | 8 passed, 1 ignored |
| `ui4me-renderer --lib` (pixel + incremental-vs-cold oracles) | 553 passed, 10 ignored |

Linux at the same commit: `python3 tools/verify.py --keep-going --jobs 8`: fmt, check, test, python, headless,
renderer, faults all PASS.
