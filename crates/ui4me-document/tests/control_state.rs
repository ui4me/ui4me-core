//! owner: 003-document-model
//!
//! T082 (FR-077-FR-081, OD-32, R-39): control-state and UI-state storage. Every field written through
//! `ControlStateWriter` journals, advances the revision and records `prior_control_flags` for the
//! selector-relevant ones; cloning copies the HTML-specified facts only; UI-state flags go through
//! the single writer, are journaled with old and new flags, and are cleared by the removal hook.

use ui4me_document::control_state::{
    ControlField, RangeState, Selection, SelectionDirection, validity,
};
use ui4me_document::document::Document;
use ui4me_document::journal::EntryKind;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::codes_generated::{Code, SubReason};

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

fn doc() -> Document {
    Document::parse_html("<form><input id=t value=abc><input id=c type=checkbox><textarea id=ta></textarea><select id=s><option id=o1>a<option id=o2 selected>b</select><div id=d></div></form>").unwrap()
}

// table-a: A-55
#[test]
fn stored_state_defaults_and_the_non_control_case() {
    let d = doc();
    let t = d.control_state(id(&d, "t")).unwrap().unwrap();
    assert_eq!(
        (t.value.as_deref(), t.dirty_value, t.checkedness),
        (Some("abc"), false, None)
    );
    let c = d.control_state(id(&d, "c")).unwrap().unwrap();
    assert_eq!(
        (c.value.as_deref(), c.checkedness),
        (Some("on"), Some(false)),
        "checkbox value defaults to on"
    );
    assert_eq!(
        d.control_state(id(&d, "o2")).unwrap().unwrap().selectedness,
        Some(true)
    );
    assert_eq!(
        d.control_state(id(&d, "o1")).unwrap().unwrap().selectedness,
        Some(false)
    );
    assert!(d.control_state(id(&d, "d")).unwrap().is_none());
}

// table-a: A-13, A-55
#[test]
fn every_field_write_journals_steps_the_revision_and_is_atomic() {
    let mut d = doc();
    let (t, o1) = (id(&d, "t"), id(&d, "o1"));
    let _ = d.take_epoch();
    let rev = d.revision();
    {
        let mut w = d.control_state_writer();
        w.set_value(t, "xyz").unwrap();
        w.set_selection(
            t,
            Some(Selection {
                start: 1,
                end: 3,
                direction: SelectionDirection::Forward,
            }),
        )
        .unwrap();
        w.set_user_validity(t, true).unwrap();
        w.set_validity_flags(t, validity::VALUE_MISSING | validity::CUSTOM_ERROR)
            .unwrap();
        w.set_custom_validity_message(t, "nope").unwrap();
        w.set_range_state(t, RangeState::OutOfRange).unwrap();
        w.set_composition(t, Some((0, 2))).unwrap();
        w.set_indeterminate(t, true).unwrap();
        w.set_selectedness(o1, true).unwrap();
    }
    assert_eq!(d.revision(), rev + 9, "one step per write");
    let seg = d.take_epoch();
    let fields: Vec<ControlField> = seg
        .entries()
        .iter()
        .filter_map(|e| match e.kind {
            EntryKind::ControlState { field, .. } => Some(field),
            _ => None,
        })
        .collect();
    assert_eq!(
        fields,
        [
            ControlField::Value,
            ControlField::Selection,
            ControlField::UserValidity,
            ControlField::ValidityFlags,
            ControlField::CustomValidityMessage,
            ControlField::RangeState,
            ControlField::Composition,
            ControlField::Indeterminate,
            ControlField::Selectedness
        ]
    );
    let priors = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::PriorControlFlags { .. }))
        .count();
    assert_eq!(
        priors, 2,
        "one prior_control_flags record per element per epoch, only for selector-relevant fields: t and o1"
    );
    let st = d.control_state(t).unwrap().unwrap();
    assert_eq!(
        (
            st.custom_validity_message.as_str(),
            st.range_state,
            st.composition
        ),
        ("nope", RangeState::OutOfRange, Some((0, 2)))
    );
    // writing the same value again is no mutation
    let r = d.revision();
    d.control_state_writer().set_value(t, "xyz").unwrap();
    assert_eq!(d.revision(), r);
}

// table-a: A-55
#[test]
fn selection_and_composition_ranges_must_sit_on_utf16_boundaries() {
    let mut d = doc();
    let t = id(&d, "t");
    d.control_state_writer()
        .set_value(t, "a\u{1F600}b")
        .unwrap();
    let rev = d.revision();
    let e = d
        .control_state_writer()
        .set_composition(t, Some((0, 2)))
        .unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::SurrogateSplit))
    );
    let e = d
        .control_state_writer()
        .set_selection(
            t,
            Some(Selection {
                start: 1,
                end: 9,
                direction: SelectionDirection::None,
            }),
        )
        .unwrap_err();
    assert_eq!(e.sub_reason, Some(SubReason::IndexSize));
    let e = d
        .control_state_writer()
        .set_composition(t, Some((3, 1)))
        .unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    assert_eq!(d.revision(), rev, "rejected writes change nothing");
    d.control_state_writer()
        .set_composition(t, Some((1, 3)))
        .unwrap();
    assert!(d.composition_is_valid(t));
    // wrong node kinds
    let div = id(&d, "d");
    assert_eq!(
        d.control_state_writer()
            .set_value(div, "x")
            .unwrap_err()
            .sub_reason,
        Some(SubReason::WrongNodeKind)
    );
    assert_eq!(
        d.control_state_writer()
            .set_selectedness(t, true)
            .unwrap_err()
            .sub_reason,
        Some(SubReason::WrongNodeKind)
    );
}

