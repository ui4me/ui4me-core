// owner: 007-resources (T074) — US5 T4 checkpoint: the bomb suite with peak RSS recorded.
//
//   cargo test -p ui4me-resources --test bombs -- --ignored --nocapture
//
// Runs alone in its own process so VmHWM reflects only the bombs. Peak RSS must stay at or below the
// configured decoded-memory budget (default 512 MiB) plus 10%, and every bomb must be rejected with
// SIZE_LIMIT / BUDGET_EXCEEDED (or the canonical decode code) within 1 s (SC-005).
#![cfg(unix)]
#![allow(clippy::duplicate_mod)]

mod support;

use serde_json::Value;
use std::time::Duration;
use support::bombs::{BOMBS, peak_rss_kib};

#[path = "limits.rs"]
#[allow(dead_code)]
mod limits;

#[test]
#[ignore = "T4 checkpoint; run with --ignored"]
fn bomb_suite_peak_rss_within_budget_plus_ten_percent() {
    let budget_kib = 512 * 1024u64;
    let baseline = peak_rss_kib();
    let mut worst = Duration::ZERO;
    let mut rows = Vec::new();
    for det in [false, true] {
        for case in limits::runnable_cases() {
            let (code, elapsed) = limits::run_bomb(&case, det);
            assert_eq!(
                code,
                case["expect"].as_str().unwrap(),
                "{} (det={det})",
                case["id"]
            );
            let limit = case
                .get("within_ms")
                .and_then(Value::as_u64)
                .unwrap_or(1000);
            assert!(
                elapsed < Duration::from_millis(limit),
                "{} took {elapsed:?}",
                case["id"]
            );
            worst = worst.max(elapsed);
            rows.push(format!(
                "  {:<40} det={:<5} {code:<22} {elapsed:?}",
                case["id"].as_str().unwrap(),
                det
            ));
        }
    }
    let peak = peak_rss_kib();
    println!("bomb cases: {}", rows.len());
    for r in &rows {
        println!("{r}");
    }
    println!(
        "baseline VmHWM {baseline} KiB, peak VmHWM {peak} KiB, budget {budget_kib} KiB (+10% = {} KiB), slowest {worst:?}",
        budget_kib * 11 / 10
    );
    assert!(
        peak <= budget_kib * 11 / 10,
        "peak RSS {peak} KiB exceeds budget + 10%"
    );
    let _ = BOMBS;
}
