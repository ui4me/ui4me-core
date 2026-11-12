//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Host computed reads (T043; 006 `style-query-api.md` §2.1; FR-015). Values are the engine's
//! CSSOM serialisation of computed values (colours as `rgb()`/`rgba()`). Layout-dependent
//! resolved values (`width`, `height`, margins, paddings, insets) are used values from 008 and
//! are substituted by the pipeline after a layout flush; this module serves the computed part.
//!
//! * unknown name → `INVALID_ARGUMENT`; a name the profile excludes → `UNSUPPORTED_FEATURE` with
//!   sub-reason `PROFILE_EXCLUDED` (reported per name; `style_get` reports it in its per-value
//!   status array); a non-element node → `INVALID_ARGUMENT`/`WRONG_NODE_KIND`;
//! * a disconnected element → `""` (and the `not_connected` flag at the 004 layer);
//! * an element inside a `display: none` subtree → its computed values (resolved on demand,
//!   not stored).
//!
//! Shorthands: the value when every longhand serialises identically, else `""` (slice subset of
//! CSSOM shorthand serialisation; `[VALIDATE X-SPECS]`).

use crate::style::traverse::StyleBridge;
use std::collections::HashMap;
use std::sync::OnceLock;
use style::properties::{
    ComputedValues, LonghandId, PropertyDeclarationId, PropertyId, ShorthandId,
};
use style::servo_arc::Arc as StyloArc;
use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Every longhand reachable from `all`, plus the two `all` excludes (unfiltered), sorted by name. Built once: the
/// list is fixed, and collecting and sorting its ~450 entries on every lookup was the largest single source of
/// allocator churn of a document cycle (3,500 allocations per cycle, A23).
pub(crate) fn all_longhands() -> &'static [LonghandId] {
    static ALL: OnceLock<Vec<LonghandId>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<LonghandId> = ShorthandId::All.longhands().collect();
        v.push(LonghandId::Direction);
        v.push(LonghandId::UnicodeBidi);
        v.sort_by_key(|id| id.name());
        v
    })
}

/// Longhands admitted by the profile (enabled for content), sorted by name.
pub fn profile_longhands() -> Vec<&'static str> {
    crate::style::profile::ensure_applied();
    // The CSSOM (the reference browser) names `font-width` by its legacy name `font-stretch`.
    all_longhands()
        .iter()
        .filter(|id| PropertyId::parse_enabled_for_all_content(id.name()).is_ok())
        .map(|id| {
            if id.name() == "font-width" {
                "font-stretch"
            } else {
                id.name()
            }
        })
        .collect()
}

/// A longhand by CSS name regardless of whether the profile enables it (internal).
pub(crate) fn longhand_by_name(name: &str) -> Option<LonghandId> {
    static BY_NAME: OnceLock<HashMap<String, LonghandId>> = OnceLock::new();
    let map = BY_NAME.get_or_init(|| {
        let mut m: HashMap<String, LonghandId> = all_longhands()
            .iter()
            .map(|id| (id.name().to_ascii_lowercase(), *id))
            .collect();
        // `font-stretch` is the legacy name of `font-width` (CSS Fonts 4); the CSSOM answers to both.
        m.insert("font-stretch".to_string(), LonghandId::FontWidth);
        m
    });
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        map.get(&name.to_ascii_lowercase()).copied()
    } else {
        map.get(name).copied()
    }
}

fn status(code: Code) -> Status {
    Status::new(code, Stage::Cascade)
}

enum Name {
    Longhand(LonghandId),
    Shorthand(ShorthandId),
    Custom(style::Atom),
}

fn parse_name(name: &str) -> Result<Name, Status> {
    crate::style::profile::ensure_applied();
    if let Some(custom) = name.strip_prefix("--") {
        return Ok(Name::Custom(style::Atom::from(custom)));
    }
    match PropertyId::parse_enabled_for_all_content(name) {
        Ok(id) => match id.as_shorthand() {
            Ok(sh) => Ok(Name::Shorthand(sh)),
            Err(PropertyDeclarationId::Longhand(l)) => Ok(Name::Longhand(l)),
            Err(PropertyDeclarationId::Custom(a)) => Ok(Name::Custom(a.clone())),
        },
        Err(()) => {
            match crate::style::profile::table::property_feature(name) {
                Some(f) => Err(status(Code::UnsupportedFeature)
                    .with_sub_reason(SubReason::ProfileExcluded)
                    .with_message(f.as_str())),
                None => Err(status(Code::InvalidArgument)
                    .with_message(format!("unknown property {name:?}"))),
            }
        }
    }
}

/// FR-077 / CSS Backgrounds: the computed width of a border or outline is 0 while its style is
/// `none` or `hidden` (the declared width is kept in the cascade, only the value read is 0).
fn zeroed_by_style(l: LonghandId) -> Option<LonghandId> {
    Some(match l {
        LonghandId::BorderTopWidth => LonghandId::BorderTopStyle,
        LonghandId::BorderRightWidth => LonghandId::BorderRightStyle,
        LonghandId::BorderBottomWidth => LonghandId::BorderBottomStyle,
        LonghandId::BorderLeftWidth => LonghandId::BorderLeftStyle,
        LonghandId::OutlineWidth => LonghandId::OutlineStyle,
        _ => return None,
    })
}