// table-a: A-55, A-12
#[test]
fn cloning_copies_value_and_checkedness_but_not_selection_composition_or_user_validity() {
    let mut d = doc();
    let t = id(&d, "t");
    {
        let mut w = d.control_state_writer();
        w.set_value(t, "typed").unwrap();
        w.set_selection(
            t,
            Some(Selection {
                start: 0,
                end: 2,
                direction: SelectionDirection::None,
            }),
        )
        .unwrap();
        w.set_composition(t, Some((0, 1))).unwrap();
        w.set_user_validity(t, true).unwrap();
    }
    let c = d.clone_node(t, false).unwrap();
    let s = d.control_state(c).unwrap().unwrap();
    assert_eq!((s.value.as_deref(), s.dirty_value), (Some("typed"), true));
    assert!(s.selection.is_none() && s.composition.is_none() && !s.user_validity);
}

// table-a: A-43
#[test]
fn content_attributes_drive_the_state_until_it_is_dirty() {
    let mut d = doc();
    let (t, c) = (id(&d, "t"), id(&d, "c"));
    d.set_attribute(t, "value", "new").unwrap();
    assert_eq!(
        d.control_state(t).unwrap().unwrap().value.as_deref(),
        Some("new")
    );
    d.control_state_writer().set_value(t, "user").unwrap();
    d.set_attribute(t, "value", "later").unwrap();
    assert_eq!(
        d.control_state(t).unwrap().unwrap().value.as_deref(),
        Some("user"),
        "dirty value ignores the attribute"
    );
    d.set_attribute(c, "checked", "").unwrap();
    assert_eq!(d.control_state(c).unwrap().unwrap().checkedness, Some(true));
    d.control_state_writer().set_checkedness(c, false).unwrap();
    d.set_attribute(c, "checked", "x").unwrap();
    assert_eq!(
        d.control_state(c).unwrap().unwrap().checkedness,
        Some(false),
        "dirty checkedness ignores the attribute"
    );
    let o1 = id(&d, "o1");
    d.set_attribute(o1, "selected", "").unwrap();
    assert_eq!(
        d.control_state(o1).unwrap().unwrap().selectedness,
        Some(true)
    );
}

// table-a: A-54
#[test]
fn ui_state_flags_are_written_only_through_the_single_writer_and_journaled() {
    let mut d = doc();
    assert!(d.take_ui_state_key().is_some());
    assert!(d.take_ui_state_key().is_none(), "one writer per document");
    let key2 = {
        let mut other = doc();
        other.take_ui_state_key().unwrap()
    };
    assert_eq!(
        d.ui_state_writer(&key2).err().map(|e| e.code),
        Some(Code::WrongDocument),
        "a key of another document is refused"
    );
    // all seven bits
    let all = UiFlags::FOCUS
        | UiFlags::FOCUS_VISIBLE
        | UiFlags::FOCUS_WITHIN
        | UiFlags::HOVER
        | UiFlags::ACTIVE
        | UiFlags::PICKER_OPEN
        | UiFlags::MODAL;
    assert_eq!(
        all.names(),
        [
            "focus",
            "focus-visible",
            "focus-within",
            "hover",
            "active",
            "picker-open",
            "modal"
        ]
    );
    assert_eq!(all.bits(), 0x7f);
}

// table-a: A-54
#[test]
fn ui_state_writes_and_the_removal_hook_clear_flags_before_the_call_returns() {
    let mut d = Document::parse_html("<div id=a><button id=b>x</button></div>").unwrap();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    let key = d.take_ui_state_key().unwrap();
    // a 012-like removal hook: clear flags in the removed subtree through the writer
    d.hooks_mut().set_on_removed(Box::new(|w, r| {
        for n in w.flagged_in_subtree(r.root, UiFlags::FOCUS | UiFlags::HOVER) {
            w.set_flags(n, UiFlags::empty()).unwrap();
        }
    }));
    d.ui_state_writer(&key)
        .unwrap()
        .set_flags(b, UiFlags::FOCUS | UiFlags::HOVER)
        .unwrap();
    assert_eq!(d.ui_flags(b).unwrap(), UiFlags::FOCUS | UiFlags::HOVER);
    let _ = d.take_epoch();
    let rev = d.revision();
    d.remove_child(a, b).unwrap();
    assert_eq!(
        d.ui_flags(b).unwrap(),
        UiFlags::empty(),
        "cleared inside the removing call"
    );
    assert_eq!(
        d.revision(),
        rev + 1,
        "the hook's writes belong to the call's single revision step"
    );
    let seg = d.take_epoch();
    let ui: Vec<_> = seg
        .entries()
        .iter()
        .filter_map(|e| match e.kind {
            EntryKind::UiState { old, new, .. } => Some((old, new)),
            _ => None,
        })
        .collect();
    assert_eq!(ui, [(UiFlags::FOCUS | UiFlags::HOVER, UiFlags::empty())]);
    // clones never carry UI state
    d.ui_state_writer(&key)
        .unwrap()
        .set_flags(a, UiFlags::HOVER)
        .unwrap();
    let c = d.clone_node(a, true).unwrap();
    assert_eq!(d.ui_flags(c).unwrap(), UiFlags::empty());
}
