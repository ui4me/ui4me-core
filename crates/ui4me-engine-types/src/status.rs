//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Status value carried by every operation result (core-types.md §4; detail fields 004
//! error-mapping §4; the ABI transfer of the message buffer is owned by 004).

use crate::codes_generated::{Code, Family, SubReason};
use crate::ids::DispatchId;
use crate::stage::Stage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub code: Code,
    pub stage: Stage,
    pub sub_reason: Option<SubReason>,
    /// 0 = none.
    pub diagnostic_id: u64,
    /// UTF-8 message.
    pub message: String,
    pub dispatch_id: Option<DispatchId>,
    pub batch_index: Option<u32>,
    pub byte_offset: Option<u64>,
}

impl Status {
    pub fn ok() -> Self {
        Status::new(Code::Ok, Stage::None)
    }
    pub fn new(code: Code, stage: Stage) -> Self {
        Status {
            code,
            stage,
            sub_reason: None,
            diagnostic_id: 0,
            message: String::new(),
            dispatch_id: None,
            batch_index: None,
            byte_offset: None,
        }
    }
    /// The family is derived from the code (never stored separately, so it cannot disagree).
    pub fn family(&self) -> Family {
        self.code.family()
    }
    pub fn is_ok(&self) -> bool {
        self.code == Code::Ok
    }
    pub fn with_sub_reason(mut self, r: SubReason) -> Self {
        self.sub_reason = Some(r);
        self
    }
    pub fn with_diagnostic_id(mut self, id: u64) -> Self {
        self.diagnostic_id = id;
        self
    }
    pub fn with_message(mut self, m: impl Into<String>) -> Self {
        self.message = m.into();
        self
    }
    pub fn with_dispatch_id(mut self, d: DispatchId) -> Self {
        self.dispatch_id = Some(d);
        self
    }
    pub fn with_batch_index(mut self, i: u32) -> Self {
        self.batch_index = Some(i);
        self
    }
    pub fn with_byte_offset(mut self, o: u64) -> Self {
        self.byte_offset = Some(o);
        self
    }
}
