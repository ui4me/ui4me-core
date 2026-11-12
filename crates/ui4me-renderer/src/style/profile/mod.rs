//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! CSS feature profile `ui4me-css/1`, version 1.0.0 (T040; 006 `css-feature-profile.md` §1;
//! AD-06; OD-01; R-37). Applied **once per process, before any stylesheet (including the UA
//! sheet) is parsed**, and immutable afterwards: every sheet parse in this crate goes through
//! [`note_parse`], which applies the profile first; a request for another profile is
//! `ENGINE_MISCONFIGURED`.
//!
//! **Digest** (006 owns the algorithm): lowercase hex SHA-256 over the canonical UTF-8 text
//!
//! ```text
//! ui4me-css-profile-digest/1\n
//! id ui4me-css/1\n
//! version 1.1.0\n
//! engine <40-hex engine revision>\n
//! pref <name>=<value>\n          one line per explicit pref, sorted by name
//! feature <id> <state>[ <gate>]\n one line per feature, sorted by id
//! ```
//!
//! so any change to an engine pref value, a feature state, the engine revision or the version
//! changes the digest. The digest is a pure function of the compiled table.

pub mod filter;
pub mod table;

use std::fmt::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use table::FeatureState;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::css_profile::CssFeatureProfile;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const ID: &str = CssFeatureProfile::ID;
/// 1.1.0: `css.media.prefers-reduced-motion` On through carried patch P-006-1 (gate X-STYLE-PATCH = PASS,
/// `verification/gates/X-STYLE-PATCH/RESULT.md`; owner approval E-029; 006 T072).
/// 1.2.0: `css.property.mask` (and `mask-*`, `-webkit-mask*`) On: mask scopes in the display list (010, lane L-DARK-r9); `mask-clip: no-clip`,
/// the SVG boxes, `mask-border-*`, `mask-type` and `url(#fragment)` mask elements are not parsed or not applied (reported).
/// 1.3.0: `clip-path` basic shapes (`inset()`, `circle()`, `ellipse()`, `polygon()`, `path()`, `rect()`, `xywh()`, with a reference box) paint as display-list clip
/// scopes (010, lane L-PAINT2-r12); the row `css.value.shape-function` (CSS Shapes 2 `shape()`, parsed away by the pref `layout.css.basic-shape-shape.enabled = false`)
/// is now reported; `layout.css.ellipse-corners.enabled = true` (ellipse() corner radii); `clip-path: url()` stays Filtered.
/// 1.4.0: `css.value.background-clip-text` On (`background-clip: text` and `-webkit-background-clip: text`; `layout.css.background-clip.text.enabled = true`): the
/// clipped layers paint inside the glyph outlines of the box's text as a display-list mask scope (010, lane L-PAINT3-r14). `css.property.filter` (the ten
/// filter functions, chains, `drop-shadow()`) is painted as a filter scope in the same release; `filter: url()` stays Filtered.
/// 1.5.0: `css.selector.has` On (`:has()`; pref `layout.css.has-selector.enabled = true`; gate G-STYLE-HAS PASS, `verification/gates/G-STYLE-HAS/RESULT-2.md`, owned
/// invalidation driver `has.rs`), `css.at-rule.scope` On (`@scope` with a `<scope-start>`, `to (<scope-end>)`, `:scope`, proximity; `layout.css.at-scope.enabled = true`),
/// `css.selector.nth-child-of` On (`:nth-child()` / `:nth-last-child()` with `of S`; `layout.css.nth-child-of.enabled = true`). New row `css.at-rule.scope-implicit` (Off):
/// an `@scope` with no `<scope-start>` is rooted at its `<style>` owner's parent, which document sheets do not carry, so it is removed and reported (015, lane L-HAS).
/// 1.6.0: container queries and containment On (015 CQ-1..CQ-3, lane L-CQ): `@container` (named containers, `and`/`or`/`not`,
/// range syntax, `width`/`height`/`inline-size`/`block-size`/`aspect-ratio`/`orientation`), `container-type`, `container-name`,
/// `container`, the `cq*` units (carried Stylo patch P-006-4, register UP-01, and the live style/layout loop of the pipeline) and
/// `contain` (size, inline-size, layout, paint, style, content, strict; CSS Containment 2 in layout and paint). Style queries
/// (`@container style(...)`) stay Off: the rule stays (the query evaluates unknown) and is reported as `css.function.container-style-query`.
/// 1.7.0: the multicolumn and break properties compute (015 FRAG-2a, lane L-FRAG): pref `layout.css.columns.enabled = true` and carried Stylo
/// patch P-006-5 (`column-fill`, `column-rule-*`, `break-before`, `break-after`, `break-inside` and the legacy `page-break-*` in servo mode;
/// `break-*: column | avoid-column | avoid-page`). Their rows stay Off (new rows `css.property.break-*`): a declaration is kept and reported
/// (`table::REPORTED_PROPERTIES`) until multicolumn and paged media work end to end (MC-1, PAGE-1).
/// 1.10.0 (on top of 1.9.0, lane L-MC): typography (015 TXT-2, TXT-6, TXT-4, lane L-TXT; was 1.8.0 in the lane): `css.property.font-variation-settings`, `font-optical-sizing`, `font-synthesis` (weight, style; small-caps and position
/// are accepted: nothing is synthesized for them) and `css.value.font-variant-caps-all` On (prefs `layout.css.font-variations.enabled`, `layout.css.font-synthesis.enabled`,
/// `layout.css.font-variant-caps.all-values.enabled`); the OpenType features of `font-variant-*`, `font-kerning` and `font-feature-settings` are applied by the shaper;
/// `css.property.text-underline-offset` and `-position` On (`layout.css.text-underline.enabled`); `css.value.text-wrap-balance` On
/// (`layout.css.text-wrap-style.balance.enabled`); `css.property.line-clamp` and `-webkit-line-clamp` On (`layout.css.line-clamp.enabled`).
/// 1.9.0 (1.8.0 is lane L-TXT's): multicolumn and column breaks work end to end (015 MC-1 and FRAG-2c, lane L-MC): the rows `css.property.columns`,
/// `column-count`, `column-width`, `column-span`, `column-fill`, `column-rule` and `break-before|after|inside` are On (page breaks
/// are page-only values: on screen they never break, as in the reference browser); `orphans` and `widows` compute (carried
/// Stylo patch P-006-7, register UP-11). Nothing is kept-and-reported any more.
/// 1.11.0 (on top of 1.10.0): CSS Grid Level 2 subgrids (015 GRID-1, lane L-GRID): `css.value.subgrid` On (pref
/// `layout.css.grid-template-subgrid-value.enabled`; `grid-template-columns/rows: subgrid [<line-name-list>]`), laid out by the
/// carried Taffy patch P-008-4 (register UP-06). `repeat(auto-fill, ...)` in a subgrid's line-name list is expanded once.
/// 1.12.0 (on top of 1.11.0): CSS hyphenation (015 TXT-3, lane L-HYPH): `css.property.hyphens` and `hyphenate-character` On (pref
/// `layout.css.hyphens.enabled`). `hyphens: auto` breaks words at the points of Liang's patterns for en-US, de, fr and es (other
/// languages, an element with no `lang` and bidirectional paragraphs are reported, never approximated); `hyphenate-limit-chars`
/// is Gecko-only in Stylo and stays unavailable.
/// 1.13.0 (on top of 1.12.0): vertical writing modes and `:dir()` (015 WM-1, WM-2, lane L-WM): `css.property.writing-mode`,
/// `text-orientation` and `text-combine-upright` On (prefs `layout.css.writing-mode.enabled`, `.text-orientation.enabled`,
/// `.sideways.enabled`, `layout.css.text-combine-upright.enabled`): the layout runs in each box's logical frame (orthogonal flows
/// included) and maps the fragments to physical coordinates; upright glyphs take the font's vertical metrics. `css.selector.dir`
/// On (carried Stylo patch P-006-8, register UP-12; pref `layout.css.dir-pseudo-class.enabled`). An orthogonal subgrid is laid out
/// as an independent grid and reported (`css.value.subgrid-orthogonal`).
/// 1.14.0 (on top of 1.13.0): `::first-line` and `::first-letter` (015 TXT-5, lane L-PSEUDO): `css.pseudo.first-line` (Off) and
/// `css.pseudo.first-letter` (Filtered) On (pref `layout.css.first-line.enabled`; the sheet census no longer removes
/// `::first-letter` rules). `initial-letter` stays off and reported.
/// 1.15.0 (on top of 1.14.0): CSS Paged Media 3 (015 PAGE-1, lane L-PAGE): `css.at-rule.page` Unavailable -> On and the new row
/// `css.property.page` On (carried Stylo patch P-006-9, register UP-13; prefs `layout.css.page-rule.enabled` and
/// `layout.css.margin-rules.enabled`): `@page` with `size`, margins, `:first` / `:left` / `:right` / `:blank`, named pages and
/// the 16 page-margin boxes are used by the paged output (`DocumentPipeline::paginate`); `break-before|after: recto | verso`
/// parse. Reported rows `css.at-rule.page-vertical` and `css.at-rule.page-width-change` (Off).
/// 1.16.0 (on top of 1.15.0): `css.pseudo.backdrop` On (015 HTML-1, lane L-HTML): the `::backdrop` of an
/// element in the top layer (a modal `dialog`) is computed, boxed under the root and painted just below its element; the sheet
/// census no longer reports `::backdrop` rules. `css.selector.modal` (On since 1.0.0) now matches: `showModal()` sets the modal flag.
/// 1.17.0: inherited `accent-color` drives native checkbox, radio, range and progress painting (015 PAINT-2).
/// 1.19.0: CSS scrollbar color, width and overlay gutter (015 PAINT-3).
/// 1.20.0 (experimental, pending 015 acceptance): CSS backdrop filter functions render; URL filters remain excluded.
/// Desktop foundation: transitions, animations, and paged media are explicitly
/// filtered until their host scheduling/lifetime contract is promoted.
/// 1.22.0: `scroll-behavior`, `overscroll-behavior`, `scroll-snap-*`, `scroll-padding*`, `scroll-margin*` (0.8 F0803/F0804; carried patch P-006-16).
/// 1.23.0 (F0808): CSS anchor positioning subset. `layout.css.anchor-positioning.enabled` On with the carried Stylo patch P-006-17
/// (`anchor-name` and `position-anchor` compute under Servo): `css.property.anchor-name` and `position-anchor` On; `anchor()` in
/// `top`/`right`/`bottom`/`left` positions an out-of-flow box against its anchor's border box (layout, every pass). `position-area` and
/// `position-try*` stay Off (dropped and reported); `anchor-size()` computes but is not applied.
/// 1.24.0 (F0901): CSS transitions and keyframe animations. `@keyframes`, `transition*` and `animation*` (except `animation-timeline`,
/// `animation-range*`) are On: the document clock (shared with smooth scroll) ticks Stylo's animation sets while one runs.
/// F0902: `@starting-style` On (carried Stylo patch P-006-18: the starting style is the before-change style of an inserted or newly
/// displayed element); `steps()`, `cubic-bezier()`, `linear()`, `transition-behavior` and `animation-composition` ride on the
/// animation tokens. `display` / `content-visibility` discrete animation (`layout.css.display-animations.enabled`) stays Off.
/// 1.25.0 (0.9 F0903): `content-visibility` (hidden skips the contents; auto never skips), `contain-intrinsic-size`,
/// `field-sizing` (text controls) and `resize` (corner drag) On with the carried Stylo patch P-006-19.
/// 1.26.0 (0.9 F0906/F0909): `backdrop-filter` On (its `url()` forms stay reported), `light-dark()` images On; a
/// `border-radius` box no longer hits outside its rounded border box; blur deviations are bounded (128 px).
/// 1.27.0 (1.0 F1001): `@page` (size, margins, named pages, margin boxes) and the `page` property On for pagination.
/// 1.28.0: intrinsic flex bases, balanced wrapping and legacy break-word are consumed;
/// standard line-clamp is honestly reported as Off (legacy -webkit-line-clamp stays On).
pub const VERSION: &str = "1.28.0";
/// The single Stylo revision of the workspace (engine/Cargo.toml; X-DEPS).
pub const ENGINE_REVISION: &str = "37c1ed3b707c8f333304de2107c13b68fbc1057c";

