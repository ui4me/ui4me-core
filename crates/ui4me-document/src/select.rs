//! owner: 012-interaction (lane L-CTRL-r10) on 003's stored control state — the `select` model
//! (WHATWG HTML section 4.10.7 "select", 4.10.10 "option", 4.10.9 "optgroup").
//!
//! The list of options of a `select`, the *selectedness setting algorithm* (kept up to date
//! inside the mutating call that changed an option, the select, or the child list, so the stored
//! `selectedness` of every option is always the one `:checked`, `selectedIndex` and `value`
//! report), the option label/value/text and the typed host operations (`selectedIndex`,
//! `value`, user picks). Writes go through [`ControlStateWriter`](crate::control_state::ControlStateWriter);
//! nothing here dispatches an event (events are 012's).

use crate::document::Document;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// HTML select display size, from its `size` and `multiple` attributes.
/// The non-negative integer algorithm accepts ASCII whitespace, one leading
/// plus and an ASCII digit prefix; zero, overflow and invalid input use the
/// control's default display size.
pub fn display_size(size: Option<&str>, multiple: bool) -> u32 {
    let parsed = size.and_then(|s| {
        let s = s.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
        let s = s.strip_prefix('+').unwrap_or(s);
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        s[..digits].parse::<u32>().ok().filter(|n| *n > 0)
    });
    parsed.unwrap_or(if multiple { 4 } else { 1 })
}

