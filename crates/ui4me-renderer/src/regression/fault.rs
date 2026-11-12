// owner: 005-pipeline-diagnostics — T022 (research R-16; U4T-184, U4T-192): `cargo test -p
// ui4me-pipeline --features fault-injection fault::`
#![cfg(feature = "fault-injection")]

mod fault {
    use crate::pipeline::fault::FaultPoint;
    use crate::pipeline::state::{DocumentPipeline, PipelineConfig, PipelineState};
    use std::path::Path;
    use ui4me_engine_types::codes_generated::Code;
    use ui4me_engine_types::stage::Stage;

    const HTML: &str = r#"<!DOCTYPE html><html><head><style>body{margin:8px;font-family:"Noto Sans";font-size:16px}
[data-state="open"]{color:#cc3333}</style></head><body><p id="p" data-state="closed">hello</p></body></html>"#;

    fn load() -> DocumentPipeline {
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
        let mut p = DocumentPipeline::load_html(
            HTML,
            PipelineConfig {
                viewport_width: 320.0,
                viewport_height: 200.0,
                scale: 1.0,
                font_files: vec![font],
            },
        )
        .expect("loads");
        p.flush().expect("first flush");
        p
    }

    fn dirty(p: &mut DocumentPipeline, text: &str) {
        let id = p.element_by_id("p").unwrap();
        p.set_text_content(id, text).unwrap();
    }

    #[test]
    fn each_point_fires_exactly_once_on_the_configured_call() {
        let expected = [
            (FaultPoint::Style, Stage::Cascade),
            (FaultPoint::Box, Stage::Box),
            (FaultPoint::Layout, Stage::Layout),
            (FaultPoint::Scene, Stage::Scene),
            (FaultPoint::Publish, Stage::Scene),
        ];
        for (point, stage) in expected {
            for code in [Code::OutOfMemory, Code::InternalInvariant] {
                let mut p = load();
                // fire on the 2nd update after arming: the 1st passes, the 2nd fails, the 3rd passes again
                p.arm_fault(point, 2, code);
                dirty(&mut p, "one");
                assert!(p.flush().is_ok(), "{point:?}: call 1 passes");
                dirty(&mut p, "two");
                let e = p.flush().expect_err("call 2 fails");
                assert_eq!((e.code, e.stage), (code, stage), "{point:?}");
                assert_eq!(p.state(), PipelineState::FailedUpdate);
                dirty(&mut p, "three");
                assert!(
                    p.flush().is_ok(),
                    "{point:?}: exactly once — call 3 passes (cold retry)"
                );
                assert_eq!(p.state(), PipelineState::Clean);
                assert_eq!(
                    p.fault_calls(point),
                    2,
                    "{point:?}: the fired fault stops counting"
                );
            }
        }
    }

    #[test]
    fn the_omission_leaves_attribute_selector_dependants_stale() {
        let mut p = load();
        p.set_omit_attribute_selector_restyle(true);
        let id = p.element_by_id("p").unwrap();
        p.set_attribute(id, "data-state", "open").unwrap();
        let live = p.flush().unwrap();
        // A cold recompute sees the rule; the incremental result lacks it.
        let mut cold = load();
        let cid = cold.element_by_id("p").unwrap();
        cold.set_attribute(cid, "data-state", "open").unwrap();
        let reference = {
            let r = cold.cold_oracle(false).unwrap();
            r.clone()
        };
        let live_ok = p.cold_oracle(false).unwrap();
        // The cold oracle on the faulty pipeline must report a mismatch (T052 asserts the stage).
        assert!(!live_ok.equal(), "the oracle reports the omission");
        assert!(
            reference.equal(),
            "without the omission live and cold agree"
        );
        let _ = live;
    }
}
