//! owner: 007-resources (T063, T064) — per-document `@font-face` faces.
//!
//! Faces are scoped per document (IF-04) while the font epoch is runtime-wide. Descriptor ranges
//! and `unicode-range` are parsed (by 006) and stored here **unchanged** for 009's matching (FR-045,
//! R-54): a single value is a degenerate range, an absent `unicode-range` is U+0-10FFFF. `src` is
//! an ordered list of `url()` sources and `local()` names tried in order (FR-048); `format()` hints
//! naming EOT, SVG or unknown fonts are skipped without a read (FR-046). `font-display` behaves
//! as `swap` and is reported once per face with `info` severity (FR-047).

use super::registry::FontBlob;
use ui4me_engine_types::codes_generated::Code;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceStyle {
    Normal,
    Italic,
    /// `oblique <min> <max>` in degrees (a single angle is a degenerate range).
    Oblique {
        min_deg: f32,
        max_deg: f32,
    },
}

/// One `src` entry as 006 hands it over.
#[derive(Clone, Debug, PartialEq)]
pub enum FaceSourceSpec {
    /// `url(reference) [format(hint)]`; `base` is the declaring sheet's base URL.
    Url {
        reference: String,
        base: Url,
        format: Option<String>,
    },
    Local(String),
}

/// Defaults belong to the selected face, never to every font in an element's fallback list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceSettings {
    pub features: Vec<([u8; 4], u32)>,
    pub variations: Vec<([u8; 4], f32)>,
}

impl FaceSettings {
    pub fn is_empty(&self) -> bool {
        self.features.is_empty() && self.variations.is_empty()
    }
}

/// An effective `@font-face` rule of one document (06's `FontFaceRule`).
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaceRule {
    /// Stable identity of the rule in the cascade; unchanged rules keep their face and state.
    pub rule_id: u64,
    pub family: String,
    pub settings: FaceSettings,
    pub weight: (f32, f32),
    pub style: FaceStyle,
    pub width: (f32, f32),
    /// Code-point ranges; empty = absent = U+0-10FFFF.
    pub unicode_range: Vec<(u32, u32)>,
    pub sources: Vec<FaceSourceSpec>,
    /// The `font-display` value as written, if any (reported once, behaves as `swap`).
    pub display: Option<String>,
}

pub enum FaceState {
    Unloaded,
    Loading {
        source_index: usize,
    },
    /// The validated blob (descriptor overrides: the rule's family).
    Loaded(FontBlob),
    /// A `local()` source that names an installed (system) font family: 009 maps the declared
    /// family to it (CSS Fonts 4 section 4.3.2; matching is by family name, see `lib.rs`).
    LocalFamily(String),
    /// Every source failed; one code per source, in order.
    Failed(Vec<Code>),
}

impl std::fmt::Debug for FaceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaceState::Unloaded => write!(f, "Unloaded"),
            FaceState::Loading { source_index } => write!(f, "Loading({source_index})"),
            FaceState::Loaded(b) => write!(f, "Loaded({})", b.name),
            FaceState::LocalFamily(n) => write!(f, "LocalFamily({n})"),
            FaceState::Failed(c) => write!(f, "Failed({c:?})"),
        }
    }
}

impl Clone for FaceState {
    fn clone(&self) -> Self {
        match self {
            FaceState::Unloaded => FaceState::Unloaded,
            FaceState::Loading { source_index } => FaceState::Loading {
                source_index: *source_index,
            },
            FaceState::Loaded(b) => FaceState::Loaded(b.clone()),
            FaceState::LocalFamily(n) => FaceState::LocalFamily(n.clone()),
            FaceState::Failed(c) => FaceState::Failed(c.clone()),
        }
    }
}

impl FaceState {
    pub fn name(&self) -> &'static str {
        match self {
            FaceState::Unloaded => "Unloaded",
            FaceState::Loading { .. } => "Loading",
            FaceState::Loaded(_) => "Loaded",
            FaceState::LocalFamily(_) => "LocalFamily",
            FaceState::Failed(_) => "Failed",
        }
    }
}

