//! owner: 003-document-model
//!
//! The batch undo log (T078; research R-10; contract section 5). Instead of a hand-written list of
//! inverse operations (`Unlink`, `Relink`, `SetAttrs`, `SetData`, `SetControlState`, `SetUiFlags`,
//! `FreeCreated`, ...), the arena records a **before-image of every node the first time any writer
//! touches it** (`Arena::get_mut` is the only write route, so the control-state writer, the UI-state
//! writer and the 012 hooks that run inline are covered automatically) plus every slot allocated
//! since the batch began. Rollback restores the before-images and frees the created nodes, which makes
//! the restored arena identical to the pre-batch arena by construction:
//!
//! * `Unlink` / `Relink`: parent and child lists live in the node records restored above.
//! * `SetAttrs`, `SetData`, `SetControlState`, `SetUiFlags`: fields of the node record.
//! * `FreeCreated`: the created slots, freed in reverse allocation order (their ids go stale; they
//!   were never observable outside the batch).
//!
//! The journal's staging area (`Journal::begin_stage`) and the notification queue are handled by
//! `exec`; this module holds the shared documentation and the invariant test.

#[cfg(test)]
mod tests {
    use crate::document::Document;

    #[test]
    fn rollback_restores_the_arena_exactly() {
        let mut d = Document::parse_html("<div id=a class=x><p id=b>t</p></div>").unwrap();
        let before = d.serialize_document().unwrap();
        let live = d.arena().live_count();
        let a = d.element_by_id("a").unwrap();
        let b = d.element_by_id("b").unwrap();
        d.arena.begin_undo();
        let n = d.create_element("i").unwrap();
        d.append_child(a, n).unwrap();
        d.set_attribute(a, "class", "y").unwrap();
        d.set_text_content(b, "changed").unwrap();
        d.remove(b).unwrap();
        assert_ne!(d.serialize_document().unwrap(), before);
        d.arena.rollback_undo();
        assert_eq!(d.serialize_document().unwrap(), before);
        assert_eq!(d.arena().live_count(), live);
        assert!(d.arena().get(n).is_none(), "created nodes are freed");
        assert_eq!(
            d.arena().node(b).parent(),
            Some(a),
            "a removed node is back"
        );
    }
}
