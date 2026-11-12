//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! `CalcHandle` (006 `computed-style-view.md` §2; AD-12): an opaque, epoch-scoped handle to a
//! computed `calc()` length-percentage. Consumers resolve it against a basis (008 implements
//! Taffy's `resolve_calc_value` through it). Resolving with the epoch of another snapshot is
//! `REVISION_MISMATCH`, an internal invariant that is never published.

use std::fmt;
use std::sync::Arc;
use style::values::computed::{Length, LengthPercentage as StyloLp};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[derive(Clone)]
pub struct CalcHandle {
    epoch: u64,
    value: Arc<StyloLp>,
}

impl CalcHandle {
    pub(crate) fn new(epoch: u64, value: &StyloLp) -> CalcHandle {
        CalcHandle {
            epoch,
            value: Arc::new(value.clone()),
        }
    }

    /// The style epoch of the snapshot this handle came from.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Resolve against `basis_px` (the percentage basis), in CSS px.
    pub fn resolve(&self, basis_px: f32) -> f32 {
        self.value.resolve(Length::new(basis_px)).px()
    }

    /// Resolve only within this handle's style epoch; report a mismatch in every build.
    pub fn resolve_in(&self, epoch: u64, basis_px: f32) -> Result<f32, Status> {
        if epoch != self.epoch {
            return Err(Status::new(Code::RevisionMismatch, Stage::Cascade)
                .with_message("CalcHandle resolved with another style epoch"));
        }
        Ok(self.resolve(basis_px))
    }

    /// Whether the expression has a percentage part (needs a definite basis).
    pub fn has_percentage(&self) -> bool {
        self.value.has_percentage()
    }

    /// A stable address for this handle's expression while any clone of it is alive (8-byte
    /// aligned), usable as an opaque tag by consumers that store tagged pointers.
    pub fn as_ptr(&self) -> *const () {
        Arc::as_ptr(&self.value) as *const ()
    }
}

impl PartialEq for CalcHandle {
    /// Same epoch and the same expression.
    fn eq(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && (Arc::ptr_eq(&self.value, &other.value) || *self.value == *other.value)
    }
}

impl fmt::Debug for CalcHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use style_traits::ToCss;
        write!(
            f,
            "CalcHandle(epoch {}, {})",
            self.epoch,
            self.value.to_css_string()
        )
    }
}