/// The applied profile.
#[derive(Debug)]
pub struct ActiveProfile {
    digest: String,
    descriptor: CssFeatureProfile,
}

impl ActiveProfile {
    pub fn id(&self) -> &'static str {
        if has_variant() { HAS_ID } else { ID }
    }
    pub fn version(&self) -> &'static str {
        VERSION
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn engine_revision(&self) -> &'static str {
        ENGINE_REVISION
    }
    /// The shared descriptor (ui4me-core `CssFeatureProfile`): enabled feature ids. Its numeric
    /// `version` is the major version (1); the full semantic version is [`Self::version`].
    pub fn descriptor(&self) -> &CssFeatureProfile {
        &self.descriptor
    }
    /// `{id, state, gate?}` rows of §7.
    pub fn features(&self) -> impl Iterator<Item = (&'static str, FeatureState)> {
        table::FEATURES.iter().copied()
    }
}

static ACTIVE: OnceLock<ActiveProfile> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(1);
static APPLIED_AT: AtomicU64 = AtomicU64::new(0);
static FIRST_PARSE_AT: AtomicU64 = AtomicU64::new(0);

/// Canonical digest input (see module docs).
pub fn digest_input() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "ui4me-css-profile-digest/1");
    let has = has_variant();
    let _ = writeln!(s, "id {}", if has { HAS_ID } else { ID });
    let _ = writeln!(s, "version {VERSION}");
    let _ = writeln!(s, "engine {ENGINE_REVISION}");
    let mut prefs: Vec<String> = table::BOOL_PREFS
        .iter()
        .map(|(n, v)| {
            format!(
                "pref {n}={}",
                *v || (has && *n == "layout.css.has-selector.enabled")
            )
        })
        .chain(
            table::INT_PREFS
                .iter()
                .map(|(n, v)| format!("pref {n}={v}")),
        )
        .collect();
    prefs.sort();
    for p in prefs {
        let _ = writeln!(s, "{p}");
    }
    let mut feats: Vec<String> = table::FEATURES
        .iter()
        .map(|(id, st)| match st {
            FeatureState::Gated { .. } if has && *id == "css.selector.has" => {
                format!("feature {id} on")
            }
            FeatureState::Gated { gate } => format!("feature {id} gated {gate}"),
            other => format!("feature {id} {}", other.name()),
        })
        .collect();
    feats.sort();
    for f in feats {
        let _ = writeln!(s, "{f}");
    }
    s
}

