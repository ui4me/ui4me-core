//! owner: 009-text-inline (T057, T060, T061) — the per-document `@font-face` face view.
//!
//! 007 hands 009 the document's faces (`FontSourcesView::faces`: descriptors stored unchanged and a
//! load state). This module turns them into the *face table* the font environment matches with:
//!
//! * a declared family **shadows** an installed or registered family of the same name (CSS Fonts 4
//!   section 4.1: the family name is looked up among the document's `@font-face` rules first), also
//!   while its faces are still loading or after they failed: the text then uses the next family of
//!   the `font-family` list (`font-display` behaves as `swap`, 007 FR-047);
//! * faces of one family are put into **groups** by `unicode-range` (and by a `local()` alias). Each
//!   group is registered in the collection under one internal family name; the descriptors
//!   (`font-weight`, `font-style`, `font-stretch`, single values or ranges) are passed to Fontique as
//!   the face's attributes, so its own matching (width, style, weight, with variable-font axes)
//!   chooses the face inside a group. Groups are tried in order, **later-declared first** (CSS Fonts 4
//!   section 4.4: a later rule wins over an earlier one with the same descriptors);
//! * a group with a `unicode-range` only applies to the code points in it: `shape` splits the text of
//!   a span at the range boundaries and gives each piece the family list with only the groups that
//!   contain its characters ([`FaceTable::effective_list`]);
//! * faces are **requested lazily** (CSS Fonts 4 section 4.8, "load only when used"): resolving a
//!   `font-family` list that names a declared family notes the faces whose descriptors win the CSS
//!   matching for the requested weight/style/width (all of a unicode-range split only when the text
//!   contains a character of the range); 007 loads them and the font epoch moves once per batch.

use std::collections::BTreeMap;
use ui4me_assets::fonts::{FaceId, FaceState, FaceStyle, FaceView, FontBlob};
use ui4me_engine_types::text_metrics::{FontSpec, FontStyleSpec};

/// One face of a declared family.
#[derive(Clone, Debug)]
pub(crate) struct FaceRecord {
    pub id: FaceId,
    pub settings: ui4me_assets::fonts::FaceSettings,
    pub weight: (f32, f32),
    pub style: FaceStyle,
    pub width: (f32, f32),
    pub state: FaceState,
    pub group: usize,
}

/// Faces that share a `unicode-range` (and an alias target): one internal collection family.
#[derive(Clone, Debug)]
pub(crate) struct Group {
    /// Internal family name the group's faces are registered under (or, for an alias group, the
    /// installed / registered family the declared family stands for).
    pub name: String,
    /// Sorted, merged code-point ranges; empty = U+0-10FFFF (no restriction).
    pub ranges: Vec<(u32, u32)>,
    /// `local()` alias: `name` is an existing family and nothing is registered.
    pub alias: bool,
    /// Position of the group's last-declared face (higher = declared later = tried first).
    pub last_declared: usize,
}

/// A family declared by one or more `@font-face` rules.
#[derive(Clone, Debug)]
pub(crate) struct DeclaredFamily {
    pub display: String,
    pub faces: Vec<FaceRecord>,
    pub groups: Vec<Group>,
}

/// The document's declared families by lower-cased name.
#[derive(Clone, Debug, Default)]
pub struct FaceTable {
    families: BTreeMap<String, DeclaredFamily>,
}

fn full_range(r: &[(u32, u32)]) -> bool {
    r.is_empty() || r.iter().any(|&(a, b)| a == 0 && b >= 0x10FFFF)
}

