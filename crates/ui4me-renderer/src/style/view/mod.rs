//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! `ComputedStyleView` (006 `computed-style-view.md`; AD-12; T043): the typed, immutable style
//! output consumed by 008 (box tree, layout), 009 (inline text), 010 (scene). Not public across
//! the C ABI; no engine type appears in it.
//!
//! A [`StyleSnapshot`] is published per style epoch (immutable; `Send + Sync`). A view borrows
//! the snapshot: `snapshot.view(node)` is `None` when the node is not styled in this snapshot
//! (not an element, disconnected, or inside a `display: none` subtree).

pub mod calc;
pub mod color;
pub mod generated;
pub mod groups;
pub mod layout_groups;
pub mod paint_groups;
pub mod scroll_props;

use groups::*;
use paint_groups::overflow;
use std::collections::HashMap;
use std::sync::Arc;
use style::properties::{ComputedValues, PropertyDeclarationId};
use style::servo_arc::Arc as StyloArc;
use ui4me_engine_types::ids::{DocumentId, NodeId};

/// Pseudo-element kinds (contract §1). The slice produces `Before`/`After` (eager pseudos).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    Before,
    After,
    Marker,
    Placeholder,
    Selection,
    Backdrop,
    DetailsContent,
    SliderThumb,
    SliderTrack,
    SliderFill,
    /// `::first-letter` of a block container (015 TXT-5).
    FirstLetter,
    /// The style of the element (or of the block's `::first-line`) on the first formatted line of an ancestor block:
    /// what the text and inline boxes on that line inherit from (015 TXT-5, `first_pseudo.rs`).
    FirstLine,
    /// `::before` / `::after` as parts of the first line.
    FirstLineBefore,
    FirstLineAfter,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeStyles {
    pub(crate) primary: StyloArc<ComputedValues>,
    pub(crate) before: Option<StyloArc<ComputedValues>>,
    pub(crate) after: Option<StyloArc<ComputedValues>>,
    /// `::marker` of a `display: list-item` element (lazy pseudo, CSS Lists 3 section 3).
    pub(crate) marker: Option<StyloArc<ComputedValues>>,
    /// `::first-letter` of a block container, cascaded from its first-line style when it has one (015 TXT-5).
    pub(crate) first_letter: Option<StyloArc<ComputedValues>>,
    /// The element on the first formatted line of an ancestor block (or the block's own `::first-line`), and its
    /// generated `::before` / `::after` there (`first_pseudo.rs`).
    pub(crate) first_line: Option<StyloArc<ComputedValues>>,
    pub(crate) first_line_before: Option<StyloArc<ComputedValues>>,
    pub(crate) first_line_after: Option<StyloArc<ComputedValues>>,
    /// `::backdrop` of an element in the top layer (lazy pseudo, CSS Position 4 section 3.1; 015 HTML-1).
    pub(crate) backdrop: Option<StyloArc<ComputedValues>>,
    /// Cascaded `display` of a flex/grid item before blockification (see `DisplayFlow`).
    pub(crate) item_display: Option<style::values::specified::Display>,
}

/// One published style output (epoch E). Immutable; cheap to share (`Arc`).
#[derive(Debug)]
pub struct StyleSnapshot {
    pub(crate) document: DocumentId,
    pub(crate) epoch: u64,
    /// 005's input epochs this output was computed from (XR-008).
    pub(crate) inputs: ui4me_engine_types::revision::RevisionInputs,
    pub(crate) styles: HashMap<NodeId, NodeStyles>,
    pub(crate) initial: StyloArc<ComputedValues>,
    /// The environment's `prefers-color-scheme` was dark when this output was computed.
    pub(crate) prefers_dark: bool,
}

impl StyleSnapshot {
    pub(crate) fn empty(
        document: DocumentId,
        initial: StyloArc<ComputedValues>,
    ) -> Arc<StyleSnapshot> {
        Arc::new(StyleSnapshot {
            document,
            epoch: 0,
            inputs: Default::default(),
            styles: HashMap::new(),
            initial,
            prefers_dark: false,
        })
    }

    pub fn document(&self) -> DocumentId {
        self.document
    }

