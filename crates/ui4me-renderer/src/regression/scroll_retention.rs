//! Scroll position across overflow/display changes and actual DOM removal.
use crate::layout::coordinator::overflow::ScrollTarget;
use crate::pipeline::state::{DocumentPipeline, PipelineConfig};

#[test]
fn connected_dormant_offsets_restore_but_removed_subtrees_do_not() {
    let html = r#"<!doctype html><style>body{margin:0} #s{width:50px;height:50px;overflow:auto} #t{width:300px;height:300px}</style>
        <div id=a><div id=s><div id=t></div></div></div><div id=b></div>"#;
    let mut p = DocumentPipeline::load_html(
        html,
        PipelineConfig {
            viewport_width: 400.0,
            viewport_height: 300.0,
            scale: 1.0,
            font_files: vec![],
        },
    )
    .unwrap();
    let a = p.element_by_id("a").unwrap();
    let b = p.element_by_id("b").unwrap();
    let s = p.element_by_id("s").unwrap();
    let t = p.element_by_id("t").unwrap();
    let target = ScrollTarget::Node(s);
    p.flush().unwrap();
    p.scroll_to(target, 25.0, 40.0).unwrap();
    for (hide, show) in [
        ("overflow:visible", "overflow:auto"),
        ("overflow:clip", "overflow:auto"),
        ("display:none", "display:block"),
        ("display:contents", "display:block"),
    ] {
        p.set_attribute(s, "style", hide).unwrap();
        p.flush().unwrap();
        assert!(p.scroll_entry(target).is_none(), "{hide}");
        assert_eq!(
            p.scroll_offsets().scrolled().count(),
            0,
            "dormant offsets are not active"
        );
        assert_eq!(p.scroll_offsets().tracked_nodes().count(), 1);
        p.set_attribute(s, "style", show).unwrap();
        p.flush().unwrap();
        assert_eq!(
            p.scroll_entry(target).unwrap().offset,
            (25.0, 40.0),
            "{hide}"
        );
    }
    // A hidden ancestor removes the descendant's layout box while it stays connected.
    p.set_attribute(a, "style", "display:none").unwrap();
    p.flush().unwrap();
    assert!(p.scroll_entry(target).is_none());
    p.set_attribute(a, "style", "").unwrap();
    p.flush().unwrap();
    assert_eq!(p.scroll_entry(target).unwrap().offset, (25.0, 40.0));
    let oracle = p.cold_oracle(true).unwrap();
    assert!(
        oracle.equal(),
        "restored scroll matches cold layout and raster: {:?}",
        oracle.oracle
    );
    // Restoration clamps to the new range.
    p.set_attribute(s, "style", "overflow:visible").unwrap();
    p.flush().unwrap();
    p.set_attribute(t, "style", "height:70px").unwrap();
    p.set_attribute(s, "style", "overflow:auto").unwrap();
    p.flush().unwrap();
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 20.0);
    // Reparenting an ancestor while its descendant stays inside resets the
    // descendant's position, even though it is connected at the end of the call.
    p.mutate(|d| d.append_child(b, a)).unwrap();
    assert_eq!(p.scroll_offsets().tracked_nodes().count(), 0);
    p.flush().unwrap();
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 0.0);
    p.scroll_to(target, 0.0, 20.0).unwrap();
    // A rejected DOM operation changes neither connectedness nor the stored offset.
    assert!(p.mutate(|d| d.append_child(s, s)).is_err());
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 20.0);
    // A batch which removes the scroller and then fails must roll back its scroll state too.
    use ui4me_document::batch::decode::{Batch, Cmd, Operand};
    use ui4me_document::batch::exec::{document_word, node_word};
    let handle = Operand::Existing(document_word(p.document().id()), node_word(s));
    let batch = Batch {
        local_count: 0,
        cmds: vec![
            Cmd::Remove { node: handle },
            Cmd::AppendChild {
                parent: handle,
                child: handle,
            },
        ],
    };
    assert!(p.mutate(|d| d.apply_batch(&batch)).is_err());
    assert!(p.document().is_connected(s).unwrap());
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 20.0);
    // Removing and collecting an unrelated subtree must not clear this offset.
    p.mutate(|d| {
        let unrelated = d.create_element("div")?;
        d.append_child(b, unrelated)?;
        d.remove(unrelated)?;
        d.collect();
        Ok(())
    })
    .unwrap();
    assert_eq!(p.scroll_offsets().tracked_nodes().count(), 1);
    p.flush().unwrap();
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 20.0);
    // Detach an ancestor and reparent the scroller inside one mutation call,
    // before any flush. The old offset must not follow the moved node.
    p.mutate(|d| {
        d.remove(a)?;
        d.append_child(b, s)?;
        d.collect();
        Ok(())
    })
    .unwrap();
    assert_eq!(p.scroll_offsets().tracked_nodes().count(), 0);
    p.flush().unwrap();
    assert_eq!(p.scroll_entry(target).unwrap().offset.1, 0.0);
    // The bounded diagnostic window may truncate a large batch, but the
    // current epoch segment retains enough information to distinguish a long
    // attribute-only batch from one that really removed the scroller.
    p.scroll_to(target, 0.0, 10.0).unwrap();
    p.mutate(|d| {
        for i in 0..4200 {
            d.set_attribute(b, "data-n", &i.to_string())?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(p.scroll_offsets().tracked_nodes().count(), 1);
    p.mutate(|d| {
        d.remove(s)?;
        for i in 4200..8400 {
            d.set_attribute(b, "data-n", &i.to_string())?;
        }
        d.append_child(b, s)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(p.scroll_offsets().tracked_nodes().count(), 0);
    p.flush().unwrap();
    // Repeated attached → dormant → detached trees do not retain offset entries.
    for _ in 0..100 {
        p.set_attribute(s, "style", "overflow:auto").unwrap();
        p.flush().unwrap();
        p.scroll_to(target, 0.0, 10.0).unwrap();
        p.set_attribute(s, "style", "display:none").unwrap();
        p.flush().unwrap();
        assert_eq!(p.scroll_offsets().tracked_nodes().count(), 1);
        p.mutate(|d| d.remove(s)).unwrap();
        assert_eq!(p.scroll_offsets().tracked_nodes().count(), 0);
        p.mutate(|d| d.append_child(b, s)).unwrap();
        p.set_attribute(s, "style", "overflow:auto").unwrap();
        p.flush().unwrap();
        assert_eq!(p.scroll_entry(target).unwrap().offset.1, 0.0);
    }
    // Distinct, freed nodes cannot accumulate dormant or active entries or
    // transfer an old offset to a later node that reuses its arena slot.
    for _ in 0..100 {
        let n = p
            .mutate(|d| {
                let n = d.create_element("div")?;
                let c = d.create_element("div")?;
                d.set_attribute(n, "style", "height:50px;overflow:auto")?;
                d.set_attribute(c, "style", "height:200px")?;
                d.append_child(n, c)?;
                d.append_child(b, n)?;
                Ok(n)
            })
            .unwrap();
        p.flush().unwrap();
        p.scroll_to(ScrollTarget::Node(n), 0.0, 30.0).unwrap();
        p.mutate(|d| {
            d.remove(n)?;
            d.collect();
            Ok(())
        })
        .unwrap();
        assert_eq!(p.scroll_offsets().tracked_nodes().count(), 0);
        assert!(!p.document().arena().contains(n));
    }
}
