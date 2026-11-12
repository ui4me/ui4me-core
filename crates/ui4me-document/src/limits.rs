//! owner: 003-document-model
//!
//! Configurable limits (T014; FR-093, FR-095; OD-19 pending; research R-20).
//!
//! `DomLimits` carries the FR-093 defaults and the R-20 hard maxima. 004's runtime configuration
//! supplies requested values at runtime creation; they are clamped to the hard maxima here and the
//! struct is read-only afterwards. Every violation is `SIZE_LIMIT` naming the limit, raised before
//! commit so there is no partial effect.

use crate::document::Document;
use crate::errors::{out_of_memory, size_limit};
use ui4me_engine_types::status::Status;

pub const MIB: u64 = 1 << 20;
pub const GIB: u64 = 1 << 30;

/// The limit names used in `SIZE_LIMIT` messages (FR-093).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitName {
    InputBytes,
    Nodes,
    StringBytes,
    SerializationBytes,
    BatchCommands,
    BatchBytes,
}

impl LimitName {
    pub const fn name(self) -> &'static str {
        match self {
            LimitName::InputBytes => "dom.input_bytes",
            LimitName::Nodes => "dom.nodes",
            LimitName::StringBytes => "dom.string_bytes",
            LimitName::SerializationBytes => "dom.serialization_bytes",
            LimitName::BatchCommands => "dom.batch_commands",
            LimitName::BatchBytes => "dom.batch_bytes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomLimits {
    /// Input size per parse (default 64 MiB; hard maximum 1 GiB).
    pub input_bytes: u64,
    /// Live nodes per document (default 4,194,304; hard maximum 2^28).
    pub nodes: u64,
    /// One attribute value or character-data string (default 16 MiB; hard maximum 256 MiB).
    pub string_bytes: u64,
    /// Serialization output (default 256 MiB; hard maximum 2 GiB).
    pub serialization_bytes: u64,
    /// Batch size in commands (default 1,048,576; hard maximum 16,777,216).
    pub batch_commands: u64,
    /// Batch size in bytes (default 256 MiB).
    pub batch_bytes: u64,
}

impl DomLimits {
    pub const MAX_INPUT_BYTES: u64 = GIB;
    pub const MAX_NODES: u64 = 1 << 28;
    pub const MAX_STRING_BYTES: u64 = 256 * MIB;
    pub const MAX_SERIALIZATION_BYTES: u64 = 2 * GIB;
    pub const MAX_BATCH_COMMANDS: u64 = 16_777_216;
    pub const MAX_BATCH_BYTES: u64 = GIB;

    /// Requested values clamped to the hard maxima (a zero request keeps the default).
    pub fn configured(req: DomLimits) -> DomLimits {
        let d = DomLimits::default();
        let pick = |want: u64, def: u64, max: u64| if want == 0 { def } else { want.min(max) };
        DomLimits {
            input_bytes: pick(req.input_bytes, d.input_bytes, Self::MAX_INPUT_BYTES),
            nodes: pick(req.nodes, d.nodes, Self::MAX_NODES),
            string_bytes: pick(req.string_bytes, d.string_bytes, Self::MAX_STRING_BYTES),
            serialization_bytes: pick(
                req.serialization_bytes,
                d.serialization_bytes,
                Self::MAX_SERIALIZATION_BYTES,
            ),
            batch_commands: pick(
                req.batch_commands,
                d.batch_commands,
                Self::MAX_BATCH_COMMANDS,
            ),
            batch_bytes: pick(req.batch_bytes, d.batch_bytes, Self::MAX_BATCH_BYTES),
        }
    }

    /// `SIZE_LIMIT` naming `which` if `value` exceeds it.
    pub fn check(&self, which: LimitName, value: u64) -> Result<(), Status> {
        let max = match which {
            LimitName::InputBytes => self.input_bytes,
            LimitName::Nodes => self.nodes,
            LimitName::StringBytes => self.string_bytes,
            LimitName::SerializationBytes => self.serialization_bytes,
            LimitName::BatchCommands => self.batch_commands,
            LimitName::BatchBytes => self.batch_bytes,
        };
        if value > max {
            Err(size_limit(which.name()))
        } else {
            Ok(())
        }
    }
}

impl Default for DomLimits {
    fn default() -> Self {
        DomLimits {
            input_bytes: 64 * MIB,
            nodes: 4_194_304,
            string_bytes: 16 * MIB,
            serialization_bytes: 256 * MIB,
            batch_commands: 1_048_576,
            batch_bytes: 256 * MIB,
        }
    }
}

impl Document {
    pub fn limits(&self) -> &DomLimits {
        &self.limits
    }

    /// Replace the limits (runtime creation only; clamped). Not callable once the document is shared:
    /// the setter takes `&mut` and is used by the creation path.
    pub fn with_limits(mut self, limits: DomLimits) -> Document {
        self.limits = DomLimits::configured(limits);
        self
    }

