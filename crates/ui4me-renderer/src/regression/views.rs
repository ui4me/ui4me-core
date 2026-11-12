// owner: 005-pipeline-diagnostics — T049 (recompute-plan §9; R-34; FR-018)
use crate::pipeline::state::{DocumentPipeline, PipelineConfig};
use crate::pipeline::views::ViewId;
use std::path::Path;
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;

const HTML: &str = r#"<!DOCTYPE html><html><head><style>body{margin:8px;font-family:"Noto Sans";font-size:16px}</style></head><body><p id="p">hello</p></body></html>"#;

fn load() -> DocumentPipeline {
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf");
    DocumentPipeline::load_html(
        HTML,
        PipelineConfig {
            viewport_width: 320.0,
            viewport_height: 200.0,
            scale: 1.0,
            font_files: vec![font],
        },
    )
    .expect("loads")
}

fn edit(p: &mut DocumentPipeline, text: &str) {
    let id = p.element_by_id("p").unwrap();
    p.set_text_content(id, text).unwrap();
}

#[test]
fn a_view_before_the_first_publication_is_snapshot_not_ready() {
    let mut p = load();
    assert_eq!(p.pin_view().unwrap_err().code, Code::SnapshotNotReady);
}

#[test]
fn a_view_pins_one_committed_set_across_later_publications() {
    let mut p = load();
    p.flush().unwrap();
    let v = p.pin_view().unwrap();
    let pinned = p.read_view(v).unwrap();
    edit(&mut p, "changed");
    let newer = p.flush().unwrap();
    assert_ne!(pinned.revision, newer.revision);
    let again = p.read_view(v).unwrap();
    assert!(
        Arc::ptr_eq(&again.committed, &pinned.committed),
        "the view keeps its set"
    );
    assert_eq!(again.revision, pinned.revision);
    assert!(
        !again.current,
        "the pinned set is older than the document's inputs"
    );
    assert!(newer.current);
}

#[test]
fn release_is_idempotent_and_a_read_after_release_is_snapshot_retired() {
    let mut p = load();
    p.flush().unwrap();
    let v = p.pin_view().unwrap();
    assert!(p.release_view(v).is_ok());
    assert!(p.release_view(v).is_ok(), "release twice is still OK");
    assert_eq!(p.read_view(v).unwrap_err().code, Code::SnapshotRetired);
    assert_eq!(p.views().live(), 0);
}

#[test]
fn a_handle_this_document_never_issued_is_stale() {
    let mut p = load();
    p.flush().unwrap();
    assert_eq!(p.read_view(ViewId(99)).unwrap_err().code, Code::StaleHandle);
    assert_eq!(
        p.release_view(ViewId(99)).unwrap_err().code,
        Code::StaleHandle
    );
}

#[test]
fn from_closing_on_reads_are_document_closed_while_release_is_ok() {
    let mut p = load();
    p.flush().unwrap();
    let kept = p.pin_view().unwrap();
    let released = p.pin_view().unwrap();
    p.release_view(released).unwrap();
    p.request_close(true); // Closing
    assert_eq!(p.read_view(kept).unwrap_err().code, Code::DocumentClosed);
    assert_eq!(
        p.read_view(released).unwrap_err().code,
        Code::DocumentClosed,
        "O1 precedes O6"
    );
    assert!(p.release_view(kept).is_ok(), "release stays OK in Closing");
    assert!(p.release_view(kept).is_ok());
    assert_eq!(p.pin_view().unwrap_err().code, Code::DocumentClosed);
    p.outermost_commit(); // Closed
    assert!(p.release_view(kept).is_ok(), "release stays OK in Closed");
    assert_eq!(p.read_view(kept).unwrap_err().code, Code::DocumentClosed);
}

#[test]
fn a_set_is_freed_once_no_view_and_no_consumer_holds_it() {
    let mut p = load();
    p.flush().unwrap();
    let v = p.pin_view().unwrap();
    let weak = Arc::downgrade(&p.read_view(v).unwrap().committed);
    edit(&mut p, "newer");
    p.flush().unwrap(); // the pipeline now holds the newer set
    assert!(weak.upgrade().is_some(), "the view still holds the old set");
    p.release_view(v).unwrap();
    assert!(
        weak.upgrade().is_none(),
        "released and superseded: the set is freed"
    );
}

#[test]
fn repeated_release_retires_ids_without_storing_tombstones() {
    let mut p = load();
    p.flush().unwrap();
    let first = p.pin_view().unwrap();
    p.release_view(first).unwrap();
    for _ in 0..10_000 {
        let view = p.pin_view().unwrap();
        p.release_view(view).unwrap();
    }
    assert_eq!(p.views().retained_entries(), 0);
    assert_eq!(p.read_view(first).unwrap_err().code, Code::SnapshotRetired);
    assert!(p.release_view(first).is_ok());
}
