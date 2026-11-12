// owner: 005-pipeline-diagnostics — T011 (diagnostic stores; FR-046, FR-064, FR-065, FR-067,
// FR-068, FR-075; schema §5, §7; R-20, R-37; IF-05)
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::*;
use ui4me_engine_types::ids::{DocumentId, NodeId, Slots};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

fn doc() -> DocumentId {
    DocumentId::from_raw(Slots::new().alloc())
}

fn key(feature: &'static str, stage: Stage, d: DocumentId, line: u32) -> ReportKey {
    ReportKey {
        feature: FeatureId::new(feature).unwrap(),
        stage,
        document: d,
        location: ReportLocation::Stylesheet {
            sheet: 0,
            origin: SheetOrigin::Author,
            line,
            column: 1,
        },
    }
}

fn node(i: u32) -> NodeId {
    let mut s = Slots::new();
    let mut last = s.alloc();
    for _ in 0..i {
        last = s.alloc();
    }
    NodeId::from_raw(last)
}

// ---- unsupported store: the single R-20 key -----------------------------------------------------

#[test]
fn the_report_key_is_feature_stage_document_location() {
    let d = doc();
    let mut r = UnsupportedReports::default();
    let base = key("css.display.flex", Stage::Box, d, 1);
    r.record(
        base,
        Code::UnsupportedFeature,
        UnsupportedReason::NotConsumedByStage,
        1,
    );
    // each key part distinguishes entries
    r.record(
        key("css.display.grid", Stage::Box, d, 1),
        Code::UnsupportedFeature,
        UnsupportedReason::NotConsumedByStage,
        1,
    );
    r.record(
        key("css.display.flex", Stage::Cascade, d, 1),
        Code::UnsupportedFeature,
        UnsupportedReason::NotConsumedByStage,
        1,
    );
    r.record(
        key("css.display.flex", Stage::Box, d, 2),
        Code::UnsupportedFeature,
        UnsupportedReason::NotConsumedByStage,
        1,
    );
    assert_eq!(r.len(), 4);
}

#[test]
fn repeats_deduplicate_per_dom_revision_with_first_and_last_seen() {
    let d = doc();
    let mut r = UnsupportedReports::default();
    let k = key("css.display.flex", Stage::Box, d, 1);
    for rev in [5, 5, 5, 7, 7, 9] {
        r.record(
            k,
            Code::UnsupportedFeature,
            UnsupportedReason::GatedOff,
            rev,
        );
    }
    let e = r.iter().next().unwrap();
    assert_eq!((e.revisions, e.first_seen_dom, e.last_seen_dom), (3, 5, 9));
    assert_eq!(r.overflow_count(), 0);
}

#[test]
fn the_store_holds_4096_distinct_entries_then_counts_overflow() {
    let d = doc();
    let mut r = UnsupportedReports::default();
    assert_eq!(UnsupportedReports::CAPACITY, 4096);
    for i in 0..4096u32 {
        r.record(
            key("css.display.flex", Stage::Box, d, i + 1),
            Code::UnsupportedFeature,
            UnsupportedReason::GatedOff,
            1,
        );
    }
    assert_eq!((r.len(), r.overflow_count()), (4096, 0));
    // a repeat of a stored key is not an overflow
    r.record(
        key("css.display.flex", Stage::Box, d, 1),
        Code::UnsupportedFeature,
        UnsupportedReason::GatedOff,
        2,
    );
    assert_eq!(r.overflow_count(), 0);
    // three further distinct keys overflow; entries stay put
    for i in 0..3u32 {
        r.record(
            key("css.display.grid", Stage::Box, d, i + 1),
            Code::UnsupportedFeature,
            UnsupportedReason::GatedOff,
            2,
        );
    }
    assert_eq!((r.len(), r.overflow_count()), (4096, 3));
}

#[test]
fn iteration_order_is_insertion_order() {
    let d = doc();
    let mut r = UnsupportedReports::default();
    for line in [9, 3, 6] {
        r.record(
            key("css.display.flex", Stage::Box, d, line),
            Code::UnsupportedFeature,
            UnsupportedReason::GatedOff,
            1,
        );
    }
    let lines: Vec<u32> = r
        .iter()
        .map(|e| match e.key.location {
            ReportLocation::Stylesheet { line, .. } => line,
            _ => 0,
        })
        .collect();
    assert_eq!(lines, vec![9, 3, 6]);
}

// ---- feature-id root registry ------------------------------------------------------------------

#[test]
fn the_root_registry_is_closed_and_rejects_unknown_roots() {
    // schema §7 lists these roots (the task text's "14" is superseded by the table: ERRATA E-005-01)
    for root in [
        "css.property",
        "css.at-rule",
        "css.selector",
        "css.pseudo",
        "css.value",
        "css.display",
        "css.media",
        "css.unit",
        "css.function",
        "css.syntax",
        "html",
        "img",
        "svg",
        "paint.capability",
        "aria.role",
        "aria.attr",
        "platform",
    ] {
        assert!(FEATURE_ROOTS.contains(&root), "{root}");
    }
    assert_eq!(FEATURE_ROOTS.len(), 17);
    assert!(FeatureId::new("img.animation").is_some());
    assert!(FeatureId::new("platform.window-position").is_some());
    assert!(FeatureId::new("bogus.id").is_none());
    assert!(FeatureId::new("css.display").is_none());
}