fn collapse(s: &str) -> String {
    s.split([' ', '\t', '\n', '\x0c', '\r'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One row of the popup or listbox of a select: an option or an `optgroup` label.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectRowRef {
    pub option: Option<NodeId>,
    /// Index in the list of options (`None` for a group label).
    pub option_index: Option<u32>,
    pub group: Option<NodeId>,
    /// An option inside an `optgroup`.
    pub indent: bool,
}

impl Document {
    fn is_html_el(&self, n: NodeId, name: &str) -> bool {
        self.arena
            .get(n)
            .and_then(|x| x.as_element())
            .is_some_and(|e| e.is_html_named(name))
    }

    fn el_attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.arena
            .get(n)
            .and_then(|x| x.as_element())
            .and_then(|e| e.attr(name))
    }

    /// The list of options of a `select`: its `option` children and those of its `optgroup`
    /// children, in tree order (empty for a non-select).
    pub fn select_options(&self, select: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        if !self.is_html_el(select, "select") {
            return out;
        }
        for &c in &self.arena.node(select).children {
            if self.is_html_el(c, "option") {
                out.push(c);
            } else if self.is_html_el(c, "optgroup") {
                for &g in &self.arena.node(c).children {
                    if self.is_html_el(g, "option") {
                        out.push(g);
                    }
                }
            }
        }
        out
    }

    /// The rows a select shows: its options (those with the `hidden` attribute left out) and its
    /// `optgroup` labels, in tree order. Row indexes are what the popup's scroll position counts.
    pub fn select_rows(&self, select: NodeId) -> Vec<SelectRowRef> {
        let options = self.select_options(select);
        let index_of = |o: NodeId| options.iter().position(|x| *x == o).map(|i| i as u32);
        let hidden = |n: NodeId| self.el_attr(n, "hidden").is_some();
        let mut out = Vec::new();
        if !self.is_html_el(select, "select") {
            return out;
        }
        for &c in &self.arena.node(select).children {
            if self.is_html_el(c, "option") && !hidden(c) {
                out.push(SelectRowRef {
                    option: Some(c),
                    option_index: index_of(c),
                    group: None,
                    indent: false,
                });
            } else if self.is_html_el(c, "optgroup") && !hidden(c) {
                out.push(SelectRowRef {
                    option: None,
                    option_index: None,
                    group: Some(c),
                    indent: false,
                });
                for &g in &self.arena.node(c).children {
                    if self.is_html_el(g, "option") && !hidden(g) {
                        out.push(SelectRowRef {
                            option: Some(g),
                            option_index: index_of(g),
                            group: Some(c),
                            indent: true,
                        });
                    }
                }
            }
        }
        out
    }

    /// The select whose list of options contains `option`: its parent select, or the select
    /// parent of its parent `optgroup`.
    pub fn option_select(&self, option: NodeId) -> Option<NodeId> {
        let p = self.arena.get(option)?.parent()?;
        if self.is_html_el(p, "select") {
            return Some(p);
        }
        if self.is_html_el(p, "optgroup") {
            let g = self.arena.get(p)?.parent()?;
            return self.is_html_el(g, "select").then_some(g);
        }
        None
    }

    /// `multiple` attribute present.
    pub fn select_is_multiple(&self, select: NodeId) -> bool {
        self.el_attr(select, "multiple").is_some()
    }

    /// The display size: `size` when a valid positive integer, else 4 with `multiple`, else 1.
    pub fn select_display_size(&self, select: NodeId) -> u32 {
        display_size(
            self.el_attr(select, "size"),
            self.select_is_multiple(select),
        )
    }

    /// A drop-down (menulist) has display size one, including `multiple size=1`.
    pub fn select_is_dropdown(&self, select: NodeId) -> bool {
        self.select_display_size(select) == 1
    }

    /// `option` disabled: its own `disabled` attribute or a disabled `optgroup` parent.
    pub fn option_disabled(&self, option: NodeId) -> bool {
        if self.el_attr(option, "disabled").is_some() {
            return true;
        }
        match self.arena.get(option).and_then(|n| n.parent()) {
            Some(p) if self.is_html_el(p, "optgroup") => self.el_attr(p, "disabled").is_some(),
            _ => false,
        }
    }

    /// `option.text`: the descendant text with white space stripped and collapsed.
    pub fn option_text(&self, option: NodeId) -> String {
        let mut s = String::new();
        for n in self.arena.subtree(option) {
            if let Some(t) = self
                .arena
                .get(n)
                .filter(|x| x.kind() == crate::node::NodeKind::Text)
                .and_then(|x| x.character_data())
            {
                // Skip text inside a `script`.
                let mut skip = false;
                let mut a = self.arena.get(n).and_then(|x| x.parent());
                while let Some(p) = a {
                    if p == option {
                        break;
                    }
                    if self.is_html_el(p, "script") {
                        skip = true;
                    }
                    a = self.arena.get(p).and_then(|x| x.parent());
                }
                if !skip {
                    s.push_str(t);
                }
            }
        }
        collapse(&s)
    }

    /// `option.label`: the `label` attribute when not empty, else the text.
    pub fn option_label(&self, option: NodeId) -> String {
        match self.el_attr(option, "label") {
            Some(l) if !l.is_empty() => l.to_owned(),
            _ => self.option_text(option),
        }
    }

    /// `option.value`: the `value` attribute, else the text.
    pub fn option_value(&self, option: NodeId) -> String {
        match self.el_attr(option, "value") {
            Some(v) => v.to_owned(),
            None => self.option_text(option),
        }
    }

    /// Selectedness of an option (stored).
    pub fn option_selected(&self, option: NodeId) -> bool {
        self.arena
            .get(option)
            .and_then(|n| n.as_element())
            .and_then(|e| e.control.as_ref())
            .and_then(|c| c.selectedness)
            .unwrap_or(false)
    }

    /// `option.index`: its index in its select's list of options (0 without a select).
    pub fn option_index(&self, option: NodeId) -> u32 {
        self.option_select(option)
            .and_then(|s| self.select_options(s).iter().position(|o| *o == option))
            .unwrap_or(0) as u32
    }

    /// `select.selectedIndex`: the index of the first selected option, -1 when none.
    pub fn select_selected_index(&self, select: NodeId) -> i32 {
        self.select_options(select)
            .iter()
            .position(|o| self.option_selected(*o))
            .map_or(-1, |i| i as i32)
    }

    /// `select.value`: the value of the first selected option, "" when none.
    pub fn select_value(&self, select: NodeId) -> String {
        self.select_options(select)
            .into_iter()
            .find(|o| self.option_selected(*o))
            .map(|o| self.option_value(o))
            .unwrap_or_default()
    }

    /// The label a drop-down shows: the first selected option's label ("" when none).
    pub fn select_display_label(&self, select: NodeId) -> String {
        self.select_options(select)
            .into_iter()
            .find(|o| self.option_selected(*o))
            .map(|o| self.option_label(o))
            .unwrap_or_default()
    }

    /// The selectedness setting algorithm for `select` (HTML 4.10.7): with `multiple` absent, of
    /// several selected options only the last stays selected; with display size 1 and none
    /// selected, the first option that is not disabled becomes selected. Not dirty-making.
    pub(crate) fn select_reset(&mut self, select: NodeId) {
        let options = self.select_options(select);
        if options.is_empty() || self.select_is_multiple(select) {
            return;
        }
        let selected: Vec<NodeId> = options
            .iter()
            .copied()
            .filter(|o| self.option_selected(*o))
            .collect();
        if selected.len() > 1 {
            for o in &selected[..selected.len() - 1] {
                let _ = self
                    .control_state_writer()
                    .set_selectedness_clean(*o, false);
            }
        } else if selected.is_empty() && self.select_display_size(select) == 1 {
            let first = options.iter().copied().find(|o| !self.option_disabled(*o));
            if let Some(o) = first {
                let _ = self.control_state_writer().set_selectedness_clean(o, true);
            }
        }
    }

    /// End of a full-document parse: every select runs the selectedness setting algorithm.
    pub(crate) fn finish_parse_selects(&mut self) {
        let selects: Vec<NodeId> = self
            .arena
            .iter()
            .filter(|n| n.as_element().is_some_and(|e| e.is_html_named("select")))
            .map(|n| n.id)
            .collect();
        for s in selects {
            self.select_reset(s);
        }
    }

    /// An option just became selected (its `selected` attribute was set, or it was inserted): in a
    /// single-selection select it wins over the other options (HTML 4.10.7: "set the selectedness
    /// of all the other option elements to false").
    pub(crate) fn select_option_wins(&mut self, option: NodeId) {
        let Some(select) = self.option_select(option) else {
            return;
        };
        if self.select_is_multiple(select) || !self.option_selected(option) {
            return;
        }
        for o in self.select_options(select) {
            if o != option && self.option_selected(o) {
                let _ = self.control_state_writer().set_selectedness_clean(o, false);
            }
        }
    }

    /// Nodes were inserted under `parent`: a selected option among them wins (the last one, when
    /// several came), then the algorithm runs.
    pub(crate) fn select_inserted(&mut self, parent: NodeId, inserted: &[NodeId]) {
        let winner = inserted
            .iter()
            .flat_map(|n| self.arena.subtree(*n))
            .rfind(|n| {
                self.is_html_el(*n, "option")
                    && self.option_selected(*n)
                    && self.option_select(*n).is_some()
            });
        if let Some(w) = winner {
            self.select_option_wins(w);
        }
        self.select_follow(parent);
    }

    /// A change of `el` (an attribute, or its children): re-run the algorithm of the select that
    /// owns it. Called inside the mutating call.
    pub(crate) fn select_follow(&mut self, el: NodeId) {
        let Some(e) = self.arena.get(el).and_then(|n| n.as_element()) else {
            return;
        };
        let select = if e.is_html_named("select") {
            Some(el)
        } else if e.is_html_named("option") {
            self.option_select(el)
        } else if e.is_html_named("optgroup") {
            self.arena
                .get(el)
                .and_then(|n| n.parent())
                .filter(|p| self.is_html_el(*p, "select"))
        } else {
            None
        };
        if let Some(s) = select {
            self.select_reset(s);
        }
    }

    /// `select.selectedIndex = i`: that option alone selected (all unselected for an
    /// out-of-range index); every option becomes dirty.
    pub fn select_set_selected_index(&mut self, select: NodeId, index: i32) -> Result<(), Status> {
        self.check_writable(select)?;
        let options = self.select_options(select);
        self.begin_call();
        for (i, o) in options.iter().enumerate() {
            let _ = self
                .control_state_writer()
                .set_selectedness(*o, i as i32 == index);
        }
        self.end_call();
        Ok(())
    }

    /// `select.value = v`: the first option with that value alone selected, else none selected.
    pub fn select_set_value(&mut self, select: NodeId, value: &str) -> Result<(), Status> {
        let options = self.select_options(select);
        let idx = options
            .iter()
            .position(|o| self.option_value(*o) == value)
            .map_or(-1, |i| i as i32);
        self.select_set_selected_index(select, idx)
    }

    /// `option.selected = v`. In a single-selection select the option wins over the others.
    pub fn option_set_selected(&mut self, option: NodeId, selected: bool) -> Result<(), Status> {
        self.check_writable(option)?;
        let select = self.option_select(option);
        self.begin_call();
        let _ = self
            .control_state_writer()
            .set_selectedness(option, selected);
        if let Some(s) = select {
            if selected && !self.select_is_multiple(s) {
                for o in self.select_options(s) {
                    if o != option {
                        let _ = self.control_state_writer().set_selectedness(o, false);
                    }
                }
            }
            self.select_reset(s);
        }
        self.end_call();
        Ok(())
    }

    /// A user pick of `option` (012): single select - that option alone; `multiple` - toggled when
    /// `toggle`, else selected alone. Returns whether any selectedness changed.
    pub fn select_user_pick(&mut self, option: NodeId, toggle: bool) -> Result<bool, Status> {
        let Some(select) = self.option_select(option) else {
            return Ok(false);
        };
        let before: Vec<bool> = self
            .select_options(select)
            .iter()
            .map(|o| self.option_selected(*o))
            .collect();
        self.begin_call();
        if self.select_is_multiple(select) && toggle {
            let now = self.option_selected(option);
            let _ = self.control_state_writer().set_selectedness(option, !now);
        } else {
            for o in self.select_options(select) {
                let _ = self.control_state_writer().set_selectedness(o, o == option);
            }
        }
        self.end_call();
        let after: Vec<bool> = self
            .select_options(select)
            .iter()
            .map(|o| self.option_selected(*o))
            .collect();
        Ok(before != after)
    }
}