    /// Fail with `SIZE_LIMIT` (naming `dom.nodes`) if `extra` more nodes would exceed the node limit.
    pub(crate) fn check_node_budget(&self, extra: u64) -> Result<(), Status> {
        self.check_alloc()?;
        self.limits
            .check(LimitName::Nodes, self.arena.live_count() as u64 + extra)
    }

    /// Fail with `SIZE_LIMIT` (naming `dom.string_bytes`) for an over-long string value.
    pub(crate) fn check_string(&self, s: &str) -> Result<(), Status> {
        self.limits.check(LimitName::StringBytes, s.len() as u64)
    }

    /// Allocation-failure injection point (`test-sinks` only): the n-th fallible allocation fails.
    pub(crate) fn check_alloc(&self) -> Result<(), Status> {
        #[cfg(feature = "test-sinks")]
        {
            let n = self.fail_alloc_countdown.get();
            if n == 1 {
                self.fail_alloc_countdown.set(0);
                return Err(out_of_memory());
            }
            if n > 1 {
                self.fail_alloc_countdown.set(n - 1);
            }
        }
        let _ = out_of_memory; // referenced so the import is used without the feature
        Ok(())
    }

    /// Make the `n`-th (1-based) following fallible allocation fail with `OUT_OF_MEMORY` (0 disarms).
    #[cfg(feature = "test-sinks")]
    pub fn inject_allocation_failure(&self, n: u32) {
        self.fail_alloc_countdown.set(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::Code;

    #[test]
    fn defaults_are_the_fr093_planning_values() {
        let d = DomLimits::default();
        assert_eq!(d.input_bytes, 64 * MIB);
        assert_eq!(d.nodes, 4_194_304);
        assert_eq!(d.string_bytes, 16 * MIB);
        assert_eq!(d.serialization_bytes, 256 * MIB);
        assert_eq!(d.batch_commands, 1_048_576);
        assert_eq!(d.batch_bytes, 256 * MIB);
    }

    #[test]
    fn configuration_is_clamped_to_the_hard_maxima_and_zero_keeps_the_default() {
        let huge = DomLimits {
            input_bytes: u64::MAX,
            nodes: u64::MAX,
            string_bytes: u64::MAX,
            serialization_bytes: u64::MAX,
            batch_commands: u64::MAX,
            batch_bytes: u64::MAX,
        };
        let c = DomLimits::configured(huge);
        assert_eq!(c.input_bytes, GIB);
        assert_eq!(c.nodes, 1 << 28);
        assert_eq!(c.string_bytes, 256 * MIB);
        assert_eq!(c.serialization_bytes, 2 * GIB);
        assert_eq!(c.batch_commands, 16_777_216);
        let zero = DomLimits {
            input_bytes: 0,
            nodes: 0,
            string_bytes: 0,
            serialization_bytes: 0,
            batch_commands: 0,
            batch_bytes: 0,
        };
        assert_eq!(DomLimits::configured(zero), DomLimits::default());
        let small = DomLimits {
            nodes: 10,
            ..DomLimits::default()
        };
        assert_eq!(DomLimits::configured(small).nodes, 10);
    }

    #[test]
    fn every_limit_error_is_size_limit_naming_the_limit() {
        let l = DomLimits {
            input_bytes: 5,
            nodes: 5,
            string_bytes: 5,
            serialization_bytes: 5,
            batch_commands: 5,
            batch_bytes: 5,
        };
        for w in [
            LimitName::InputBytes,
            LimitName::Nodes,
            LimitName::StringBytes,
            LimitName::SerializationBytes,
            LimitName::BatchCommands,
            LimitName::BatchBytes,
        ] {
            assert!(l.check(w, 5).is_ok());
            let e = l.check(w, 6).unwrap_err();
            assert_eq!(e.code, Code::SizeLimit);
            assert!(e.message.contains(w.name()), "{}", e.message);
        }
    }

    #[test]
    fn node_limit_blocks_creation_without_partial_effect() {
        let doc = Document::new_empty(ui4me_engine_types::ids::DocumentId::from_raw(
            ui4me_engine_types::ids::RawId {
                index: 99,
                generation: std::num::NonZeroU32::MIN,
            },
        ));
        let mut doc = doc.with_limits(DomLimits {
            nodes: 3,
            ..DomLimits::default()
        });
        // the Document node counts as one live node
        doc.create_element("a").unwrap();
        doc.create_element("b").unwrap();
        let before = doc.revision();
        let e = doc.create_element("c").unwrap_err();
        assert_eq!(e.code, Code::SizeLimit);
        assert!(e.message.contains("dom.nodes"));
        assert_eq!(doc.revision(), before);
    }

    #[cfg(feature = "test-sinks")]
    #[test]
    fn injected_allocation_failure_surfaces_out_of_memory() {
        let mut doc = Document::parse_html("<p>x</p>").unwrap();
        doc.inject_allocation_failure(1);
        let e = doc.create_element("a").unwrap_err();
        assert_eq!(e.code, Code::OutOfMemory);
        assert!(doc.create_element("a").is_ok(), "the injection fires once");
    }
}
