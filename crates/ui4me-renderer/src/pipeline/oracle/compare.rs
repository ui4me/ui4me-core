//! owner: 005-pipeline-diagnostics — T059. The stage-ordered comparison of the live committed set
//! with the cold scratch result (recompute-plan §8 step 4): computed values → box structure →
//! fragments (exact in LayoutUnit) → display list → hit index. The first difference is reported
//! with its stage, node key, field and both values; properties of a running transition or
//! animation are excluded and listed as `skipped: animating` (Clarification Q1). Moved from
//! 001's cold hook (R-28), not rewritten.

use crate::diagnostics::probes::cold_oracle::{ColdDifference, ColdOracleInput, ColdStage};
use crate::layout::coordinator::LayoutOutput;
use crate::pipeline::publish::CommittedSet;
use crate::scene::appendix_e::SceneSnapshot;
use crate::style::view::StyleSnapshot;
use std::sync::Arc;
use ui4me_document::document::Document;
use ui4me_document::node::NodeKind;
use ui4me_engine_types::ids::NodeId;

fn clip(s: String) -> String {
    const MAX: usize = 512;
    if s.len() <= MAX {
        s
    } else {
        let mut end = MAX;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// The first index where two sequences differ, with both values' debug forms.
fn first_diff<T: PartialEq + std::fmt::Debug>(
    live: &[T],
    cold: &[T],
) -> Option<(usize, String, String)> {
    let n = live.len().max(cold.len());
    (0..n).find_map(|i| match (live.get(i), cold.get(i)) {
        (Some(a), Some(b)) if a == b => None,
        (a, b) => Some((i, clip(format!("{a:?}")), clip(format!("{b:?}")))),
    })
}

/// Compare `live` with the cold result of the same document.
pub(crate) fn compare(
    doc: &Document,
    live: &Arc<CommittedSet>,
    cold_style: &Arc<StyleSnapshot>,
    cold_layout: &LayoutOutput,
    cold_scene: &SceneSnapshot,
    animating: &std::collections::HashSet<NodeId>,
) -> ColdOracleInput {
    let mut skipped: Vec<(NodeId, String)> = Vec::new();
    // 4. Compare in stage order; the first difference is reported.
    let mut stages = Vec::new();
    let mut first: Option<ColdDifference> = None;
    let mut note =
        |stage: ColdStage, diff: Option<ColdDifference>, first: &mut Option<ColdDifference>| {
            stages.push((stage, diff.is_none()));
            if first.is_none() {
                *first = diff;
            }
        };

    // Computed values: every connected element, tree order, every profile longhand. The live
    // values come from the committed snapshot, the cold ones from the scratch bridge.
    let elements: Vec<NodeId> = doc
        .arena()
        .subtree(doc.document_node())
        .into_iter()
        .filter(|n| doc.node_kind(*n) == Ok(NodeKind::Element))
        .collect();
    let longhands = crate::style::computed::profile_longhands();
    let mut cascade_diff = None;
    'cascade: for &node in &elements {
        let (lv, cv) = (live.style.view(node), cold_style.view(node));
        match (lv, cv) {
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {
                cascade_diff = Some(ColdDifference {
                    stage: ColdStage::Cascade,
                    node: Some(node),
                    field: "styled".into(),
                    live: live.style.view(node).is_some().to_string(),
                    cold: cold_style.view(node).is_some().to_string(),
                });
                break 'cascade;
            }
            (Some(lv), Some(cv)) => {
                if animating.contains(&node) || lv.flags().is_animating || cv.flags().is_animating {
                    // Skipped as `animating` (Clarification Q1): a cold restyle starts no transition.
                    skipped.push((node, "*".to_string()));
                    continue;
                }
                for p in &longhands {
                    let (a, b) = (lv.serialize(p), cv.serialize(p));
                    if a != b {
                        cascade_diff = Some(ColdDifference {
                            stage: ColdStage::Cascade,
                            node: Some(node),
                            field: (*p).to_string(),
                            live: a.unwrap_or_default(),
                            cold: b.unwrap_or_default(),
                        });
                        break 'cascade;
                    }
                }
            }
        }
    }
    note(ColdStage::Cascade, cascade_diff, &mut first);

    let box_live: Vec<String> = live
        .layout
        .boxes
        .boxes
        .iter()
        .map(|b| format!("{b:?}"))
        .collect();
    let box_cold: Vec<String> = cold_layout
        .boxes
        .boxes
        .iter()
        .map(|b| format!("{b:?}"))
        .collect();
    let box_diff = first_diff(&box_live, &box_cold).map(|(i, a, b)| ColdDifference {
        stage: ColdStage::Box,
        node: live.layout.boxes.boxes.get(i).map(|b| b.owner.node()),
        field: format!("boxes[{i}]"),
        live: a,
        cold: b,
    });
    // Layout and paint follow the animated values: with any animating element only the
    // computed values of the other elements are compared (FR-068).
    let downstream = animating.is_empty();
    let box_diff = box_diff.filter(|_| downstream);
    note(ColdStage::Box, box_diff, &mut first);

    let layout_diff = first_diff(
        &live.layout.fragments.fragments,
        &cold_layout.fragments.fragments,
    )
    .map(|(i, a, b)| ColdDifference {
        stage: ColdStage::Layout,
        node: None,
        field: format!("fragments[{i}]"),
        live: a,
        cold: b,
    });
    note(
        ColdStage::Layout,
        layout_diff.filter(|_| downstream),
        &mut first,
    );

    let scene_diff = first_diff(
        &live.scene.display_list.items,
        &cold_scene.display_list.items,
    )
    .map(|(i, a, b)| ColdDifference {
        stage: ColdStage::Scene,
        node: live.scene.display_list.items.get(i).map(|it| it.owner.node),
        field: format!("display_list.items[{i}]"),
        live: a,
        cold: b,
    });
    note(
        ColdStage::Scene,
        scene_diff.filter(|_| downstream),
        &mut first,
    );

    let (hl, hc) = (
        format!("{:?}", live.scene.hit_index),
        format!("{:?}", cold_scene.hit_index),
    );
    let hit_diff = (hl != hc).then(|| ColdDifference {
        stage: ColdStage::HitIndex,
        node: None,
        field: "hit_index".into(),
        live: clip(hl),
        cold: clip(hc),
    });
    note(
        ColdStage::HitIndex,
        hit_diff.filter(|_| downstream),
        &mut first,
    );

    let equal = stages.iter().all(|(_, e)| *e);
    ColdOracleInput {
        equal,
        stages,
        first_difference: first,
        skipped,
        seed: None,
        step: None,
    }
}
