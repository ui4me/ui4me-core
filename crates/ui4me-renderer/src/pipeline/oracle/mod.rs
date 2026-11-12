//! owner: 005-pipeline-diagnostics — T059–T061. The cold-recompute oracle (constitution VII;
//! recompute-plan §8): `verify` is the semantic entry of the ABI **testing** group (R-24) —
//! owner thread only, available in every build, opt-in, and never run by a normal flush. 004
//! owns the C signature and maps this function.
//!
//! - [`compare`]: the stage-ordered comparison (T059);
//! - [`driver`]: the seeded random-mutation driver (T060);
//! - [`shrink`]: delta-debugging of a failing sequence (T060).

pub mod compare;
pub mod driver;
pub mod shrink;

use crate::diagnostics::probes::cold_oracle::ColdOracleInput;
use crate::pipeline::state::DocumentPipeline;
use ui4me_engine_types::status::Status;

/// The oracle's report: per-stage equality, the first difference (stage, node key, field, live
/// value, cold value), the properties skipped because they were animating, and — when produced
/// by the random driver — the seed and step.
pub type OracleReport = ColdOracleInput;

/// Compare the live committed set of `doc` with a cold recompute. The live document is not
/// modified: a `Dirty` document is first brought to `Clean` by an `oracle-prep` flush (the only
/// live effect), then a scratch pipeline recomputes it from the same DOM and fonts.
pub fn verify(doc: &mut DocumentPipeline) -> Result<OracleReport, Status> {
    Ok(doc.cold_oracle(false)?.oracle)
}
