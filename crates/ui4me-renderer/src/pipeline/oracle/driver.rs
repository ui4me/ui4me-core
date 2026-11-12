//! owner: 005-pipeline-diagnostics — T060. The seeded random-mutation driver (FR-034; recompute-
//! plan §8): a seeded PRNG chooses operations from the mutation set, applies them through the
//! public DOM API on the owner thread, and runs the cold oracle after every step. Operations are
//! plain data that address nodes by *index into the current element list*, so a sequence
//! replays identically from a seed and any sub-sequence stays applicable, which is what the
//! shrinker (shrink.rs) needs.

use crate::diagnostics::probes::cold_oracle::ColdOracleInput;
use crate::pipeline::state::DocumentPipeline;
use ui4me_document::node::NodeKind;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// The classes, attributes, inline declarations and tags the driver draws from. They match the
/// selectors of `verification/fixtures/pipeline-oracle-corpus/*`, so the mutations matter.
pub const CLASSES: &[&str] = &["on", "alt", "wide", "active", "item", "box", "c1", "c2"];
pub const ATTRS: &[(&str, &str)] = &[
    ("data-state", "open"),
    ("data-state", "closed"),
    ("data-state", "warn idle"),
    ("hidden", ""),
    ("title", "t"),
    ("lang", "en"),
    ("dir", "rtl"),
];
pub const STYLES: &[&str] = &[
    "width: 120px",
    "width: 33%",
    "height: 30px",
    "color: #cc3333",
    "background-color: #ddeeff",
    "margin-left: 7px",
    "padding: 3px",
    "font-size: 22px",
    "display: none",
    "display: block",
    "display: inline",
    "visibility: hidden",
    "opacity: 0.5",
    "border: 2px solid #336699",
    "text-align: center",
    "line-height: 30px",
    "",
];
pub const TEXTS: &[&str] = &[
    "",
    "x",
    "alpha beta",
    "a much longer run of words that must wrap inside the block",
    "line\nbreak",
    "\u{e9}\u{e8}",
];
pub const TAGS: &[&str] = &["div", "span", "p", "li", "ul", "b"];

/// One mutation. Indices are taken modulo the number of candidate elements at apply time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    ToggleClass {
        target: usize,
        class: usize,
    },
    SetAttribute {
        target: usize,
        attr: usize,
    },
    RemoveAttribute {
        target: usize,
        attr: usize,
    },
    SetStyle {
        target: usize,
        decl: usize,
    },
    SetText {
        target: usize,
        text: usize,
    },
    InsertChild {
        parent: usize,
        tag: usize,
        text: usize,
    },
    RemoveNode {
        target: usize,
    },
    MoveNode {
        target: usize,
        new_parent: usize,
    },
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::ToggleClass { .. } => "toggle-class",
            Op::SetAttribute { .. } => "set-attribute",
            Op::RemoveAttribute { .. } => "remove-attribute",
            Op::SetStyle { .. } => "set-style",
            Op::SetText { .. } => "set-text",
            Op::InsertChild { .. } => "insert-child",
            Op::RemoveNode { .. } => "remove-node",
            Op::MoveNode { .. } => "move-node",
        }
    }
}

/// xorshift64*: small, seedable and platform-independent.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// `steps` operations drawn from `seed`.
pub fn generate(seed: u64, steps: usize) -> Vec<Op> {
    let mut r = Rng::new(seed);
    (0..steps).map(|_| next_op(&mut r)).collect()
}

fn next_op(r: &mut Rng) -> Op {
    let big = usize::MAX >> 1;
    match r.below(16) {
        0..=2 => Op::ToggleClass {
            target: r.below(big),
            class: r.below(CLASSES.len()),
        },
        3..=4 => Op::SetAttribute {
            target: r.below(big),
            attr: r.below(ATTRS.len()),
        },
        5 => Op::RemoveAttribute {
            target: r.below(big),
            attr: r.below(ATTRS.len()),
        },
        6..=8 => Op::SetStyle {
            target: r.below(big),
            decl: r.below(STYLES.len()),
        },
        9..=10 => Op::SetText {
            target: r.below(big),
            text: r.below(TEXTS.len()),
        },
        11..=12 => Op::InsertChild {
            parent: r.below(big),
            tag: r.below(TAGS.len()),
            text: r.below(TEXTS.len()),
        },
        13 => Op::RemoveNode {
            target: r.below(big),
        },
        _ => Op::MoveNode {
            target: r.below(big),
            new_parent: r.below(big),
        },
    }
}

/// The elements under `body` (candidates for mutation), in tree order. `body` itself is the
/// first candidate so the document never becomes empty of targets.
pub fn candidates(p: &DocumentPipeline) -> Vec<NodeId> {
    let doc = p.document();
    let Some(body) = doc.body() else {
        return Vec::new();
    };
    doc.arena()
        .subtree(body)
        .into_iter()
        .filter(|n| doc.node_kind(*n) == Ok(NodeKind::Element))
        .collect()
}