// ---- bounded rings -------------------------------------------------------------------------------

#[test]
fn documented_capacities() {
    assert_eq!(EVENT_LOG_CAPACITY, 1024);
    assert_eq!(ERROR_RING_CAPACITY, 256);
    assert_eq!(RECEIPTS_PER_TARGET, 32);
    assert_eq!(INVALIDATION_HISTORY, 16);
    assert_eq!(REQUEST_LOG_CAPACITY, 1024);
}

fn record(n: u32) -> InvalidationRecord {
    InvalidationRecord {
        document: doc(),
        reason: "DOM_TEXT",
        dom_revision: u64::from(n),
        stages: vec![Stage::Layout],
    }
}

#[test]
fn invalidation_history_keeps_the_last_16_updates() {
    let mut s = DiagStores::new();
    for n in 0..20 {
        s.push_invalidation(record(n));
    }
    let kept: Vec<u64> = s.invalidations.iter().map(|r| r.dom_revision).collect();
    assert_eq!(kept, (4..20).collect::<Vec<_>>());
    assert_eq!(s.invalidations.dropped(), 4);
}

#[test]
fn error_ring_keeps_256_and_counts_drops() {
    let mut s = DiagStores::new();
    for _ in 0..300 {
        s.push_error(ErrorRecord {
            document: None,
            status: Status::new(Code::InternalInvariant, Stage::Host),
        });
    }
    assert_eq!((s.errors.len(), s.errors.dropped()), (256, 44));
}

#[test]
fn receipts_keep_32_per_target_in_order() {
    let mut s = DiagStores::new();
    let d = doc();
    let rec = |seq: u64| ReceiptRecord {
        document: d,
        frame_seq: seq,
        scene_dom_revision: 1,
        status: Code::Ok,
        input_to_present_ms: None,
    };
    for seq in 1..=40 {
        s.push_receipt(ReceiptTargetKey::Window(1), rec(seq));
    }
    for seq in 1..=5 {
        s.push_receipt(ReceiptTargetKey::Headless, rec(seq));
    }
    let w: Vec<u64> = s
        .receipts
        .iter(ReceiptTargetKey::Window(1))
        .map(|r| r.frame_seq)
        .collect();
    assert_eq!(w, (9..=40).collect::<Vec<_>>());
    assert_eq!(s.receipts.dropped(ReceiptTargetKey::Window(1)), 8);
    assert_eq!(s.receipts.iter(ReceiptTargetKey::Headless).count(), 5);
    assert_eq!(s.receipts.iter(ReceiptTargetKey::Window(2)).count(), 0);
}

fn event(n: u64) -> DispatchRecord {
    let d = doc();
    DispatchRecord {
        document: d,
        dispatch_id: ui4me_engine_types::ids::DispatchIdSource::default().next(),
        parent_dispatch_id: None,
        depth: 1,
        event_type: format!("e{n}"),
        bubbles: true,
        cancelable: true,
        is_trusted: true,
        origin: DispatchOrigin::Native,
        target: node(1),
        path: vec![],
        activation_target: None,
        pre_activation: None,
        invocations: vec![],
        canceled: false,
        event_data: None,
        status: Code::Ok,
        nondeterministic: Nondeterministic::default(),
    }
}

#[test]
fn event_log_keeps_1024_oldest_dropped_with_a_count() {
    let mut s = DiagStores::new();
    for n in 0..1030 {
        s.push_event(event(n));
    }
    assert_eq!((s.events.len(), s.events.dropped()), (1024, 6));
    assert_eq!(s.events.iter().next().unwrap().event_type, "e6");
}

// ---- request log (IF-05) -------------------------------------------------------------------------

#[test]
fn request_log_evicts_the_oldest_completed_and_never_a_pending_one() {
    let mut log = RequestLog::with_capacity(3);
    let a = log.push_pending("a");
    let b = log.push_pending("b");
    let c = log.push_pending("c");
    log.complete(b, "ok");
    // full: pushing evicts the oldest *completed* (b), not the older pending a
    log.push_pending("d");
    let names: Vec<&str> = log.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(names, vec!["a", "c", "d"]);
    assert_eq!(log.dropped_count(), 1);
    assert!(log.get(a).is_some() && log.get(c).is_some() && log.get(b).is_none());
}

#[test]
fn request_log_with_only_pending_entries_grows_past_the_bound_without_dropping_them() {
    let mut log = RequestLog::with_capacity(2);
    for l in ["p1", "p2", "p3", "p4"] {
        log.push_pending(l);
    }
    assert_eq!(log.len(), 4, "pending requests are never evicted");
    assert_eq!(log.dropped_count(), 0);
    // completing one makes it evictable by the next push
    log.complete(RequestId(0), "ok");
    log.push_pending("p5");
    assert_eq!(log.dropped_count(), 1);
    assert!(log.iter().all(|r| r.label != "p1"));
}

#[test]
fn request_log_default_is_1024_and_push_request_is_the_store_api() {
    let mut s = DiagStores::new();
    for n in 0..1100 {
        let id = s.push_request(format!("r{n}"));
        s.requests.complete(id, "ok");
    }
    assert_eq!(s.requests.len(), 1024);
    assert_eq!(s.requests.dropped_count(), 76);
}
