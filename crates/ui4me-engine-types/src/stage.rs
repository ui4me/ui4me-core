//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Stage attribution (core-types.md §5). Numeric encoding: 004 error-mapping §2, also emitted
//! by `cargo xtask gen-codes` as `codes_generated::STAGE_ENCODING` (checked in tests).

/// The ten constitution stages plus `resource` and `host`; `None` = 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Stage {
    None = 0,
    Parse = 1,
    Cascade = 2,
    Box = 3,
    Text = 4,
    Layout = 5,
    Scene = 6,
    Raster = 7,
    Event = 8,
    Platform = 9,
    Accessibility = 10,
    Resource = 11,
    Host = 12,
}

impl Stage {
    pub const ALL: &'static [Stage] = &[
        Stage::None,
        Stage::Parse,
        Stage::Cascade,
        Stage::Box,
        Stage::Text,
        Stage::Layout,
        Stage::Scene,
        Stage::Raster,
        Stage::Event,
        Stage::Platform,
        Stage::Accessibility,
        Stage::Resource,
        Stage::Host,
    ];

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Stage::None => "none",
            Stage::Parse => "parse",
            Stage::Cascade => "cascade",
            Stage::Box => "box",
            Stage::Text => "text",
            Stage::Layout => "layout",
            Stage::Scene => "scene",
            Stage::Raster => "raster",
            Stage::Event => "event",
            Stage::Platform => "platform",
            Stage::Accessibility => "accessibility",
            Stage::Resource => "resource",
            Stage::Host => "host",
        }
    }

    pub const fn from_code(c: u8) -> Option<Stage> {
        if (c as usize) < Stage::ALL.len() {
            Some(Stage::ALL[c as usize])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_generated_stage_encoding() {
        let generated = crate::codes_generated::STAGE_ENCODING;
        assert_eq!(generated.len(), super::Stage::ALL.len());
        for (&(c, n), s) in generated.iter().zip(super::Stage::ALL) {
            assert_eq!((c, n), (s.code(), s.name()));
        }
    }
}
