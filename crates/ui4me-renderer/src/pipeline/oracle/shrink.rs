//! owner: 005-pipeline-diagnostics — T060. Delta-debugging shrinker (ddmin): reduces a failing
//! operation sequence to a minimal one that still fails, so a mismatch is reported as
//! `seed + minimal op list` (recompute-plan §8).

use super::driver::Op;

/// The minimal sub-sequence of `ops` (1-minimal: removing any single op makes it pass) for which
/// `fails` still holds. `fails(ops)` must hold for the input.
pub fn shrink(ops: &[Op], mut fails: impl FnMut(&[Op]) -> bool) -> Vec<Op> {
    let mut current: Vec<Op> = ops.to_vec();
    debug_assert!(fails(&current), "shrink needs a failing sequence");
    let mut granularity = 2usize;
    while current.len() >= 2 {
        let chunk = current.len().div_ceil(granularity);
        let mut reduced = false;
        // try removing each chunk (the complement)
        let mut start = 0;
        while start < current.len() {
            let end = (start + chunk).min(current.len());
            let mut candidate = current[..start].to_vec();
            candidate.extend_from_slice(&current[end..]);
            if !candidate.is_empty() && fails(&candidate) {
                current = candidate;
                granularity = (granularity - 1).max(2);
                reduced = true;
                break;
            }
            start = end;
        }
        if !reduced {
            if granularity >= current.len() {
                break;
            }
            granularity = (granularity * 2).min(current.len());
        }
    }
    // final single-op pass for 1-minimality
    let mut i = 0;
    while i < current.len() && current.len() > 1 {
        let mut candidate = current.clone();
        candidate.remove(i);
        if fails(&candidate) {
            current = candidate;
        } else {
            i += 1;
        }
    }
    current
}