    /// A snapshot of a synthetic document (015 PAGE-1: the page-margin boxes of one page, `crate::style::page`): each
    /// element's primary style is given; no pseudo-elements.
    pub(crate) fn synthetic(
        document: DocumentId,
        entries: Vec<(NodeId, StyloArc<ComputedValues>)>,
        initial: StyloArc<ComputedValues>,
    ) -> Arc<StyleSnapshot> {
        let styles = entries
            .into_iter()
            .map(|(n, primary)| {
                (
                    n,
                    NodeStyles {
                        primary,
                        before: None,
                        after: None,
                        marker: None,
                        first_letter: None,
                        first_line: None,
                        first_line_before: None,
                        first_line_after: None,
                        backdrop: None,
                        item_display: None,
                    },
                )
            })
            .collect();
        Arc::new(StyleSnapshot {
            document,
            epoch: 1,
            inputs: Default::default(),
            styles,
            initial,
            prefers_dark: false,
        })
    }

    /// The style output epoch (005's `style` output epoch is assigned at publication).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The input epochs (dom, environment, font, ...) this output was computed from.
    pub fn revision_inputs(&self) -> ui4me_engine_types::revision::RevisionInputs {
        self.inputs
    }

    /// 005's revision tuple for this output: the inputs it was computed from and the style
    /// output epoch. `layout` and `scene` are assigned by the scheduler at publication (005),
    /// so they are 0 here.
    pub fn revision_tuple(&self) -> ui4me_engine_types::revision::RevisionTuple {
        ui4me_engine_types::revision::RevisionTuple {
            document: self.document,
            inputs: self.inputs,
            style: self.epoch,
            layout: 0,
            scene: 0,
        }
    }

    /// The environment's `prefers-color-scheme` is dark (the preference the media feature and the
    /// `light dark` value of `color-scheme` follow).
    pub fn prefers_dark(&self) -> bool {
        self.prefers_dark
    }

    /// Whether the element's used colour scheme is dark (CSS Color Adjust 1: `dark` alone is dark,
    /// `light` alone is light, both follow the environment, and `normal` - not aware of colour
    /// schemes - uses the user agent's light default, as the reference browser does; the same rule
    /// as the system colours, Stylo patch P-006-20); `None` when the element is not styled. The root
    /// element's used scheme decides the canvas default (010 FR-023).
    pub fn used_color_scheme_dark(&self, node: NodeId) -> Option<bool> {
        let s = self.styles.get(&node)?;
        use style::values::specified::color::ColorSchemeFlags as F;
        let bits = s.primary.get_inherited_ui().color_scheme_bits();
        let (dark, light) = (bits.contains(F::DARK), bits.contains(F::LIGHT));
        Some(if dark != light {
            dark
        } else {
            dark && self.prefers_dark
        })
    }

    /// Number of styled elements.
    pub fn len(&self) -> usize {
        self.styles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// The element's primary style, if it is styled in this snapshot.
    pub fn view(&self, node: NodeId) -> Option<ComputedStyleView<'_>> {
        self.view_pseudo(node, None)
    }

    /// The element's style or one of its generated pseudo-elements' (contract §1).
    pub fn view_pseudo(
        &self,
        node: NodeId,
        pseudo: Option<PseudoKind>,
    ) -> Option<ComputedStyleView<'_>> {
        let s = self.styles.get(&node)?;
        let values = match pseudo {
            None => &s.primary,
            Some(PseudoKind::Before) => s.before.as_ref()?,
            Some(PseudoKind::After) => s.after.as_ref()?,
            Some(PseudoKind::Marker) => s.marker.as_ref()?,
            Some(PseudoKind::FirstLetter) => s.first_letter.as_ref()?,
            Some(PseudoKind::FirstLine) => s.first_line.as_ref()?,
            Some(PseudoKind::FirstLineBefore) => s.first_line_before.as_ref()?,
            Some(PseudoKind::FirstLineAfter) => s.first_line_after.as_ref()?,
            Some(PseudoKind::Backdrop) => s.backdrop.as_ref()?,
            Some(_) => return None,
        };
        let item_display = if pseudo.is_none() {
            s.item_display
        } else {
            None
        };
        Some(ComputedStyleView {
            snapshot: self,
            node,
            pseudo,
            values,
            item_display,
        })
    }

    /// Styled element ids (unordered).
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.styles.keys().copied()
    }

    /// What identifies `node`'s computed values in this snapshot: two snapshots that give a node
    /// equal identities give it the same values (the style system shares the value objects of an
    /// element it did not restyle), so anything derived from the values alone may be reused.
    pub fn identity(&self, node: NodeId) -> Option<StyleIdentity> {
        let s = self.styles.get(&node)?;
        Some(StyleIdentity {
            primary: s.primary.clone(),
            before: s.before.clone(),
            after: s.after.clone(),
            marker: s.marker.clone(),
            first_letter: s.first_letter.clone(),
            first_line: s.first_line.clone(),
            first_line_before: s.first_line_before.clone(),
            first_line_after: s.first_line_after.clone(),
            backdrop: s.backdrop.clone(),
            item_display: s.item_display,
        })
    }
}

