//! owner: 006-style-engine — T045, T046, T047, T051
//!
//! The post-parse rule census (AD-P-006-b, approved by R-61; the observable FR-055 rule binds)
//! and the owned text pre-scan:
//!
//! * [`census`] walks each new sheet once. It removes and reports the rules and declarations of
//!   constructs the engine parses but the profile excludes (`::first-letter`, `baseline-source`,
//!   `clip-path: url()`, `filter: url()`), reports never-matching selectors once per sheet per
//!   feature (`::backdrop`, `::details-content`, `::file-selector-button`, `::color-swatch`) and
//!   removes rules of `@font-feature-values` (parsed, no consumer).
//! * [`prescan`] tokenises the sheet text once. It reports unknown media features once per sheet
//!   per feature (evaluated as unknown, so `not` of one is false). `prefers-reduced-motion` is a
//!   known feature since patch P-006-1 (profile 1.1.0). Unknown vendor-prefixed declarations are
//!   no longer scanned for here: patch P-006-2 makes the engine report each one through the
//!   `ParseErrorReporter` (`diagnostics::classify`; T049 retired the T047 pre-scan path).
//!
//! `supports` evaluation (T051) lives in [`supports_declaration`].

use crate::style::diagnostics::{Severity, StyleDiagnosticRecord};
use cssparser::{Parser, SourceLocation, ToCss as _, Token};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};
use style::properties::PropertyId;
use style::shared_lock::SharedRwLock;
use style::stylesheets::{CssRule, Stylesheet};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, SheetOrigin};
use ui4me_engine_types::ids::NodeId;

/// Where the records of one sheet are attributed.
#[derive(Clone, Copy)]
pub(crate) struct Where {
    pub(crate) sheet_id: u32,
    pub(crate) origin: SheetOrigin,
    pub(crate) node: Option<NodeId>,
}

fn record(
    w: Where,
    code: Code,
    sev: Severity,
    feature: Option<FeatureId>,
    category: Option<&'static str>,
    loc: SourceLocation,
    excerpt: String,
) -> StyleDiagnosticRecord {
    StyleDiagnosticRecord {
        code,
        severity: sev,
        feature,
        category,
        sheet_id: w.sheet_id,
        origin: w.origin,
        url: None,
        node: w.node,
        line: loc.line + 1,
        column: loc.column,
        excerpt: excerpt.chars().take(120).collect(),
    }
}

/// Interned dynamic feature ids (`css.media.<name>`), bounded so a hostile sheet cannot grow it.
fn intern(prefix: &str, name: &str) -> Option<FeatureId> {
    static MAP: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let key = format!("{prefix}{name}");
    let mut m = MAP.get_or_init(Default::default).lock().ok()?;
    if let Some(s) = m.get(&key) {
        return FeatureId::new(s);
    }
    if m.len() >= 256
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return FeatureId::new("css.media.other");
    }
    let leaked: &'static str = Box::leak(key.clone().into_boxed_str());
    m.insert(key, leaked);
    FeatureId::new(leaked)
}

// ------------------------------------------------------------------ text pre-scan

const KNOWN_MEDIA: &[&str] = &[
    "width",
    "height",
    "orientation",
    "pointer",
    "any-pointer",
    "hover",
    "any-hover",
    "aspect-ratio",
    "device-width",
    "device-height",
    "scan",
    "resolution",
    "device-pixel-ratio",
    "-moz-device-pixel-ratio",
    "-webkit-device-pixel-ratio",
    "prefers-color-scheme",
    "prefers-reduced-motion",
];

fn known_media_feature(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let base = n
        .strip_prefix("min-")
        .or_else(|| n.strip_prefix("max-"))
        .unwrap_or(&n);
    let base = base
        .strip_prefix("-webkit-")
        .map_or_else(|| base.to_owned(), |b| format!("-webkit-{b}"));
    KNOWN_MEDIA.contains(&n.as_str())
        || KNOWN_MEDIA.contains(&base.as_str())
        || (n.starts_with("-webkit-m") && n.ends_with("device-pixel-ratio"))
}

struct Scan<'a> {
    w: Where,
    out: Vec<StyleDiagnosticRecord>,
    media_seen: BTreeSet<String>,
    _text: &'a str,
}