fn normalise_ranges(r: &[(u32, u32)]) -> Vec<(u32, u32)> {
    if full_range(r) {
        return Vec::new();
    }
    let mut v: Vec<(u32, u32)> = r.iter().map(|&(a, b)| (a.min(b), a.max(b))).collect();
    v.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn in_ranges(r: &[(u32, u32)], c: u32) -> bool {
    r.is_empty() || r.iter().any(|&(a, b)| a <= c && c <= b)
}

impl FaceTable {
    /// Build the table from 007's face views (declaration order).
    pub fn build(views: &[FaceView]) -> FaceTable {
        let mut families: BTreeMap<String, DeclaredFamily> = BTreeMap::new();
        for (decl, v) in views.iter().enumerate() {
            let key = v.family.to_ascii_lowercase();
            let fam = families
                .entry(key.clone())
                .or_insert_with(|| DeclaredFamily {
                    display: v.family.clone(),
                    faces: Vec::new(),
                    groups: Vec::new(),
                });
            let ranges = normalise_ranges(&v.unicode_range);
            let alias = match &v.state {
                FaceState::LocalFamily(n) => Some(n.clone()),
                _ => None,
            };
            let group = match fam.groups.iter().position(|g| {
                g.ranges == ranges
                    && g.alias == alias.is_some()
                    && (alias.is_none() || Some(&g.name) == alias.as_ref())
            }) {
                Some(i) => i,
                None => {
                    let name = match &alias {
                        Some(n) => n.clone(),
                        None => format!("ui4me-face/{key}/{}", fam.groups.len()),
                    };
                    fam.groups.push(Group {
                        name,
                        ranges,
                        alias: alias.is_some(),
                        last_declared: decl,
                    });
                    fam.groups.len() - 1
                }
            };
            fam.groups[group].last_declared = decl;
            fam.faces.push(FaceRecord {
                id: v.id,
                settings: v.settings.clone(),
                weight: v.weight,
                style: v.style,
                width: v.width,
                state: v.state.clone(),
                group,
            });
        }
        FaceTable { families }
    }

    pub fn has_settings(&self) -> bool {
        self.families
            .values()
            .any(|f| f.faces.iter().any(|face| !face.settings.is_empty()))
    }

    /// Best matching declared faces in family-list order, later declarations first,
    /// restricted to the actual character's unicode-range. Selection still happens
    /// in Fontique; these candidates only identify the defaults of its chosen font.
    pub(crate) fn setting_candidates(&self, spec: &FontSpec, ch: char) -> Vec<FaceRecord> {
        let mut out = Vec::new();
        for name in spec.families.iter() {
            let ui4me_engine_types::text_metrics::FamilyName::Named(name) = name else {
                continue;
            };
            let Some(family) = self.families.get(&name.to_ascii_lowercase()) else {
                continue;
            };
            for i in css_best(&family.faces, spec).into_iter().rev() {
                let face = &family.faces[i];
                if in_ranges(&family.groups[face.group].ranges, ch as u32) {
                    out.push(face.clone());
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }

    /// Whether the document declares `family` (case-insensitive).
    pub fn declares(&self, family: &str) -> bool {
        self.families.contains_key(&family.to_ascii_lowercase())
    }

    /// Declared family names as written by the first rule.
    pub fn family_names(&self) -> Vec<String> {
        self.families.values().map(|f| f.display.clone()).collect()
    }

    /// The loaded blobs with the group name each is registered under, **later-declared first** (the
    /// registration order that makes the later rule win a tie).
    pub(crate) fn loaded(&self) -> Vec<(&FaceRecord, &Group, &FontBlob)> {
        let mut out = Vec::new();
        for fam in self.families.values() {
            for f in fam.faces.iter().rev() {
                if let FaceState::Loaded(b) = &f.state {
                    out.push((f, &fam.groups[f.group], b));
                }
            }
        }
        out
    }

    /// The collection family names a declared family stands for, tried in order: groups with any
    /// usable face (loaded or aliased), later-declared first. `None` if `family` is not declared;
    /// `Some(empty)` while nothing is usable (shadowing: the next family of the list applies).
    pub(crate) fn names_for(&self, family: &str) -> Option<Vec<&Group>> {
        let fam = self.families.get(&family.to_ascii_lowercase())?;
        let mut groups: Vec<&Group> = fam
            .groups
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                g.alias
                    || fam
                        .faces
                        .iter()
                        .any(|f| f.group == *i && matches!(f.state, FaceState::Loaded(_)))
            })
            .map(|(_, g)| g)
            .collect();
        groups.sort_by_key(|g| std::cmp::Reverse(g.last_declared));
        Some(groups)
    }

    /// The family list for one character: every declared family of `resolved` is replaced by the
    /// groups that contain `c`.
    pub(crate) fn has_ranged_groups(&self, resolved: &[String]) -> bool {
        self.families.values().any(|f| {
            f.groups
                .iter()
                .any(|g| !g.ranges.is_empty() && resolved.contains(&g.name))
        })
    }

    /// `resolved` restricted to the groups whose `unicode-range` contains `c`.
    pub(crate) fn effective_list(&self, resolved: &[String], c: char) -> Vec<String> {
        resolved
            .iter()
            .filter(|n| {
                self.families
                    .values()
                    .all(|f| match f.groups.iter().find(|g| &g.name == *n) {
                        Some(g) => in_ranges(&g.ranges, c as u32),
                        None => true,
                    })
            })
            .cloned()
            .collect()
    }

    /// The unloaded faces the document needs for `spec` (CSS Fonts 4 section 5.2 matching over the
    /// declared faces of every family the list names): full-range faces always, unicode-range faces
    /// only if `text` has a character in their range. Faces already loading or failed are not listed.
    pub(crate) fn needed_faces(&self, spec: &FontSpec, text: Option<&str>) -> Vec<FaceId> {
        let mut out = Vec::new();
        for f in spec.families.iter() {
            let ui4me_engine_types::text_metrics::FamilyName::Named(n) = f else {
                continue;
            };
            let Some(fam) = self.families.get(&n.to_ascii_lowercase()) else {
                continue;
            };
            for i in css_best(&fam.faces, spec) {
                let face = &fam.faces[i];
                if !matches!(face.state, FaceState::Unloaded) {
                    continue;
                }
                let ranges = &fam.groups[face.group].ranges;
                if ranges.is_empty()
                    || text.is_some_and(|t| t.chars().any(|c| in_ranges(ranges, c as u32)))
                {
                    out.push(face.id);
                }
            }
        }
        out
    }
}

/// CSS Fonts 4 section 5.2 on descriptor ranges: the faces whose (width, style, weight) win for
/// `spec`. Faces with the same winning descriptors all stay (they differ by `unicode-range` or
/// declaration order).
fn css_best(faces: &[FaceRecord], spec: &FontSpec) -> Vec<usize> {
    if faces.is_empty() {
        return Vec::new();
    }
    let mut cand: Vec<usize> = (0..faces.len()).collect();
    // 1. width: the closest (narrower first at or below normal, wider first above)
    let w = spec.stretch;
    let point = |r: (f32, f32), d: f32| d.clamp(r.0.min(r.1), r.0.max(r.1));
    let best = best_point(cand.iter().map(|&i| point(faces[i].width, w)), w, |p, d| {
        if d <= 100.0 {
            (p > d, (d - p).abs())
        } else {
            (p < d, (p - d).abs())
        }
    });
    cand.retain(|&i| point(faces[i].width, w) == best);
    // 2. style
    let order: [u8; 3] = match spec.style {
        FontStyleSpec::Normal => [0, 2, 1],
        FontStyleSpec::Italic => [1, 2, 0],
        FontStyleSpec::Oblique(_) => [2, 1, 0],
    };
    let kind = |s: FaceStyle| match s {
        FaceStyle::Normal => 0u8,
        FaceStyle::Italic => 1,
        FaceStyle::Oblique { .. } => 2,
    };
    if let Some(best_kind) = order
        .iter()
        .copied()
        .find(|k| cand.iter().any(|&i| kind(faces[i].style) == *k))
    {
        cand.retain(|&i| kind(faces[i].style) == best_kind);
    }
    // 3. weight
    let d = spec.weight;
    let best = best_point(
        cand.iter().map(|&i| point(faces[i].weight, d)),
        d,
        |p, d| {
            // lower key = better; (class, distance)
            let class = if (400.0..=500.0).contains(&d) {
                if p >= d && p <= 500.0 {
                    0
                } else if p < d {
                    1
                } else {
                    2
                }
            } else if d < 400.0 {
                if p <= d { 0 } else { 1 }
            } else if p >= d {
                0
            } else {
                1
            };
            (class, (p - d).abs())
        },
    );
    cand.retain(|&i| point(faces[i].weight, d) == best);
    cand
}

/// The candidate point with the smallest `key` (lexicographic on `(class, distance)`; a `bool` class
/// sorts `false` first).
fn best_point<K: PartialOrd + Copy>(
    points: impl Iterator<Item = f32>,
    d: f32,
    key: impl Fn(f32, f32) -> (K, f32),
) -> f32 {
    let mut best: Option<(f32, (K, f32))> = None;
    for p in points {
        let k = key(p, d);
        let better = match &best {
            None => true,
            Some((_, bk)) => k.0 < bk.0 || (k.0 == bk.0 && k.1 < bk.1),
        };
        if better {
            best = Some((p, k));
        }
    }
    best.map_or(d, |(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::text_metrics::FamilyName;

    fn rec(
        id: u64,
        w: (f32, f32),
        style: FaceStyle,
        width: (f32, f32),
        state: FaceState,
    ) -> FaceRecord {
        FaceRecord {
            id: FaceId(id),
            settings: Default::default(),
            weight: w,
            style,
            width,
            state,
            group: 0,
        }
    }

    fn spec(weight: f32, style: FontStyleSpec, stretch: f32) -> FontSpec {
        FontSpec {
            families: vec![FamilyName::Named("x".into())].into(),
            weight,
            style,
            stretch,
            lang: String::new(),
        }
    }

    #[test]
    fn css_matching_orders_width_style_weight() {
        let n = FaceStyle::Normal;
        let faces = vec![
            rec(1, (300.0, 300.0), n, (100.0, 100.0), FaceState::Unloaded),
            rec(2, (400.0, 400.0), n, (100.0, 100.0), FaceState::Unloaded),
            rec(3, (700.0, 700.0), n, (100.0, 100.0), FaceState::Unloaded),
            rec(
                4,
                (400.0, 400.0),
                FaceStyle::Italic,
                (100.0, 100.0),
                FaceState::Unloaded,
            ),
            rec(5, (400.0, 400.0), n, (75.0, 75.0), FaceState::Unloaded),
        ];
        let ids = |s: FontSpec| {
            css_best(&faces, &s)
                .into_iter()
                .map(|i| faces[i].id.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(spec(400.0, FontStyleSpec::Normal, 100.0)), [2]);
        assert_eq!(ids(spec(700.0, FontStyleSpec::Normal, 100.0)), [3]);
        assert_eq!(
            ids(spec(900.0, FontStyleSpec::Normal, 100.0)),
            [3],
            "heavier than any face: the heaviest"
        );
        assert_eq!(
            ids(spec(100.0, FontStyleSpec::Normal, 100.0)),
            [1],
            "lighter than any face: the lightest"
        );
        assert_eq!(
            ids(spec(500.0, FontStyleSpec::Normal, 100.0)),
            [2],
            "500: nothing in [500,500]; the closest below (400) before anything above"
        );
        assert_eq!(ids(spec(400.0, FontStyleSpec::Italic, 100.0)), [4]);
        assert_eq!(
            ids(spec(400.0, FontStyleSpec::Normal, 70.0)),
            [5],
            "narrow request: the narrow face"
        );
        assert_eq!(
            ids(spec(400.0, FontStyleSpec::Normal, 120.0)),
            [2],
            "wide request: no wider face, the widest below"
        );
    }

    #[test]
    fn ranges_match_inside_and_clamp_outside() {
        let n = FaceStyle::Normal;
        let faces = vec![
            rec(1, (100.0, 900.0), n, (100.0, 100.0), FaceState::Unloaded),
            rec(2, (400.0, 400.0), n, (100.0, 100.0), FaceState::Unloaded),
        ];
        let ids = |w: f32| {
            css_best(&faces, &spec(w, FontStyleSpec::Normal, 100.0))
                .into_iter()
                .map(|i| faces[i].id.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(600.0), [1], "inside the variable range");
        assert_eq!(
            ids(400.0),
            [1, 2],
            "both cover 400: both stay (the later rule is tried first)"
        );
    }

    #[test]
    fn ranges_normalise() {
        assert!(normalise_ranges(&[(0, 0x10FFFF)]).is_empty());
        assert_eq!(
            normalise_ranges(&[(0x41, 0x5A), (0x30, 0x39), (0x5B, 0x60)]),
            vec![(0x30, 0x39), (0x41, 0x60)]
        );
        assert!(in_ranges(&[], 0x1F600));
        assert!(!in_ranges(&[(0x41, 0x5A)], 0x61));
    }
}
