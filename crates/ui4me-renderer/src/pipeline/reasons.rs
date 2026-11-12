//! owner: 005-pipeline-diagnostics — T015. The invalidation reason registry
//! (`contracts/invalidation-reasons.md` §1–§2; U4C-INV): 21 stable codes, their subjects, their
//! minimum stages, the v1 widening rule and the style-change classification mapping.
//! Adding a reason is an additive (PATCH) schema change; changing a reason's minimum stages is a
//! reviewed change backed by oracle evidence.

use ui4me_engine_types::ids::NodeId;

/// The stages a reason can dirty (invalidation-reasons "Stage abbreviations").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// `S`: style.
    Style,
    /// `B`: box tree.
    Box,
    /// `L`: layout and fragments.
    Layout,
    /// `P`: scene and hit index.
    Scene,
    /// `R`: raster (frame only).
    Raster,
    /// `A`: accessibility at COMMIT (013).
    Access,
}

impl Stage {
    pub const ALL: [Stage; 6] = [
        Stage::Style,
        Stage::Box,
        Stage::Layout,
        Stage::Scene,
        Stage::Raster,
        Stage::Access,
    ];

    const fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// The schema's stage name where the probe exports a stage (`cascade`, `box`, `layout`,
    /// `scene`, `raster`, `a11y`).
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Style => "cascade",
            Stage::Box => "box",
            Stage::Layout => "layout",
            Stage::Scene => "scene",
            Stage::Raster => "raster",
            Stage::Access => "a11y",
        }
    }
}

/// A set of [`Stage`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StageSet(u8);

impl StageSet {
    pub const EMPTY: StageSet = StageSet(0);

    pub const fn with(self, s: Stage) -> StageSet {
        StageSet(self.0 | s.bit())
    }
    pub const fn union(self, o: StageSet) -> StageSet {
        StageSet(self.0 | o.0)
    }
    pub const fn contains(self, s: Stage) -> bool {
        self.0 & s.bit() != 0
    }
    pub const fn contains_all(self, o: StageSet) -> bool {
        self.0 & o.0 == o.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn iter(self) -> impl Iterator<Item = Stage> {
        Stage::ALL.into_iter().filter(move |s| self.contains(*s))
    }
}

/// The 21 reason codes (invalidation-reasons §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    DomChildList,
    DomText,
    DomAttrClass,
    DomAttrId,
    DomAttrStyle,
    DomAttrOther,
    ElementState,
    ControlState,
    Stylesheet,
    EnvironmentViewport,
    EnvironmentScale,
    EnvironmentMedia,
    FontEpoch,
    ResourceEpoch,
    Scroll,
    CaretSelectionPaint,
    AnimationTick,
    AnimationCancel,
    HasRelative,
    ContainerFacts,
    ColdRebuild,
}

/// What a reason's subject is (the "Subject" column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubjectKind {
    /// Parent, inserted/removed roots and adjacent siblings.
    ParentAndRoots,
    TextNode,
    Element,
    ElementAndAttribute,
    ControlElement,
    SheetId,
    WindowOrDocument,
    Document,
    FontFamilySet,
    ResourceAndReferrers,
    ScrollContainer,
    ControlOrTextNode,
    AnimatingElement,
    AnchorElements,
    Container,
}

/// Document facts that make v1 widening larger (invalidation-reasons §1, `ENVIRONMENT_SCALE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Widening {
    /// Any resolution media query exists in the document.
    pub resolution_media: bool,
}

const fn set(stages: &[Stage]) -> StageSet {
    let mut s = StageSet::EMPTY;
    let mut i = 0;
    while i < stages.len() {
        s = s.with(stages[i]);
        i += 1;
    }
    s
}

const S: Stage = Stage::Style;
const B: Stage = Stage::Box;
const L: Stage = Stage::Layout;
const P: Stage = Stage::Scene;
const R: Stage = Stage::Raster;
const A: Stage = Stage::Access;

impl Reason {
    pub const ALL: [Reason; 21] = [
        Reason::DomChildList,
        Reason::DomText,
        Reason::DomAttrClass,
        Reason::DomAttrId,
        Reason::DomAttrStyle,
        Reason::DomAttrOther,
        Reason::ElementState,
        Reason::ControlState,
        Reason::Stylesheet,
        Reason::EnvironmentViewport,
        Reason::EnvironmentScale,
        Reason::EnvironmentMedia,
        Reason::FontEpoch,
        Reason::ResourceEpoch,
        Reason::Scroll,
        Reason::CaretSelectionPaint,
        Reason::AnimationTick,
        Reason::AnimationCancel,
        Reason::HasRelative,
        Reason::ContainerFacts,
        Reason::ColdRebuild,
    ];

