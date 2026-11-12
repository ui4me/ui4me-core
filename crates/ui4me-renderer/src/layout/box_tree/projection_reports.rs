//! Diagnostics for typed style values outside the consumed layout grammar.
use ui4me_engine_types::diag::FeatureId;

/// Collects the features whose computed value was outside the consumed grammar.
pub struct Unconsumed<'a> {
    pub out: &'a mut Vec<FeatureId>,
    pub intern: &'a dyn Fn(&str) -> FeatureId,
}

impl Unconsumed<'_> {
    pub(super) fn push(&mut self, id: String) {
        let f = (self.intern)(&id);
        if !self.out.contains(&f) {
            self.out.push(f);
        }
    }
    pub(super) fn property(&mut self, name: &str) {
        self.push(format!("css.property.{name}"));
    }
    /// A value-level id (`css.value.<name>`), for FR-043 and FR-047.
    pub(super) fn value(&mut self, name: &str) {
        self.push(format!("css.value.{name}"));
    }
}
