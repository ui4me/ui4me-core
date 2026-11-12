// owner: 001-vertical-slice — T023 (core-types.md §2; 005 FR-015–FR-017)
use ui4me_engine_types::ids::{DocumentId, RawId, Slots};
use ui4me_engine_types::revision::*;

fn doc() -> DocumentId {
    DocumentId::from_raw(Slots::new().alloc())
}

#[test]
fn current_iff_inputs_equal_documents_current_inputs() {
    let d = doc();
    let inputs = RevisionInputs {
        dom: 3,
        environment: 1,
        resource: 0,
        font: 2,
        scroll: 0,
        animation: 0,
    };
    let r = RevisionTuple {
        document: d,
        inputs,
        style: 4,
        layout: 4,
        scene: 4,
    };
    assert!(r.is_current(d, &inputs));
    // output epochs never affect currency
    let r2 = RevisionTuple {
        style: 99,
        layout: 1,
        scene: 0,
        ..r
    };
    assert!(r2.is_current(d, &inputs));
    // every input field participates
    for bump in 0..6 {
        let mut cur = inputs;
        match bump {
            0 => cur.dom += 1,
            1 => cur.environment += 1,
            2 => cur.resource += 1,
            3 => cur.font += 1,
            4 => cur.scroll += 1,
            _ => cur.animation += 1,
        }
        assert!(
            !r.is_current(d, &cur),
            "input field {bump} must participate"
        );
    }
}

#[test]
fn other_document_is_never_current() {
    let mut s = Slots::new();
    let d1 = DocumentId::from_raw(s.alloc());
    let d2 = DocumentId::from_raw(s.alloc());
    let inputs = RevisionInputs::default();
    let r = RevisionTuple {
        document: d1,
        inputs,
        style: 0,
        layout: 0,
        scene: 0,
    };
    assert!(!r.is_current(d2, &inputs));
    let _ = RawId {
        index: 0,
        generation: std::num::NonZeroU32::MIN,
    };
}

// ---- 005 T009/T010: increment API per writer, outputs at publication, `current` ----------------

#[test]
fn each_writer_increments_only_its_own_field_and_monotonically() {
    let writers = [
        (InputWriter::DocumentCore, 0usize),
        (InputWriter::HostRuntime, 1),
        (InputWriter::ResourceManager, 2),
        (InputWriter::FontManager, 3),
        (InputWriter::InteractionRuntime, 4),
        (InputWriter::SchedulerClock, 5),
    ];
    let as_array = |i: &RevisionInputs| {
        [
            i.dom,
            i.environment,
            i.resource,
            i.font,
            i.scroll,
            i.animation,
        ]
    };
    for (w, idx) in writers {
        let mut inputs = RevisionInputs::default();
        let mut last = 0;
        for n in 1..=3u64 {
            let v = inputs.bump(w);
            assert_eq!(v, n);
            assert!(v > last, "monotonic");
            last = v;
            let a = as_array(&inputs);
            for (i, x) in a.iter().enumerate() {
                assert_eq!(
                    *x,
                    if i == idx { n } else { 0 },
                    "{w:?} touches only field {idx}"
                );
            }
            assert_eq!(inputs.get(w), n);
        }
    }
}

#[test]
fn publication_increments_only_changed_output_epochs() {
    let d = doc();
    let t0 = RevisionTuple {
        document: d,
        inputs: RevisionInputs::default(),
        style: 2,
        layout: 3,
        scene: 4,
    };
    let mut inputs = t0.inputs;
    inputs.bump(InputWriter::DocumentCore);
    let t1 = t0.published(
        inputs,
        ChangedOutputs {
            style: false,
            layout: true,
            scene: true,
        },
    );
    assert_eq!((t1.style, t1.layout, t1.scene), (2, 4, 5));
    assert_eq!(t1.inputs.dom, 1);
    assert_eq!(t1.document, d);
}

#[test]
fn current_is_evaluated_against_the_inputs_at_return_time() {
    let d = doc();
    let mut inputs = RevisionInputs::default();
    let t = RevisionTuple {
        document: d,
        inputs,
        style: 1,
        layout: 1,
        scene: 1,
    };
    assert!(t.stamped(&inputs).current);
    inputs.bump(InputWriter::SchedulerClock);
    let r = t.stamped(&inputs);
    assert!(
        !r.current,
        "an input moved after publication: the Committed read is stale"
    );
    assert_eq!(r.tuple, t);
}