    /// The stable string id used in the `invalidation` probe and oracle reports.
    pub const fn code(self) -> &'static str {
        match self {
            Reason::DomChildList => "DOM_CHILD_LIST",
            Reason::DomText => "DOM_TEXT",
            Reason::DomAttrClass => "DOM_ATTR_CLASS",
            Reason::DomAttrId => "DOM_ATTR_ID",
            Reason::DomAttrStyle => "DOM_ATTR_STYLE",
            Reason::DomAttrOther => "DOM_ATTR_OTHER",
            Reason::ElementState => "ELEMENT_STATE",
            Reason::ControlState => "CONTROL_STATE",
            Reason::Stylesheet => "STYLESHEET",
            Reason::EnvironmentViewport => "ENVIRONMENT_VIEWPORT",
            Reason::EnvironmentScale => "ENVIRONMENT_SCALE",
            Reason::EnvironmentMedia => "ENVIRONMENT_MEDIA",
            Reason::FontEpoch => "FONT_EPOCH",
            Reason::ResourceEpoch => "RESOURCE_EPOCH",
            Reason::Scroll => "SCROLL",
            Reason::CaretSelectionPaint => "CARET_SELECTION_PAINT",
            Reason::AnimationTick => "ANIMATION_TICK",
            Reason::AnimationCancel => "ANIMATION_CANCEL",
            Reason::HasRelative => "HAS_RELATIVE",
            Reason::ContainerFacts => "CONTAINER_FACTS",
            Reason::ColdRebuild => "COLD_REBUILD",
        }
    }

    pub fn from_code(code: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.code() == code)
    }

    pub const fn subject_kind(self) -> SubjectKind {
        match self {
            Reason::DomChildList => SubjectKind::ParentAndRoots,
            Reason::DomText => SubjectKind::TextNode,
            Reason::DomAttrClass
            | Reason::DomAttrId
            | Reason::DomAttrStyle
            | Reason::ElementState
            | Reason::AnimationCancel => SubjectKind::Element,
            Reason::DomAttrOther => SubjectKind::ElementAndAttribute,
            Reason::ControlState => SubjectKind::ControlElement,
            Reason::Stylesheet => SubjectKind::SheetId,
            Reason::EnvironmentViewport | Reason::EnvironmentScale => SubjectKind::WindowOrDocument,
            Reason::EnvironmentMedia | Reason::ColdRebuild => SubjectKind::Document,
            Reason::FontEpoch => SubjectKind::FontFamilySet,
            Reason::ResourceEpoch => SubjectKind::ResourceAndReferrers,
            Reason::Scroll => SubjectKind::ScrollContainer,
            Reason::CaretSelectionPaint => SubjectKind::ControlOrTextNode,
            Reason::AnimationTick => SubjectKind::AnimatingElement,
            Reason::HasRelative => SubjectKind::AnchorElements,
            Reason::ContainerFacts => SubjectKind::Container,
        }
    }

    /// The "Minimum stages" column: stages always dirtied by the reason.
    pub const fn minimum_stages(self) -> StageSet {
        match self {
            Reason::DomChildList => set(&[S, B, L, P, A]),
            Reason::DomText => set(&[L, P, A]),
            Reason::DomAttrClass
            | Reason::DomAttrId
            | Reason::DomAttrStyle
            | Reason::DomAttrOther
            | Reason::ElementState => set(&[S]),
            Reason::ControlState => set(&[S, P, A]),
            Reason::Stylesheet
            | Reason::EnvironmentMedia
            | Reason::AnimationTick
            | Reason::HasRelative => set(&[S]),
            Reason::EnvironmentViewport => set(&[S, L, P]),
            Reason::EnvironmentScale => set(&[R]),
            Reason::FontEpoch => set(&[S, L, P]),
            Reason::ResourceEpoch => set(&[L, P]),
            Reason::Scroll | Reason::CaretSelectionPaint => set(&[P]),
            Reason::AnimationCancel => StageSet::EMPTY,
            Reason::ContainerFacts | Reason::ColdRebuild => set(&[S, B, L, P]),
        }
    }

    /// The v1 widening rule (the last column) as the stage set it dirties until a scoped
    /// mechanism passes its oracle. Always a superset of [`Reason::minimum_stages`].
    pub const fn widened_stages(self, ctx: Widening) -> StageSet {
        match self {
            // B, L and P for the whole document.
            Reason::DomChildList => set(&[S, B, L, P, A]),
            // Full L and P.
            Reason::DomText => set(&[L, P, A]),
            // Changed element: REBUILD_BOX.
            Reason::DomAttrClass
            | Reason::DomAttrId
            | Reason::DomAttrStyle
            | Reason::DomAttrOther
            | Reason::ElementState
            | Reason::AnimationTick => set(&[S, B, L, P]),
            // L and P for the control's subtree.
            Reason::ControlState => set(&[S, L, P, A]),
            Reason::Stylesheet | Reason::EnvironmentViewport => set(&[S, B, L, P]),
            // Full S if any resolution media query exists; R always.
            Reason::EnvironmentScale => {
                if ctx.resolution_media {
                    set(&[S, R])
                } else {
                    set(&[R])
                }
            }
            Reason::EnvironmentMedia => set(&[S]),
            Reason::FontEpoch => set(&[S, L, P]),
            Reason::ResourceEpoch => set(&[L, P]),
            Reason::Scroll | Reason::CaretSelectionPaint => set(&[P]),
            Reason::AnimationCancel => StageSet::EMPTY,
            // Restyle of the subtrees anchored by relative-selector flags.
            Reason::HasRelative => set(&[S]),
            Reason::ContainerFacts | Reason::ColdRebuild => set(&[S, B, L, P]),
        }
    }

    /// Whether handling this reason with the v1 policy uses a conservative step where a scoped
    /// one exists in principle (`widened: true` in the update record). Bookkeeping-only and cold
    /// rebuilds are not substitutes for anything.
    pub const fn is_widened_in_v1(self) -> bool {
        !matches!(self, Reason::AnimationCancel | Reason::ColdRebuild)
    }
}