impl Scan<'_> {
    fn media_condition(&mut self, p: &mut Parser<'_>, loc: SourceLocation) {
        // Inside one parenthesis block: nested blocks are conditions; otherwise the first
        // identifier that is not a combinator is the feature name.
        let mut feature: Option<String> = None;
        let mut nested: Vec<()> = vec![];
        while let Ok(t) = p.next_including_whitespace().cloned() {
            match t {
                Token::ParenthesisBlock => {
                    nested.push(());
                    let _ = p.parse_nested_block(|q| -> Result<(), cssparser::ParseError<()>> {
                        self.media_condition(q, loc);
                        Ok(())
                    });
                }
                Token::Ident(id)
                    if feature.is_none()
                        && !["and", "or", "not", "only"].contains(&&*id.to_ascii_lowercase()) =>
                {
                    feature = Some(id.to_string())
                }
                _ => {}
            }
        }
        if nested.is_empty()
            && let Some(f) = feature
        {
            self.media_feature(&f, loc);
        }
    }

    fn media_feature(&mut self, name: &str, loc: SourceLocation) {
        let lname = name.to_ascii_lowercase();
        if known_media_feature(&lname) || !self.media_seen.insert(lname.clone()) {
            return;
        }
        if let Some(f) = intern("css.media.", &lname) {
            self.out.push(record(
                self.w,
                Code::UnsupportedFeature,
                Severity::Warning,
                Some(f),
                None,
                loc,
                format!("@media ({lname}): unknown media feature"),
            ));
        }
    }

    fn walk(&mut self, p: &mut Parser<'_>) {
        let mut pending_media: Option<SourceLocation> = None;
        loop {
            let loc = p.current_source_location();
            let Ok(t) = p.next_including_whitespace_and_comments().cloned() else {
                break;
            };
            match t {
                Token::WhiteSpace(_) | Token::Comment(_) => continue,
                Token::AtKeyword(k) => {
                    pending_media = k.eq_ignore_ascii_case("media").then_some(loc);
                }
                Token::ParenthesisBlock if pending_media.is_some() => {
                    let l = pending_media.unwrap_or(loc);
                    let _ = p.parse_nested_block(|q| -> Result<(), cssparser::ParseError<()>> {
                        self.media_condition(q, l);
                        Ok(())
                    });
                }
                Token::CurlyBracketBlock => {
                    pending_media = None;
                    let _ = p.parse_nested_block(|q| -> Result<(), cssparser::ParseError<()>> {
                        self.walk(q);
                        Ok(())
                    });
                }
                Token::Semicolon => {
                    pending_media = None;
                }
                Token::Ident(name) if p.try_parse(|q| q.expect_colon()).is_ok() => {
                    // A declaration of a kept-and-reported row (1.7.0, `REPORTED_PROPERTIES`), by the name
                    // as written (a shorthand reports its own row). Selectors (`a:hover`) never name one.
                    pending_media = None;
                    if let Some(f) = crate::style::profile::table::reported_property_feature(&name)
                    {
                        let value = p
                            .slice_from(p.position())
                            .split([';', '}'])
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_owned();
                        self.out.push(record(
                            self.w,
                            Code::UnsupportedFeature,
                            Severity::Warning,
                            Some(f),
                            None,
                            loc,
                            format!("{}: {value}", name.to_ascii_lowercase()),
                        ));
                    }
                }
                Token::ParenthesisBlock | Token::SquareBracketBlock | Token::Function(_) => {
                    let _ = p.parse_nested_block(|q| -> Result<(), cssparser::ParseError<()>> {
                        while q.next_including_whitespace_and_comments().is_ok() {}
                        Ok(())
                    });
                }
                _ => {}
            }
        }
    }
}

/// Tokenise the sheet text once for unknown media features (reported once per sheet per feature).
/// Lines are 1-based positions inside the text. (`background-clip: text` was reported here while the
/// profile excluded it, WPT-D46; it is On since profile 1.4.0.)
pub(crate) fn prescan(
    text: &str,
    w: Where,
    _known: &[StyleDiagnosticRecord],
) -> Vec<StyleDiagnosticRecord> {
    let mut parser = Parser::new(text);
    let mut scan = Scan {
        w,
        out: vec![],
        media_seen: BTreeSet::new(),
        _text: text,
    };
    scan.walk(&mut parser);
    scan.out
}

/// `prescan` of a `style=""` value (one declaration block): the same findings, positioned inside
/// the attribute. An engine record of the feature on the element suppresses the duplicate.
pub(crate) fn prescan_declarations(
    text: &str,
    w: Where,
    known: &[StyleDiagnosticRecord],
) -> Vec<StyleDiagnosticRecord> {
    let wrapped = format!("*{{{text}}}");
    let mut found = prescan(&wrapped, w, &[]);
    found.retain(|r| !known.iter().any(|k| k.feature == r.feature));
    found
}

