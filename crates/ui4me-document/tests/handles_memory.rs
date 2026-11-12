//! owner: 003-document-model
//!
//! T055 / SC-006: 10,000 create-detach-drop cycles. The detached count is zero at the next
//! checkpoint and the arena's slot usage does not grow after warm-up (less than 1 %).

use ui4me_document::document::Document;

// table-a: A-12
#[test]
fn ten_thousand_create_detach_drop_cycles_leave_no_detached_nodes_and_no_growth() {
    let mut d = Document::parse_html("<body><div id=host></div></body>").unwrap();
    let host = d.element_by_id("host").unwrap();
    let mut warm = None;
    for i in 0..10_000 {
        let el = d.create_element("section").unwrap();
        let txt = d.create_text_node("payload").unwrap();
        d.append_child(el, txt).unwrap();
        d.set_attribute(el, "data-i", &i.to_string()).unwrap();
        d.retain(el).unwrap();
        d.append_child(host, el).unwrap();
        d.remove_child(host, el).unwrap();
        d.release(el).unwrap();
        d.checkpoint();
        let c = d.counts();
        assert_eq!(
            (c.detached, c.detached_trees, c.host_refs_total),
            (0, 0, 0),
            "cycle {i}"
        );
        if i == 100 {
            warm = Some(d.arena().slot_count());
        }
    }
    let warm = warm.unwrap();
    let end = d.arena().slot_count();
    assert!(
        end as f64 <= warm as f64 * 1.01,
        "slot usage grew from {warm} to {end}"
    );
    assert_eq!(d.arena().retired_slots(), 0);
    assert_eq!(d.counts().freed_total, 20_000);
}