/// The element-state flags that `ELEMENT_STATE` covers (R-38: `picker-open` drives `select:open`,
/// `modal` drives `:modal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementStateFlag {
    Focus,
    FocusVisible,
    FocusWithin,
    Hover,
    Active,
    PickerOpen,
    Modal,
}

impl ElementStateFlag {
    pub const ALL: [ElementStateFlag; 7] = [
        ElementStateFlag::Focus,
        ElementStateFlag::FocusVisible,
        ElementStateFlag::FocusWithin,
        ElementStateFlag::Hover,
        ElementStateFlag::Active,
        ElementStateFlag::PickerOpen,
        ElementStateFlag::Modal,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ElementStateFlag::Focus => "focus",
            ElementStateFlag::FocusVisible => "focus-visible",
            ElementStateFlag::FocusWithin => "focus-within",
            ElementStateFlag::Hover => "hover",
            ElementStateFlag::Active => "active",
            ElementStateFlag::PickerOpen => "picker-open",
            ElementStateFlag::Modal => "modal",
        }
    }
}

/// The subject of one recorded input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Subject {
    Document,
    Node(NodeId),
    /// Parent, inserted/removed roots and adjacent siblings.
    Nodes(Vec<NodeId>),
    Attribute {
        node: NodeId,
        name: String,
    },
    Sheet(u32),
    Resource(u64),
    FontFamilies(Vec<String>),
}

/// The style change classification (invalidation-reasons §2; 006 owns the final table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Classification {
    None,
    Repaint,
    RebuildStacking,
    Relayout,
    RebuildBox,
    /// Not classified: treated as `RebuildBox`.
    Unknown,
}

impl Classification {
    /// The later stages the class dirties.
    pub const fn stages(self) -> StageSet {
        match self {
            Classification::None => StageSet::EMPTY,
            Classification::Repaint | Classification::RebuildStacking => set(&[P]),
            Classification::Relayout => set(&[L, P]),
            Classification::RebuildBox | Classification::Unknown => set(&[B, L, P]),
        }
    }

    /// The v1 rule: until G-INVALIDATION evidence covers the classification, every non-`None`
    /// change is `RebuildBox`. Returns the effective class and whether it widened the change.
    pub const fn effective_v1(self) -> (Classification, bool) {
        match self {
            Classification::None => (Classification::None, false),
            Classification::RebuildBox | Classification::Unknown => {
                (Classification::RebuildBox, false)
            }
            Classification::Repaint
            | Classification::RebuildStacking
            | Classification::Relayout => (Classification::RebuildBox, true),
        }
    }
}