// ------------------------------------------------------------------ rule census

/// The sheet-level pseudo-element features the profile excludes although the engine may parse
/// them (`css-feature-profile.md` section 6, section 3): `(selector text, feature, removed)`.
const PSEUDOS: &[(&str, &str, bool)] = &[
    ("::details-content", "css.pseudo.details-content", false),
    (
        "::file-selector-button",
        "css.pseudo.file-selector-button",
        false,
    ),
    ("::color-swatch", "css.pseudo.color-swatch", false),
];

/// The feature of a sheet selector that the profile excludes (used when the engine rejects the
/// whole rule at parse time).
pub fn sheet_selector_feature(text: &str) -> Option<FeatureId> {
    let t = text.to_ascii_lowercase();
    PSEUDOS
        .iter()
        .find(|(p, _, _)| t.contains(p))
        .and_then(|(_, f, _)| FeatureId::new(f))
}

fn declaration_feature(name: &str, value: &str) -> Option<FeatureId> {
    let id = match name {
        // `vertical-align` expands to baseline-source: auto; css-wide keywords come from that expansion; only `first`/`last` are the author's.
        "baseline-source"
            if !matches!(
                value.trim(),
                "auto" | "inherit" | "initial" | "unset" | "revert"
            ) =>
        {
            "css.property.baseline-source"
        }
        "clip-path" if value.to_ascii_lowercase().contains("url(") => "css.value.clip-path-url",
        "filter" | "backdrop-filter" if value.to_ascii_lowercase().contains("url(") => {
            "css.value.filter-url"
        }
        _ => return crate::style::profile::table::property_feature(name),
    };
    FeatureId::new(id)
}

type Rules = style::servo_arc::Arc<style::shared_lock::Locked<style::stylesheets::CssRules>>;
type Block =
    style::servo_arc::Arc<style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>>;

enum Action {
    RemoveRule(Rules, usize),
    RemoveDecl(Block, PropertyId),
}

/// Walk a parsed sheet once: report and remove (or just report) the post-parse exclusions.
pub(crate) fn census(
    sheet: &Stylesheet,
    lock: &SharedRwLock,
    w: Where,
) -> Vec<StyleDiagnosticRecord> {
    let mut out = vec![];
    let mut once: BTreeSet<&'static str> = BTreeSet::new();
    let mut actions = vec![];
    {
        let r = lock.read();
        let contents = sheet.contents.read_with(&r).clone();
        plan_rules(&contents.rules, &r, w, &mut out, &mut once, &mut actions);
    }
    if !actions.is_empty() {
        let mut g = lock.write();
        // Rule removals go from the highest index down so earlier indices stay valid.
        let mut removals: Vec<(Rules, usize)> = vec![];
        for a in actions {
            match a {
                Action::RemoveDecl(b, pid) => {
                    let blk = b.write_with(&mut g);
                    if let Some(first) = blk.first_declaration_to_remove(&pid) {
                        blk.remove_property(&pid, first);
                    }
                }
                Action::RemoveRule(r, i) => removals.push((r, i)),
            }
        }
        removals.sort_by_key(|(_, i)| std::cmp::Reverse(*i));
        for (r, i) in removals {
            let v = &mut r.write_with(&mut g).0;
            if i < v.len() {
                v.remove(i);
            }
        }
    }
    out
}

