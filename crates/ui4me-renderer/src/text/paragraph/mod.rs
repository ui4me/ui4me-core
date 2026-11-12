//! owner: 009-text-inline — created by 001 slice (R-28)

pub(crate) mod decoration;
pub mod frozen;
pub(crate) mod hyphen;
pub mod limits;
pub mod offset_map;
pub mod scratch;
pub mod shaped;
pub(crate) mod snap;
pub mod source;
pub mod synthetic;
pub mod vertical;
pub use hyphen::HyphenUnsupported;
pub use vertical::GlyphOrientation;

pub use frozen::{
    DecorationKind, FreezeExtras, FreezeParams, FrozenParagraph, GlyphRunView, TextAlignment,
    TextPiece,
};
pub use scratch::{AtomicBox, BreakEvent, LineEnd, LineParams, ScratchLayout, scratch};
pub use source::{
    AtomicContribution, DecorationLayer, DecorationLineStyle, DecorationThickness, HyphensKind,
    InlineItemKind, LineBreakKind, LineHeightSpec, ParagraphSource, ShapedParagraph, SpanCollapse,
    SpanProps, SynthKind, SynthRequest, SynthRun, TabSpec, TextStyle, UnderlinePos,
    VerticalAlignSpec, WordBreakKind, WrapOverflow,
};