/// See [`StyleSnapshot::identity`]. Holds the value objects, so a pointer cannot be reused while it is kept.
#[derive(Clone)]
pub struct StyleIdentity {
    primary: StyloArc<ComputedValues>,
    before: Option<StyloArc<ComputedValues>>,
    after: Option<StyloArc<ComputedValues>>,
    marker: Option<StyloArc<ComputedValues>>,
    first_letter: Option<StyloArc<ComputedValues>>,
    first_line: Option<StyloArc<ComputedValues>>,
    first_line_before: Option<StyloArc<ComputedValues>>,
    first_line_after: Option<StyloArc<ComputedValues>>,
    backdrop: Option<StyloArc<ComputedValues>>,
    item_display: Option<style::values::specified::Display>,
}

impl PartialEq for StyleIdentity {
    fn eq(&self, other: &Self) -> bool {
        let same =
            |a: &Option<StyloArc<ComputedValues>>, b: &Option<StyloArc<ComputedValues>>| match (
                a, b,
            ) {
                (Some(a), Some(b)) => StyloArc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
        StyloArc::ptr_eq(&self.primary, &other.primary)
            && same(&self.before, &other.before)
            && same(&self.after, &other.after)
            && same(&self.marker, &other.marker)
            && same(&self.first_letter, &other.first_letter)
            && same(&self.first_line, &other.first_line)
            && same(&self.first_line_before, &other.first_line_before)
            && same(&self.first_line_after, &other.first_line_after)
            && same(&self.backdrop, &other.backdrop)
            && self.item_display == other.item_display
    }
}

impl Eq for StyleIdentity {}

/// Hashes the value objects' addresses (equal identities share them), so elements whose computed
/// values are shared (Stylo's style sharing) can share what is derived from them (L-PERF-b3).
impl std::hash::Hash for StyleIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let p = |a: &Option<StyloArc<ComputedValues>>| {
            a.as_ref().map_or(0usize, |a| a.raw_ptr().as_ptr() as usize)
        };
        (self.primary.raw_ptr().as_ptr() as usize).hash(state);
        p(&self.before).hash(state);
        p(&self.after).hash(state);
        p(&self.marker).hash(state);
        p(&self.first_letter).hash(state);
        p(&self.first_line).hash(state);
        p(&self.first_line_before).hash(state);
        p(&self.first_line_after).hash(state);
        p(&self.backdrop).hash(state);
    }
}

/// A typed view of one element's (or pseudo-element's) computed values in one snapshot.
#[derive(Clone, Copy)]
pub struct ComputedStyleView<'s> {
    snapshot: &'s StyleSnapshot,
    node: NodeId,
    pseudo: Option<PseudoKind>,
    values: &'s StyloArc<ComputedValues>,
    item_display: Option<style::values::specified::Display>,
}

impl std::fmt::Debug for ComputedStyleView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputedStyleView")
            .field("node", &self.node)
            .field("pseudo", &self.pseudo)
            .field("epoch", &self.snapshot.epoch)
            .finish()
    }
}

impl<'s> ComputedStyleView<'s> {
    pub fn epoch(&self) -> u64 {
        self.snapshot.epoch
    }
    pub fn node(&self) -> NodeId {
        self.node
    }
    pub fn pseudo(&self) -> Option<PseudoKind> {
        self.pseudo
    }

    pub fn flags(&self) -> ViewFlags {
        use style::computed_value_flags::ComputedValueFlags as F;
        let f = self.values.flags;
        ViewFlags {
            depends_on_font_metrics: f
                .intersects(F::DEPENDS_ON_SELF_FONT_METRICS | F::DEPENDS_ON_INHERITED_FONT_METRICS),
            uses_viewport_units: f.intersects(F::USES_VIEWPORT_UNITS),
            is_animating: false,
            has_generated_content: self
                .snapshot
                .styles
                .get(&self.node)
                .is_some_and(|s| s.before.is_some() || s.after.is_some() || s.marker.is_some()),
        }
    }