/// Apply `op` through the public DOM API. `Ok(false)` = not applicable here (skipped, no
/// mutation); `Err` is a rejected call (e.g. a cycle), which changes nothing (FR-009).
pub fn apply(p: &mut DocumentPipeline, op: &Op) -> Result<bool, Status> {
    let els = candidates(p);
    if els.is_empty() {
        return Ok(false);
    }
    let pick = |i: usize| els[i % els.len()];
    match *op {
        Op::ToggleClass { target, class } => {
            p.toggle_class(pick(target), CLASSES[class % CLASSES.len()])?;
        }
        Op::SetAttribute { target, attr } => {
            let (n, v) = ATTRS[attr % ATTRS.len()];
            p.set_attribute(pick(target), n, v)?;
        }
        Op::RemoveAttribute { target, attr } => {
            let (n, _) = ATTRS[attr % ATTRS.len()];
            let t = pick(target);
            p.mutate(|d| d.remove_attribute(t, n))?;
        }
        Op::SetStyle { target, decl } => {
            p.set_attribute(pick(target), "style", STYLES[decl % STYLES.len()])?;
        }
        Op::SetText { target, text } => {
            let t = pick(target);
            // Only elements whose children are all text are edited, so structure is preserved.
            let leafish = p
                .document()
                .children(t)
                .map(|c| {
                    c.iter()
                        .all(|k| p.document().node_kind(*k) == Ok(NodeKind::Text))
                })
                .unwrap_or(false);
            if !leafish {
                return Ok(false);
            }
            p.set_text_content(t, TEXTS[text % TEXTS.len()])?;
        }
        Op::InsertChild { parent, tag, text } => {
            let par = pick(parent);
            let child = p.create_element(TAGS[tag % TAGS.len()])?;
            p.set_text_content(child, TEXTS[text % TEXTS.len()])?;
            p.append_child(par, child)?;
        }
        Op::RemoveNode { target } => {
            let t = pick(target);
            if Some(t) == p.document().body() {
                return Ok(false);
            }
            p.mutate(|d| d.remove(t))?;
        }
        Op::MoveNode { target, new_parent } => {
            let (t, np) = (pick(target), pick(new_parent));
            if Some(t) == p.document().body()
                || t == np
                || p.document().arena().subtree(t).contains(&np)
            {
                return Ok(false);
            }
            p.append_child(np, t)?;
        }
    }
    Ok(true)
}

/// A mismatch found by [`run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub seed: u64,
    /// 0-based index of the operation after which the oracle first disagreed.
    pub step: usize,
    pub report: Box<ColdOracleInput>,
}

/// Run `ops` against a fresh pipeline from `make`, verifying against the cold recompute after
/// every applied step (and once at the start). `Err(Failure)` at the first mismatch.
pub fn run(
    make: &dyn Fn() -> DocumentPipeline,
    seed: u64,
    ops: &[Op],
) -> Result<RunStats, Failure> {
    let mut p = make();
    let mut stats = RunStats::default();
    let check = |p: &mut DocumentPipeline, step: usize| -> Result<(), Failure> {
        // The live document updates incrementally through an ordinary Flush; only then does the
        // oracle compare it with the cold recompute (an `oracle-prep` trigger would itself run
        // cold and hide an incremental defect).
        let flushed = p.flush().map(|_| ());
        let report = match flushed.and_then(|()| p.cold_oracle(false)) {
            Ok(r) => r.oracle,
            Err(status) => ColdOracleInput {
                equal: false,
                stages: Vec::new(),
                first_difference: Some(crate::diagnostics::probes::cold_oracle::ColdDifference {
                    stage: crate::diagnostics::probes::cold_oracle::ColdStage::Cascade,
                    node: None,
                    field: "update-failed".into(),
                    live: status.code.name().to_string(),
                    cold: String::new(),
                }),
                skipped: Vec::new(),
                seed: None,
                step: None,
            },
        };
        if report.equal {
            Ok(())
        } else {
            Err(Failure {
                seed,
                step,
                report: Box::new(ColdOracleInput {
                    seed: Some(seed),
                    step: Some(step as u64),
                    ..report
                }),
            })
        }
    };
    check(&mut p, 0)?;
    for (i, op) in ops.iter().enumerate() {
        match apply(&mut p, op) {
            Ok(true) => stats.applied += 1,
            Ok(false) => {
                stats.skipped += 1;
                continue;
            }
            Err(_) => {
                stats.rejected += 1;
                continue;
            }
        }
        check(&mut p, i)?;
    }
    Ok(stats)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    pub applied: u64,
    pub skipped: u64,
    pub rejected: u64,
}

/// Persist a failing sequence under `dir` (normally `verification/oracle-seeds/`) as
/// `<fixture>-<seed>.json`: the fixture, the seed and step, the first difference, and the
/// minimal operation list (as readable `Debug` forms) so the failure replays as a regression.
/// Returns the file written.
pub fn persist_failure(
    dir: &std::path::Path,
    fixture: &str,
    failure: &Failure,
    minimal: &[Op],
) -> std::io::Result<std::path::PathBuf> {
    use crate::diagnostics::canonical_json::{serde_json::json, to_canonical_bytes};
    let diff = failure.report.first_difference.as_ref().map(
        |d| json!({"stage": d.stage.name(), "field": d.field, "live": d.live, "cold": d.cold}),
    );
    let record = json!({
        "fixture": fixture,
        "seed": failure.seed,
        "step": failure.step,
        "first_difference": diff,
        "minimal_ops": minimal.iter().map(|o| format!("{o:?}")).collect::<Vec<_>>(),
    });
    std::fs::create_dir_all(dir)?;
    let name = format!("{}-{}.json", fixture.replace('/', "_"), failure.seed);
    let path = dir.join(name);
    std::fs::write(&path, to_canonical_bytes(&record))?;
    Ok(path)
}