fn plan_rules(
    rules: &Rules,
    r: &style::shared_lock::SharedRwLockReadGuard<'_>,
    w: Where,
    out: &mut Vec<StyleDiagnosticRecord>,
    once: &mut BTreeSet<&'static str>,
    actions: &mut Vec<Action>,
) {
    let list: Vec<CssRule> = rules.read_with(r).0.clone();
    for (i, rule) in list.iter().enumerate() {
        match rule {
            CssRule::Style(sr) => {
                let sr = sr.read_with(r);
                let selectors = {
                    let mut s = String::new();
                    let _ = sr.selectors.to_css(&mut s);
                    s
                };
                let loc = sr.source_location;
                let lower = selectors.to_ascii_lowercase();
                for (pat, feat, removed) in PSEUDOS {
                    if lower.contains(pat) {
                        if let Some(f) = FeatureId::new(feat)
                            && (*removed || once.insert(feat))
                        {
                            out.push(record(
                                w,
                                Code::UnsupportedFeature,
                                Severity::Warning,
                                Some(f),
                                None,
                                loc,
                                selectors.clone(),
                            ));
                        }
                        if *removed {
                            actions.push(Action::RemoveRule(rules.clone(), i));
                        }
                        break;
                    }
                }
                // Declarations the profile excludes although the engine parsed them.
                let b = sr.block.read_with(r);
                for d in b.declarations() {
                    let name = d.id().name().to_string();
                    let mut value = String::new();
                    if let Ok(pid) = PropertyId::parse_enabled_for_all_content(&name) {
                        let _ = b.property_value_to_css(&pid, &mut value);
                        // Kept-and-reported rows (1.7.0): the pre-scan reports them by the name as written.
                        if crate::style::profile::table::reported_property_feature(&name).is_some()
                        {
                            continue;
                        }
                        if let Some(f) = declaration_feature(&name, &value) {
                            out.push(record(
                                w,
                                Code::UnsupportedFeature,
                                Severity::Warning,
                                Some(f),
                                None,
                                loc,
                                format!("{name}: {value}"),
                            ));
                            actions.push(Action::RemoveDecl(sr.block.clone(), pid));
                        }
                    }
                }
                if let Some(n) = &sr.rules {
                    plan_rules(n, r, w, out, once, actions);
                }
            }
            CssRule::FontFeatureValues(ffv) => {
                if let Some(f) = FeatureId::new("css.at-rule.font-feature-values") {
                    out.push(record(
                        w,
                        Code::UnsupportedFeature,
                        Severity::Warning,
                        Some(f),
                        None,
                        ffv.source_location,
                        "@font-feature-values".into(),
                    ));
                }
                actions.push(Action::RemoveRule(rules.clone(), i));
            }
            CssRule::Container(c) => {
                // Style queries stay off (pref `layout.css.style-queries.enabled`, profile 1.6.0): the engine
                // parses `style(...)` as `<general-enclosed>`, which evaluates to unknown. The rule stays (its
                // other conditions keep their meaning) and the query is reported, never silently ignored.
                let mut cond = String::new();
                let _ = style_traits::ToCss::to_css(
                    &c.conditions,
                    &mut style_traits::CssWriter::new(&mut cond),
                );
                if cond.to_ascii_lowercase().contains("style(")
                    && let Some(f) = FeatureId::new("css.function.container-style-query")
                {
                    out.push(record(
                        w,
                        Code::UnsupportedFeature,
                        Severity::Warning,
                        Some(f),
                        None,
                        c.source_location,
                        format!("@container {cond}"),
                    ));
                }
                plan_rules(&c.rules, r, w, out, once, actions)
            }
            CssRule::Scope(sc) => {
                // An implicit-scope `@scope { ... }` (no `<scope-start>`) is rooted at the parent of its
                // owner `<style>` element (CSS Cascade 6 section 2.5.1); the engine's document sheets
                // carry no owner, so it would silently root at `:root`. Not implemented: removed and
                // reported (015 L-HAS; an unimplemented feature stays UNSUPPORTED_FEATURE).
                if sc.bounds.start.is_none() {
                    if let Some(f) = FeatureId::new("css.at-rule.scope-implicit") {
                        out.push(record(
                            w,
                            Code::UnsupportedFeature,
                            Severity::Warning,
                            Some(f),
                            None,
                            sc.source_location,
                            "@scope".into(),
                        ));
                    }
                    actions.push(Action::RemoveRule(rules.clone(), i));
                } else {
                    plan_rules(&sc.rules, r, w, out, once, actions);
                }
            }
            CssRule::Media(m) => plan_rules(&m.rules, r, w, out, once, actions),
            CssRule::Supports(s) => plan_rules(&s.rules, r, w, out, once, actions),
            CssRule::LayerBlock(l) => plan_rules(&l.rules, r, w, out, once, actions),
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ @supports (T051)

/// Whether `property: value` is supported under the profile: the engine must accept it and the
/// profile must not exclude it (shared by `@supports` and `css_supports`).
pub fn supports_declaration(property: &str, value: &str) -> bool {
    crate::style::profile::ensure_applied();
    if crate::style::profile::table::property_feature(property).is_some()
        || crate::style::profile::table::value_feature(property, value).is_some()
    {
        return false;
    }
    if declaration_feature(&property.to_ascii_lowercase(), value).is_some() {
        return false;
    }
    let url = crate::style::sheets::blank_url();
    let mut src = style::properties::SourcePropertyDeclaration::default();
    match PropertyId::parse_enabled_for_all_content(property) {
        Ok(id) => style::properties::parse_one_declaration_into(
            &mut src,
            id,
            value,
            style::stylesheets::Origin::Author,
            &url,
            None,
            style_traits::ParsingMode::DEFAULT,
            style::context::QuirksMode::NoQuirks,
            style::stylesheets::CssRuleType::Style,
        )
        .is_ok(),
        Err(()) => false,
    }
}

/// `css_supports_condition(text)`: the condition grammar of `@supports` (`not`, `and`, `or`,
/// parentheses, `(property: value)`, `selector(...)`, `at-rule(@name)`), evaluated through the
/// profile: every construct excluded at parse level answers false, consistent with the census.
pub fn supports_condition(text: &str) -> bool {
    crate::style::profile::ensure_applied();
    let t = text.trim();
    let mut pos = 0;
    let b = t.as_bytes();
    let v = cond_or(t, b, &mut pos);
    v && {
        skip_ws(b, &mut pos);
        pos >= b.len()
    }
}

fn skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && b[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn word_at(t: &str, pos: usize, w: &str) -> bool {
    t.get(pos..pos + w.len())
        .is_some_and(|x| x.eq_ignore_ascii_case(w))
        && t.as_bytes()
            .get(pos + w.len())
            .is_none_or(|c| c.is_ascii_whitespace() || *c == b'(')
}

fn cond_or(t: &str, b: &[u8], pos: &mut usize) -> bool {
    let mut v = cond_and(t, b, pos);
    loop {
        skip_ws(b, pos);
        if word_at(t, *pos, "or") {
            *pos += 2;
            let r = cond_and(t, b, pos);
            v = v || r;
        } else {
            return v;
        }
    }
}

fn cond_and(t: &str, b: &[u8], pos: &mut usize) -> bool {
    let mut v = cond_not(t, b, pos);
    loop {
        skip_ws(b, pos);
        if word_at(t, *pos, "and") {
            *pos += 3;
            let r = cond_not(t, b, pos);
            v = v && r;
        } else {
            return v;
        }
    }
}

fn cond_not(t: &str, b: &[u8], pos: &mut usize) -> bool {
    skip_ws(b, pos);
    if word_at(t, *pos, "not") {
        *pos += 3;
        return !cond_atom(t, b, pos);
    }
    cond_atom(t, b, pos)
}

/// A balanced `( ... )` group starting at `*pos`; returns its inner text.
fn group<'a>(t: &'a str, b: &[u8], pos: &mut usize) -> Option<&'a str> {
    skip_ws(b, pos);
    if b.get(*pos) != Some(&b'(') {
        return None;
    }
    let start = *pos + 1;
    let mut depth = 0;
    let mut i = *pos;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    *pos = i + 1;
                    return Some(&t[start..i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn cond_atom(t: &str, b: &[u8], pos: &mut usize) -> bool {
    skip_ws(b, pos);
    for (kw, kind) in [("selector", 0), ("at-rule", 1)] {
        if word_at(t, *pos, kw) {
            *pos += kw.len();
            let Some(inner) = group(t, b, pos) else {
                return false;
            };
            return if kind == 0 {
                selector_supported(inner.trim())
            } else {
                at_rule_supported(inner.trim())
            };
        }
    }
    let Some(inner) = group(t, b, pos) else {
        return false;
    };
    let inner = inner.trim();
    // A nested condition, or a declaration `name: value`.
    if inner.starts_with('(') || word_at(inner, 0, "not") {
        let mut p = 0;
        let ib = inner.as_bytes();
        return cond_or(inner, ib, &mut p);
    }
    match inner.split_once(':') {
        Some((name, value)) => supports_declaration(name.trim(), value.trim()),
        None => false,
    }
}

fn selector_supported(sel: &str) -> bool {
    crate::style::query::SelectorService::new()
        .validate(sel)
        .is_ok()
        && sheet_selector_feature(sel).is_none()
}

fn at_rule_supported(name: &str) -> bool {
    let n = name.trim_start_matches('@').to_ascii_lowercase();
    crate::style::profile::table::at_rule_feature(&n).is_none()
        && matches!(
            n.as_str(),
            "media"
                | "supports"
                | "font-face"
                | "keyframes"
                | "layer"
                | "import"
                | "namespace"
                | "property"
                | "charset"
                | "scope"
                | "starting-style"
        )
}
