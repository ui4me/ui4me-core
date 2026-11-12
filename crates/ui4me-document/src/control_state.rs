//! owner: 003-document-model — created by 001 slice (R-28), completed by T085
//!
//! Stored control state (FR-077-FR-078; AD-28; OD-32; R-39; data-model section 2). DocumentCore stores
//! the facts; their semantics (defaults, sanitisation, validity computation, range checks) are 012's.
//! Writes go through [`ControlStateWriter`]; each journals `control_state {field}` and, for the first
//! selector-relevant change of the epoch, `prior_control_flags`, and steps the DOM revision once (or
//! belongs to the hooked/batched call's single step). Selection and composition ranges are UTF-16
//! offsets into the value (invariant 14): they are validated against the value and must not split a
//! surrogate pair. Cloning copies value, dirty value, checkedness, dirty checkedness (and the option
//! selectedness facts) but never `composition`, `selection` or `user_validity`.

use crate::chardata::byte_offset;
use crate::document::Document;
use crate::errors::wrong_node_kind;
use crate::journal::EntryKind;
use crate::node::ElementData;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Direction of a text selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionDirection {
    Forward,
    Backward,
    #[default]
    None,
}

/// `selection`: UTF-16 offsets into the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: u32,
    pub end: u32,
    pub direction: SelectionDirection,
}

/// `:in-range` / `:out-of-range` state of an `input` with range limitations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RangeState {
    #[default]
    None,
    InRange,
    OutOfRange,
}

/// The HTML validity-state bits plus `valid` (OD-32); selector-relevant.
pub mod validity {
    pub const VALUE_MISSING: u16 = 1 << 0;
    pub const TYPE_MISMATCH: u16 = 1 << 1;
    pub const PATTERN_MISMATCH: u16 = 1 << 2;
    pub const TOO_LONG: u16 = 1 << 3;
    pub const TOO_SHORT: u16 = 1 << 4;
    pub const RANGE_UNDERFLOW: u16 = 1 << 5;
    pub const RANGE_OVERFLOW: u16 = 1 << 6;
    pub const STEP_MISMATCH: u16 = 1 << 7;
    pub const BAD_INPUT: u16 = 1 << 8;
    pub const CUSTOM_ERROR: u16 = 1 << 9;
    pub const VALID: u16 = 1 << 10;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlState {
    pub value: Option<String>,
    pub dirty_value: bool,
    /// `Some` for checkbox and radio inputs.
    pub checkedness: Option<bool>,
    pub dirty_checkedness: bool,
    pub indeterminate: bool,
    /// `Some` for `option`.
    pub selectedness: Option<bool>,
    pub dirty_selectedness: bool,
    pub selection: Option<Selection>,
    pub user_validity: bool,
    pub validity_flags: u16,
    pub custom_validity_message: String,
    pub range_state: RangeState,
    /// Preedit range (UTF-16 units); the preedit text is part of `value` (R-39).
    pub composition: Option<(u32, u32)>,
    /// `select` dropdown popup (012 lane L-CTRL-r10): the highlighted option, as an index into the
    /// select's list of options, while the popup is open. Not cloned.
    pub picker_highlight: Option<u32>,
    /// `select` popup: index of the first visible row while the popup is open. Not cloned.
    pub picker_scroll: u32,
}

/// Selector-relevant control facts (`prior_control_flags`, mutation-journal.md section 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ControlFlags {
    pub checked: Option<bool>,
    pub indeterminate: bool,
    pub selected: Option<bool>,
    pub validity_flags: u16,
    pub user_validity: bool,
    pub range_state: RangeState,
    pub value_empty: bool,
}

impl ControlState {
    pub fn flags(&self) -> ControlFlags {
        ControlFlags {
            checked: self.checkedness,
            indeterminate: self.indeterminate,
            selected: self.selectedness,
            validity_flags: self.validity_flags,
            user_validity: self.user_validity,
            range_state: self.range_state,
            value_empty: self.value.as_deref().is_none_or(str::is_empty),
        }
    }
}

/// The written field (`control_state` entry `field`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlField {
    Value,
    DirtyValue,
    Checkedness,
    DirtyCheckedness,
    Indeterminate,
    Selectedness,
    DirtySelectedness,
    Selection,
    UserValidity,
    ValidityFlags,
    CustomValidityMessage,
    RangeState,
    Composition,
    Picker,
}

impl ControlField {
    pub const fn name(self) -> &'static str {
        match self {
            ControlField::Value => "value",
            ControlField::DirtyValue => "dirty_value",
            ControlField::Checkedness => "checkedness",
            ControlField::DirtyCheckedness => "dirty_checkedness",
            ControlField::Indeterminate => "indeterminate",
            ControlField::Selectedness => "selectedness",
            ControlField::DirtySelectedness => "dirty_selectedness",
            ControlField::Selection => "selection",
            ControlField::UserValidity => "user_validity",
            ControlField::ValidityFlags => "validity_flags",
            ControlField::CustomValidityMessage => "custom_validity_message",
            ControlField::RangeState => "range_state",
            ControlField::Composition => "composition",
            ControlField::Picker => "picker",
        }
    }
}