/// The digest of the compiled table.
pub fn compute_digest() -> String {
    sha256_hex(digest_input().as_bytes())
}

/// Apply the profile if it is not applied yet; return it.
pub fn ensure_applied() -> &'static ActiveProfile {
    apply(false)
}

fn apply(has: bool) -> &'static ActiveProfile {
    ACTIVE.get_or_init(|| {
        if has {
            HAS_VARIANT.store(true, Ordering::SeqCst);
        }
        table::apply_bool_prefs();
        if has {
            // The test-only variant: `:has()` parses (G-STYLE-HAS experiment, T093/T094).
            stylo_static_prefs::set_pref!("layout.css.has-selector.enabled", true);
        }
        table::apply_int_prefs();
        APPLIED_AT.store(SEQ.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
        let enabled = table::FEATURES
            .iter()
            .filter(|(id, s)| *s == FeatureState::On || (has && *id == "css.selector.has"))
            .map(|(id, _)| *id);
        let digest = compute_digest();
        ActiveProfile {
            descriptor: CssFeatureProfile::new(1, digest.clone(), enabled),
            digest,
        }
    })
}

/// Verdicts of the gates that rows depend on (`gate_status` of contract section 7).
fn gate_status(gate: &str) -> &'static str {
    match gate {
        // Verdict PASS (verification/gates/X-STYLE-PATCH/RESULT.md); both patches are carried and
        // `css.media.prefers-reduced-motion` is On, so no report row names this gate any more.
        "X-STYLE-PATCH" => "PASS",
        _ => "UNKNOWN",
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl ActiveProfile {
    /// The canonical `css_profile` document of contract section 7: keys in a fixed order,
    /// features sorted by id, no whitespace variation, so the bytes are identical across runs
    /// and processes (SC-005).
    pub fn report_json(&self) -> String {
        let mut o = String::new();
        let _ = write!(
            o,
            "{{\"css_profile\":{{\"id\":{},\"version\":{},\"digest\":{},\"engine_revision\":{},\"features\":[",
            json_str(self.id()),
            json_str(self.version()),
            json_str(&self.digest),
            json_str(ENGINE_REVISION)
        );
        let has = has_variant();
        let mut feats: Vec<(&str, FeatureState)> = table::FEATURES.to_vec();
        feats.sort_by_key(|(id, _)| *id);
        for (i, (id, st)) in feats.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            let _ = write!(o, "{{\"id\":{}", json_str(id));
            match st {
                FeatureState::Gated { .. } if has && *id == "css.selector.has" => {
                    o.push_str(",\"state\":\"on\"")
                }
                FeatureState::Gated { gate } => {
                    let _ = write!(
                        o,
                        ",\"state\":\"gated\",\"gate\":{},\"gate_status\":{}",
                        json_str(gate),
                        json_str(gate_status(gate))
                    );
                }
                other => {
                    let _ = write!(o, ",\"state\":{}", json_str(other.name()));
                }
            }
            o.push('}');
        }
        o.push_str("]}}");
        o
    }
}