/// CSSOM spelling of the legacy `text-align` keywords: Stylo serialises them with the Gecko name
/// (`-moz-center`); the engine's CSSOM (reference browser, Compat Standard) is `-webkit-*`.
/// The UA margin a quirks-mode margin-collapsing rule zeroed for layout (HTML 15.3.9 "Margin
/// collapsing quirks"), as `Npx`: the reference browser keeps it as the CSSOM value of the margin
/// and ignores it only in layout. The rules (`ua/quirks.css`) record it in the engine-internal
/// `--ui4me-quirk-margin-start|end` carriers; it applies while the computed margin is `0px`
/// (an author margin wins and is reported as is). D-LSTYFIXr7-05.
pub(crate) fn quirk_margin(cv: &ComputedValues, l: LonghandId) -> Option<String> {
    let carrier = match l {
        LonghandId::MarginTop | LonghandId::MarginBlockStart => "ui4me-quirk-margin-start",
        LonghandId::MarginBottom | LonghandId::MarginBlockEnd => "ui4me-quirk-margin-end",
        _ => return None,
    };
    if cv.computed_value_to_string(PropertyDeclarationId::Longhand(l)) != "0px" {
        return None;
    }
    let v = cv.computed_value_to_string(PropertyDeclarationId::Custom(&style::Atom::from(carrier)));
    (!v.is_empty() && v != "0px").then_some(v)
}

pub(crate) fn legacy_keyword(l: LonghandId, s: String) -> String {
    if l == LonghandId::TextAlign {
        match s.as_str() {
            "-moz-center" => return "-webkit-center".to_owned(),
            "-moz-left" => return "-webkit-left".to_owned(),
            "-moz-right" => return "-webkit-right".to_owned(),
            _ => {}
        }
    }
    if l == LonghandId::Contain && s.contains(' ') {
        // The reference browser lists the size keyword first (`inline-size layout style paint`);
        // the engine serialises it last. Same value, canonical order (015 CQ-2).
        let words: Vec<&str> = s.split(' ').collect();
        let order = ["size", "inline-size", "layout", "style", "paint"];
        let mut sorted: Vec<&str> = order
            .iter()
            .copied()
            .filter(|k| words.contains(k))
            .collect();
        sorted.extend(words.iter().copied().filter(|w| !order.contains(w)));
        return sorted.join(" ");
    }
    s
}

fn serialize(cv: &ComputedValues, name: &Name) -> String {
    match name {
        Name::Longhand(l) => {
            if let Some(style) = zeroed_by_style(*l) {
                let st = cv.computed_value_to_string(PropertyDeclarationId::Longhand(style));
                if st == "none" || st == "hidden" {
                    return "0px".to_owned();
                }
            }
            if let Some(q) = quirk_margin(cv, *l) {
                return q;
            }
            legacy_keyword(
                *l,
                cv.computed_value_to_string(PropertyDeclarationId::Longhand(*l)),
            )
        }
        Name::Custom(a) => cv.computed_value_to_string(PropertyDeclarationId::Custom(a)),
        Name::Shorthand(sh) => {
            let mut vals = sh
                .longhands()
                .map(|l| cv.computed_value_to_string(PropertyDeclarationId::Longhand(l)));
            let first = vals.next().unwrap_or_default();
            if vals.all(|v| v == first) {
                first
            } else {
                String::new()
            }
        }
    }
}

impl StyleBridge {
    /// The element's computed values: styled, or resolved on demand when undisplayed.
    /// `Ok(None)` for a disconnected element.
    fn values_for(
        &self,
        doc: &Document,
        node: NodeId,
    ) -> Result<Option<StyloArc<ComputedValues>>, Status> {
        if doc.id() != self.document() {
            return Err(status(Code::WrongDocument));
        }
        let n = doc
            .arena()
            .get(node)
            .ok_or_else(|| status(Code::StaleHandle))?;
        doc.dom_revision().map_err(|e| status(e.code))?;
        if !n.is_element() {
            return Err(status(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind));
        }
        if !doc.arena().is_connected(node) {
            return Ok(None);
        }
        if let Some(v) = self.primary_style(doc, node) {
            return Ok(Some(v));
        }
        Ok(self.resolve_undisplayed(doc, node))
    }

    /// One computed value by name (`style_get`, kind `computed`).
    pub fn computed_value(
        &self,
        doc: &Document,
        node: NodeId,
        name: &str,
    ) -> Result<String, Status> {
        let n = parse_name(name)?;
        Ok(self
            .values_for(doc, node)?
            .map(|cv| serialize(&cv, &n))
            .unwrap_or_default())
    }

    /// `style_get_all`: every longhand admitted by the profile plus every custom property in
    /// scope (sorted by name), as `(name, value)`.
    pub fn computed_all(
        &self,
        doc: &Document,
        node: NodeId,
    ) -> Result<Vec<(String, String)>, Status> {
        let Some(cv) = self.values_for(doc, node)? else {
            return Ok(vec![]);
        };
        let mut out: Vec<(String, String)> = profile_longhands()
            .into_iter()
            .map(|n| {
                (
                    n.to_owned(),
                    serialize(&cv, &Name::Longhand(longhand_by_name(n).expect("known"))),
                )
            })
            .collect();
        let custom = cv.custom_properties();
        let mut names: Vec<String> = custom
            .inherited
            .iter()
            .map(|(k, _)| k.to_string())
            .chain(custom.non_inherited.iter().map(|(k, _)| k.to_string()))
            .collect();
        names.sort();
        names.dedup();
        // `--ui4me-*` are engine-internal carriers (quirks), never listed.
        for k in names.into_iter().filter(|k| !k.starts_with("ui4me-")) {
            let v = serialize(&cv, &Name::Custom(style::Atom::from(k.as_str())));
            out.push((format!("--{k}"), v));
        }
        Ok(out)
    }
}