    pub fn display_flow(&self) -> DisplayFlow {
        display_flow(self.values, self.item_display)
    }
    pub fn sizing(&self) -> Sizing {
        sizing(self.epoch(), self.values)
    }
    pub fn box_model(&self) -> BoxModel {
        box_model(self.epoch(), self.values)
    }
    pub fn position_offsets(&self) -> PositionOffsets {
        position_offsets(self.epoch(), self.values)
    }
    pub fn font(&self) -> FontGroup {
        font(self.values)
    }
    /// `text-wrap-style` and `line-clamp` (015 TXT-4).
    pub fn line_flow(&self) -> LineFlow {
        line_flow(self.values)
    }
    /// Variation settings, optical sizing and the OpenType features asked for (015 TXT-2).
    pub fn font_open_type(&self) -> FontOpenType {
        font_open_type(self.values)
    }
    pub fn inline_text(&self) -> InlineText {
        inline_text(self.epoch(), self.values)
    }
    pub fn text_decoration(&self) -> TextDecoration {
        text_decoration(self.epoch(), self.values)
    }
    pub fn paint(&self) -> Paint {
        paint(self.epoch(), self.values)
    }
    pub fn overflow(&self) -> Overflow {
        overflow(self.values)
    }
    /// Scroll behavior, overscroll, snap, scroll-padding and scroll-margin (0.8 F0803/F0804).
    pub fn scroll_props(&self) -> scroll_props::ScrollProps {
        scroll_props::scroll_props(self.values)
    }

    /// Computed anchor-size expressions, including ones introduced through var().
    /// They are explicitly reported until anchor size dependency resolution exists.
    pub fn uses_anchor_size(&self) -> bool {
        use style::values::generics::length::{GenericMargin, GenericMaxSize, GenericSize};
        let p = self.values.get_position();
        let m = self.values.get_margin();
        [&p.width, &p.height, &p.min_width, &p.min_height]
            .into_iter()
            .any(|s| {
                matches!(
                    s,
                    GenericSize::AnchorSizeFunction(_)
                        | GenericSize::AnchorContainingCalcFunction(_)
                )
            })
            || [&p.max_width, &p.max_height].into_iter().any(|s| {
                matches!(
                    s,
                    GenericMaxSize::AnchorSizeFunction(_)
                        | GenericMaxSize::AnchorContainingCalcFunction(_)
                )
            })
            || [
                &m.margin_top,
                &m.margin_right,
                &m.margin_bottom,
                &m.margin_left,
            ]
            .into_iter()
            .any(|s| {
                matches!(
                    s,
                    GenericMargin::AnchorSizeFunction(_)
                        | GenericMargin::AnchorContainingCalcFunction(_)
                )
            })
    }

    /// Layout consumes typed computed facts. CSSOM serialization remains for
    /// explicit style queries and diagnostics.
    pub fn claims_fixed_descendants(&self) -> bool {
        use style::values::generics::box_::GenericPerspective;
        use style::values::generics::transform::{GenericRotate, GenericScale, GenericTranslate};
        use style::values::specified::box_::WillChangeBits;
        let b = self.values.get_box();
        !b.transform.0.is_empty()
            || !matches!(b.translate, GenericTranslate::None)
            || !matches!(b.rotate, GenericRotate::None)
            || !matches!(b.scale, GenericScale::None)
            || !matches!(b.perspective, GenericPerspective::None)
            || !self.values.get_effects().filter.0.is_empty()
            || b.will_change.bits.intersects(
                WillChangeBits::TRANSFORM
                    | WillChangeBits::PERSPECTIVE
                    | WillChangeBits::CONTAIN
                    | WillChangeBits::FIXPOS_CB_NON_SVG,
            )
    }