/// Initial control state of an element created by the parser or `createElement` (HTML: input
/// value from `value`, checkedness from `checked` for checkbox/radio; textarea/select get a
/// record without checkedness; option gets its selectedness). `None` for non-controls.
pub(crate) fn initial_for(e: &ElementData) -> Option<ControlState> {
    if !e.is_html() {
        return None;
    }
    match e.local_name() {
        "input" => {
            let checkable = e.input_type().is_some_and(|kind| kind.checkable());
            Some(ControlState {
                // HTML "value" IDL, mode `default/on`: a checkbox or radio without a `value`
                // attribute reports "on".
                value: Some(
                    e.attr("value")
                        .unwrap_or(if checkable { "on" } else { "" })
                        .to_owned(),
                ),
                checkedness: checkable.then(|| e.attr("checked").is_some()),
                ..Default::default()
            })
        }
        "textarea" | "select" => Some(ControlState {
            value: Some(String::new()),
            ..Default::default()
        }),
        "option" => Some(ControlState {
            selectedness: Some(e.attr("selected").is_some()),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Typed setters over the stored control state.
pub struct ControlStateWriter<'a> {
    pub(crate) doc: &'a mut Document,
}

impl ControlStateWriter<'_> {
    fn write(
        &mut self,
        el: NodeId,
        field: ControlField,
        f: impl FnOnce(&mut ControlState) -> Result<bool, Status>,
    ) -> Result<(), Status> {
        self.doc.check_writable(el)?;
        let before = self
            .doc
            .arena
            .get(el)
            .and_then(|n| n.as_element())
            .and_then(|e| e.control.clone())
            .ok_or_else(wrong_node_kind)?;
        let mut after = before.clone();
        if !f(&mut after)? || after == before {
            return Ok(());
        }
        self.doc.begin_call();
        let rev = self.doc.pending_revision();
        if before.flags() != after.flags() {
            self.doc.journal.note_control_flags(rev, el, before.flags());
        }
        self.doc
            .arena
            .get_mut(el)
            .and_then(|n| n.as_element_mut())
            .expect("checked")
            .control = Some(after);
        self.doc
            .journal
            .push(rev, EntryKind::ControlState { node: el, field });
        self.doc.end_call();
        Ok(())
    }

    /// Checkedness (checkbox/radio); also sets the dirty-checkedness flag.
    pub fn set_checkedness(&mut self, el: NodeId, checked: bool) -> Result<(), Status> {
        self.write(el, ControlField::Checkedness, |c| {
            if c.checkedness.is_none() {
                return Err(wrong_node_kind());
            }
            c.checkedness = Some(checked);
            c.dirty_checkedness = true;
            Ok(true)
        })
    }

    pub fn set_value(&mut self, el: NodeId, value: &str) -> Result<(), Status> {
        self.doc.check_string(value)?;
        self.write(el, ControlField::Value, |c| {
            if c.value.is_none() {
                return Err(wrong_node_kind());
            }
            c.value = Some(value.to_owned());
            c.dirty_value = true;
            // Offsets into the old value are no longer meaningful past the new end.
            Ok(true)
        })
    }

    pub fn set_indeterminate(&mut self, el: NodeId, on: bool) -> Result<(), Status> {
        self.write(el, ControlField::Indeterminate, |c| {
            c.indeterminate = on;
            Ok(true)
        })
    }

    /// Selectedness of an `option`; also sets the dirty flag.
    pub fn set_selectedness(&mut self, el: NodeId, selected: bool) -> Result<(), Status> {
        self.write(el, ControlField::Selectedness, |c| {
            if c.selectedness.is_none() {
                return Err(wrong_node_kind());
            }
            c.selectedness = Some(selected);
            c.dirty_selectedness = true;
            Ok(true)
        })
    }

    /// Selectedness of an `option` without touching the dirty flag (the select's reset algorithm).
    pub(crate) fn set_selectedness_clean(
        &mut self,
        el: NodeId,
        selected: bool,
    ) -> Result<(), Status> {
        self.write(el, ControlField::Selectedness, |c| {
            if c.selectedness.is_none() {
                return Err(wrong_node_kind());
            }
            c.selectedness = Some(selected);
            Ok(true)
        })
    }

    /// HTML "reset algorithm" of one control: the given default value, checkedness and selectedness
    /// (`None` = the control has none of that fact), all dirty flags cleared, selection, composition
    /// and user validity dropped.
    pub(crate) fn reset_to_defaults(
        &mut self,
        el: NodeId,
        value: Option<String>,
        checked: Option<bool>,
        selected: Option<bool>,
    ) -> Result<(), Status> {
        self.write(el, ControlField::Value, |c| {
            if let (Some(v), Some(_)) = (value, c.value.as_ref()) {
                c.value = Some(v);
            }
            c.dirty_value = false;
            if checked.is_some() && c.checkedness.is_some() {
                c.checkedness = checked;
            }
            c.dirty_checkedness = false;
            if selected.is_some() && c.selectedness.is_some() {
                c.selectedness = selected;
            }
            c.dirty_selectedness = false;
            c.selection = None;
            c.composition = None;
            c.user_validity = false;
            Ok(true)
        })
    }

    /// The popup state of a `select`: highlighted option index and first visible row.
    pub fn set_picker(
        &mut self,
        el: NodeId,
        highlight: Option<u32>,
        scroll: u32,
    ) -> Result<(), Status> {
        self.write(el, ControlField::Picker, |c| {
            c.picker_highlight = highlight;
            c.picker_scroll = scroll;
            Ok(true)
        })
    }

    /// Text selection (UTF-16 offsets into the value; `start <= end`, on code-point boundaries).
    pub fn set_selection(
        &mut self,
        el: NodeId,
        selection: Option<Selection>,
    ) -> Result<(), Status> {
        self.write(el, ControlField::Selection, |c| {
            if let Some(s) = selection {
                check_range(
                    c.value.as_deref().ok_or_else(wrong_node_kind)?,
                    s.start,
                    s.end,
                )?;
            }
            c.selection = selection;
            Ok(true)
        })
    }

    pub fn set_user_validity(&mut self, el: NodeId, on: bool) -> Result<(), Status> {
        self.write(el, ControlField::UserValidity, |c| {
            c.user_validity = on;
            Ok(true)
        })
    }

    /// The validity-state bits (see [`validity`]).
    pub fn set_validity_flags(&mut self, el: NodeId, flags: u16) -> Result<(), Status> {
        self.write(el, ControlField::ValidityFlags, |c| {
            c.validity_flags = flags;
            Ok(true)
        })
    }

    pub fn set_custom_validity_message(&mut self, el: NodeId, message: &str) -> Result<(), Status> {
        self.doc.check_string(message)?;
        self.write(el, ControlField::CustomValidityMessage, |c| {
            c.custom_validity_message = message.to_owned();
            Ok(true)
        })
    }

    pub fn set_range_state(&mut self, el: NodeId, state: RangeState) -> Result<(), Status> {
        self.write(el, ControlField::RangeState, |c| {
            c.range_state = state;
            Ok(true)
        })
    }

    /// The preedit composition range (UTF-16 offsets; invariant 14).
    pub fn set_composition(&mut self, el: NodeId, range: Option<(u32, u32)>) -> Result<(), Status> {
        self.write(el, ControlField::Composition, |c| {
            if let Some((s, e)) = range {
                check_range(c.value.as_deref().ok_or_else(wrong_node_kind)?, s, e)?;
            }
            c.composition = range;
            Ok(true)
        })
    }
}

/// `start <= end <= length` and neither offset splits a surrogate pair.
fn check_range(value: &str, start: u32, end: u32) -> Result<(), Status> {
    if start > end {
        return Err(crate::errors::index_size());
    }
    byte_offset(value, start as usize)?;
    byte_offset(value, end as usize)?;
    Ok(())
}

impl Document {
    /// The value a text control shows and edits. A `textarea` whose value was never written
    /// (not dirty) has the *raw value* of its text children as its default value (HTML
    /// `textarea` "reset algorithm": the child text content, a first line feed removed);
    /// everything else is the stored value. `None` for a non-control (lane L-EDIT-r4; the
    /// stored value of a clean `textarea` stays empty until written, 012's `on_children_changed`
    /// hook would keep it in step).
    pub fn control_value(&self, el: NodeId) -> Option<String> {
        let c = self.arena.get(el)?.as_element()?.control.as_ref()?;
        let clean_textarea =
            !c.dirty_value && self.arena.get(el)?.as_element()?.local_name() == "textarea";
        if !clean_textarea {
            return c.value.clone();
        }
        let mut text = String::new();
        for child in self.children(el).ok()? {
            if let Ok(t) = self.text_data(*child) {
                text.push_str(t);
            }
        }
        Some(
            text.strip_prefix("\r\n")
                .or_else(|| text.strip_prefix('\n'))
                .map(str::to_owned)
                .unwrap_or(text),
        )
    }

    /// The control-state invariant 14 (composition on UTF-16 boundaries) for tests and the oracle.
    pub fn composition_is_valid(&self, el: NodeId) -> bool {
        let Some(c) = self
            .arena
            .get(el)
            .and_then(|n| n.as_element())
            .and_then(|e| e.control.as_ref())
        else {
            return true;
        };
        match (c.composition, c.value.as_deref()) {
            (Some((s, e)), Some(v)) => check_range(v, s, e).is_ok(),
            (Some(_), None) => false,
            _ => true,
        }
    }
}