/// What 009 sees of a face (`resource-view.md` §5 `FaceView`).
#[derive(Clone, Debug)]
pub struct FaceView {
    pub id: FaceId,
    pub family: String,
    pub settings: FaceSettings,
    pub weight: (f32, f32),
    pub style: FaceStyle,
    pub width: (f32, f32),
    pub unicode_range: Vec<(u32, u32)>,
    pub state: FaceState,
    /// Informational and V1-LIMIT notes (e.g. `font.display`).
    pub reports: Vec<&'static str>,
}

#[derive(Debug)]
pub struct Face {
    pub id: FaceId,
    pub rule: FontFaceRule,
    pub state: FaceState,
    /// One code per source that failed so far.
    pub failures: Vec<Code>,
    pub requested: bool,
    pub reports: Vec<&'static str>,
}

impl Face {
    pub fn view(&self) -> FaceView {
        FaceView {
            id: self.id,
            family: self.rule.family.clone(),
            settings: self.rule.settings.clone(),
            weight: self.rule.weight,
            style: self.rule.style,
            width: self.rule.width,
            unicode_range: if self.rule.unicode_range.is_empty() {
                vec![(0, 0x10FFFF)]
            } else {
                self.rule.unicode_range.clone()
            },
            state: self.state.clone(),
            reports: self.reports.clone(),
        }
    }
}

/// The faces of one document.
#[derive(Debug, Default)]
pub struct FaceSet {
    faces: Vec<Face>,
}

impl FaceSet {
    /// Register the document's effective rules (called at every style update). A rule that is
    /// unchanged since the last sync keeps its face, id and state; a changed or new rule gets a new
    /// face; rules that left the cascade disappear. Returns the ids in rule order.
    pub fn sync(&mut self, rules: &[FontFaceRule], next_id: &mut u64) -> Vec<FaceId> {
        let mut old = std::mem::take(&mut self.faces);
        let mut ids = Vec::with_capacity(rules.len());
        for rule in rules {
            let pos = old.iter().position(|f| f.rule == *rule);
            let face = match pos {
                Some(i) => old.remove(i),
                None => {
                    let id = FaceId(*next_id);
                    *next_id += 1;
                    let mut reports = Vec::new();
                    if rule.display.is_some() {
                        // font-display behaves as `swap` for every value; info severity
                        reports.push("font.display");
                    }
                    Face {
                        id,
                        rule: rule.clone(),
                        state: FaceState::Unloaded,
                        failures: Vec::new(),
                        requested: false,
                        reports,
                    }
                }
            };
            ids.push(face.id);
            self.faces.push(face);
        }
        ids
    }

    /// A bundle's font bytes may change without any change to the CSS
    /// `@font-face` declarations. Retire the old loaded blob and request state
    /// so the unchanged descriptor can be loaded from its new revision.
    pub fn invalidate_sources_under(&mut self, prefix: &str) -> Vec<FaceId> {
        let mut changed = Vec::new();
        for face in &mut self.faces {
            let matches = face.rule.sources.iter().any(|source| match source {
                FaceSourceSpec::Url {
                    reference, base, ..
                } => {
                    reference.starts_with(prefix)
                        || base
                            .join(reference)
                            .ok()
                            .is_some_and(|u| u.as_str().starts_with(prefix))
                }
                FaceSourceSpec::Local(_) => false,
            });
            if matches {
                face.state = FaceState::Unloaded;
                face.failures.clear();
                face.requested = false;
                changed.push(face.id);
            }
        }
        changed
    }

    pub fn get(&self, id: FaceId) -> Option<&Face> {
        self.faces.iter().find(|f| f.id == id)
    }

    pub fn get_mut(&mut self, id: FaceId) -> Option<&mut Face> {
        self.faces.iter_mut().find(|f| f.id == id)
    }

    pub fn views(&self) -> Vec<FaceView> {
        self.faces.iter().map(Face::view).collect()
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}