    pub fn native_appearance(&self) -> bool {
        self.values.get_box().appearance != style::values::specified::Appearance::None
    }
    /// Flex container/item longhands and `order` (CSS Flexbox 1; 006:T038).
    pub fn flex(&self) -> layout_groups::Flex {
        layout_groups::flex(self.epoch(), self.values)
    }
    /// The box-alignment longhands and the gaps (CSS Box Alignment 3; 006:T038).
    pub fn alignment(&self) -> layout_groups::Alignment {
        layout_groups::alignment(self.epoch(), self.values)
    }
    /// Grid container and item longhands: tracks with line names, areas, auto tracks, flow and
    /// placement (CSS Grid 2; 006:T038).
    pub fn grid(&self) -> layout_groups::Grid {
        layout_groups::grid(self.epoch(), self.values)
    }
    /// `contain`, `container-type` and `container-name`: the containment in effect (CSS Containment 2
    /// and the containment `container-type` implies, CSS Conditional 5; 015 CQ-2).
    pub fn containment(&self) -> layout_groups::Containment {
        layout_groups::containment(self.values)
    }
    /// The multicolumn and fragmentation longhands (CSS Multicol 1, CSS Break 3; 015 FRAG-2a).
    pub fn fragmentation(&self) -> layout_groups::Fragmentation {
        layout_groups::fragmentation(self.values)
    }
    /// `content`, counters, `quotes` and the list properties (CSS Lists 3, CSS Content 3).
    pub fn generated(&self) -> generated::Generated {
        generated::generated(self.epoch(), self.values)
    }

    /// A custom property's computed value as its token-stream text (unregistered properties).
    pub fn custom(&self, name: &str) -> Option<String> {
        let n = name.strip_prefix("--")?;
        let atom = style::Atom::from(n);
        let v = self
            .values
            .computed_value_to_string(PropertyDeclarationId::Custom(&atom));
        (!v.is_empty()).then_some(v)
    }

    /// The names (`--x`, sorted) of every custom property with a computed value on this element:
    /// declared or inherited ones and registered (`@property`) ones with their initial value.
    /// The reserved `--ui4me-` prefix (engine-internal carriers) is not listed.
    pub fn custom_property_names(&self) -> Vec<String> {
        let c = self.values.custom_properties();
        let mut names: Vec<String> = c
            .inherited
            .iter()
            .chain(c.non_inherited.iter())
            .map(|(k, _)| format!("--{k}"))
            .filter(|n| !n.starts_with("--ui4me-"))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Longhands whose computed value differs from the property's initial value, by CSS
    /// name, in a stable order. Consumers use this to report out-of-subset values (001
    /// FR-021/FR-022) without the view exposing every property as a typed field.
    pub fn non_initial_properties(&self) -> Vec<&'static str> {
        crate::style::computed::all_longhands()
            .iter()
            .copied()
            .filter(|&id| {
                let p = PropertyDeclarationId::Longhand(id);
                self.values.computed_value_to_string(p)
                    != self.snapshot.initial.computed_value_to_string(p)
            })
            .map(|id| id.name())
            .collect()
    }

    /// Whether one computed longhand equals the property's initial value (`true` for a name the
    /// engine does not know). Cheap enough for a fixed list of properties per element.
    pub fn is_initial(&self, longhand: &str) -> bool {
        let Some(id) = crate::style::computed::longhand_by_name(longhand) else {
            return true;
        };
        let p = PropertyDeclarationId::Longhand(id);
        self.values.computed_value_to_string(p) == self.snapshot.initial.computed_value_to_string(p)
    }

    /// Whether `longhand` (a margin) is a quirks-mode margin that layout ignores while the CSSOM
    /// value keeps the UA length (see `computed::quirk_margin`): the used margin of the box is not
    /// the value to report.
    pub fn is_quirk_ignored_margin(&self, longhand: &str) -> bool {
        crate::style::computed::longhand_by_name(longhand)
            .is_some_and(|id| crate::style::computed::quirk_margin(self.values, id).is_some())
    }

    /// The CSSOM serialisation of one computed longhand (diagnostics, tests).
    pub fn serialize(&self, longhand: &str) -> Option<String> {
        if longhand.starts_with("--") {
            return self.custom(longhand);
        }
        let id = crate::style::computed::longhand_by_name(longhand)?;
        if let Some(q) = crate::style::computed::quirk_margin(self.values, id) {
            return Some(q);
        }
        Some(crate::style::computed::legacy_keyword(
            id,
            self.values
                .computed_value_to_string(PropertyDeclarationId::Longhand(id)),
        ))
    }
}