/// The test-only profile `ui4me-css/1+has` (carries `:has()`); available only in builds with the
/// `gate-profiles` cargo feature, never in release.
pub const HAS_ID: &str = "ui4me-css/1+has";

static HAS_VARIANT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the process runs the `+has` test profile (`:has()` parses and matches).
pub fn has_variant() -> bool {
    HAS_VARIANT.load(Ordering::SeqCst)
}

/// The active profile (applies it on first use).
pub fn active() -> &'static ActiveProfile {
    ensure_applied()
}

pub fn is_applied() -> bool {
    ACTIVE.get().is_some()
}

/// A request for a profile by identity: OK for `ui4me-css/1` 1.1.0; anything else is
/// `ENGINE_MISCONFIGURED` (the profile is fixed per process; §1).
pub fn request(id: &str, version: &str) -> Result<&'static ActiveProfile, Status> {
    if id == HAS_ID && version == VERSION && cfg!(feature = "gate-profiles") {
        let p = apply(true);
        if has_variant() {
            return Ok(p);
        }
        return Err(Status::new(Code::EngineMisconfigured, Stage::Cascade).with_message(format!("profile {id} requested after {ID} was applied; the profile is fixed for this process")));
    }
    let p = ensure_applied();
    if id == ID && version == VERSION && !has_variant() {
        Ok(p)
    } else {
        Err(
            Status::new(Code::EngineMisconfigured, Stage::Cascade).with_message(format!(
                "profile {id} {version} requested; {ID} {VERSION} is fixed for this process"
            )),
        )
    }
}

/// Called before every stylesheet parse in this crate (UA, author, inline, queries).
pub(crate) fn note_parse() {
    ensure_applied();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let _ = FIRST_PARSE_AT.compare_exchange(0, seq, Ordering::SeqCst, Ordering::SeqCst);
}

/// Ordering evidence: when the profile was applied and when the first sheet was parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileSequence {
    pub applied_at: Option<u64>,
    pub first_parse_at: Option<u64>,
}

pub fn sequence() -> ProfileSequence {
    let f = |a: &AtomicU64| Some(a.load(Ordering::SeqCst)).filter(|&v| v != 0);
    ProfileSequence {
        applied_at: f(&APPLIED_AT),
        first_parse_at: f(&FIRST_PARSE_AT),
    }
}

// ------------------------------------------------------------------ SHA-256 (FIPS 180-4)

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            super::sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = "a".repeat(1000);
        assert_eq!(super::sha256_hex(long.as_bytes()).len(), 64);
    }
}
