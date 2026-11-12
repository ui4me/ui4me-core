//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `dom.counts` probe input (schema §5; 003 counters). Serialised by 001 T052.

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DomCountsInput {
    pub connected: u64,
    pub detached: u64,
    pub detached_trees: u64,
    pub freed_total: u64,
    pub host_refs_total: u64,
}

pub(crate) fn to_json(c: &DomCountsInput) -> Value {
    json!({"connected": c.connected, "detached": c.detached, "detached_trees": c.detached_trees,
           "freed_total": c.freed_total, "host_refs_total": c.host_refs_total})
}
