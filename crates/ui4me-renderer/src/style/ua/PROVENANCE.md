# Provenance: UA stylesheets (`ua/html.css`, `ua/forms.css`, `ua/quirks.css`)

- **Owner:** 006-style-engine. Slice subset by 001 T040 (research R-16); the full v1 sheets by 006 T031 (`html.css`), T032 (`forms.css`), T033 (`quirks.css`).
- **Source:** WHATWG HTML Living Standard, section 15 "Rendering", <https://html.spec.whatwg.org/multipage/rendering.html>.
- **Snapshot:** `whatwg/html` @ `6d7788bc56a4b39d955a9a7fdb501c7c7d27999a` (`reference-library/sources/11-standards-tests/whatwg-html/source`, lines 152349-154300; read only). Line numbers below refer to that file.
- **Licence:** CC-BY-4.0 (OD-26 default, **pending owner approval**). Attribution: "Copyright (c) WHATWG (Apple, Google, Mozilla, Microsoft)", licensed under CC-BY-4.0, <https://creativecommons.org/licenses/by/4.0/>; title, link, licence and the changes below go into NOTICE (T103). Each CSS file carries an `SPDX-License-Identifier: CC-BY-4.0` header with the same attribution.
  - Note for the owner: the snapshot's `LICENSE` also says that portions incorporated into source code are licensed under BSD-3-Clause instead. OD-26 chose CC-BY-4.0 for these data files; the BSD-3-Clause reading is an alternative basis, not applied here.
- **Not used:** Blitz `default.css` (provenance unverified); Servo's UA sheet (MPL-2.0; alternative only if OD-26 is declined). No MPL-derived cross-check rules are included, so there is no separate MPL-2.0 file.
- **Ownership of values:** the rules of `html.css`, `quirks.css` and block 1 of `forms.css` are WHATWG-derived. Block 2 of `forms.css` (padding, border and margin values of 012 `contracts/controls.md` section 4, "Chromium-like") is **ui4me-owned** (T032, OD-28).

## Rules taken (`html.css`; verbatim except where a modification is listed)

| Section (source line) | Rules | Mods |
|---|---|---|
| 15.3.1 Hidden elements (152514) | `area, base, basefont, datalist, head, link, meta, noembed, noframes, param, rp, script, style, template, title { display: none }`; `[hidden]...`; `embed[hidden]`; `input[type=hidden i]` | M6 |
| 15.3.2 The page (152540) | `html, body { display: block }`; `body { margin: 8px }` | M2 |
| 15.3.3 Flow content (152679-152748) | block display list, `margin-block: 1em` list, `blockquote, figure` inline margins, `address`, `listing/plaintext/pre/xmp`, `dialog` (closed, open, `:modal`), `slot` | M9 (popover and `::backdrop` rules omitted) |
| 15.3.4 Phrasing content (152803-152835) | `cite..var`, `b, strong`, `code..tt`, `big`, `small`, `sub/sup`, `:link/:visited`, `:focus-visible`, `mark`, `abbr/acronym[title]`, `ins/u/del/s/strike`, `q::before/::after`, `nobr`, `nobr wbr`, `marquee` (L-INL, WPT-D51) | M5 (ruby, `br`, `wbr` omitted) |
| 15.3.5 Bidirectional text (152992-153011) | `unicode-bidi: isolate` list, `bdo`, `plaintext` rules | M7 |
| 15.3.7 Sections and headings (153049-153063) | `article..section`, heading weights, margins, sizes | M3 |
| 15.3.8 Lists (153072-153095) | list displays, margins, padding, counters, list-style types | none |
| 15.3.9 Tables (153187-153242, 153412) | table-part displays, `visibility: collapse`, `table`, `td, th`, `th`, `caption`, vertical alignment, border colours, `form` in table parts | M8 |
| 15.3.10 The hr element (153711) | `hr` | none |
| 15.3.11 fieldset and legend (153766-153789) | `fieldset`, `legend`, `legend[align]` | none |
| 15.4.1 Embedded content (154134) | `iframe`, `video` | none |
| 15.5 details and summary (prose) | `details, summary { display: block }`, disclosure marker | M10 |

## Rules taken (`forms.css`)

| Section (source line) | Rules | Mods |
|---|---|---|
| 15.5 Form controls (153628-153685) | `input, button, textarea` initials, `appearance: auto`, `overflow: clip !important`, `text-align`, `display: inline-block`, `appearance: none` for hidden/file/image, `box-sizing: border-box`, `textarea { white-space: pre-wrap }` | M4, M11 |
| 012 `controls.md` section 4 (ui4me-owned) | padding, border and margin values of text inputs, textarea, checkbox, radio, button and submit/reset/button inputs, select, range | M12 |
| M13 | Block 2 gains `input, select, textarea, button { font: 13.3333px system-ui; }` (the reference browser's `font: -webkit-small-control`). | L-BLK-r13: the control's font does not inherit; layout-block/box-display-contents (`display: contents` button text). ui4me-owned. |

## Rules taken (`quirks.css`)

| Section (source line) | Rules | Mods |
|---|---|---|
| 15.3.3 (152759-152761) | `form { margin-block-end: 1em }` | none |
| 15.3.8 (153114-153118) | `li { list-style-position: inside }` and the two resets | none |
| 15.3.9 (153380-153390) | `table` property resets | none |
| 15.5 (153690-153692) | `input:not([type=image i]), textarea { box-sizing: border-box }` | none |
| 15.4.2 (154247-154250) | `img[align=left/right i]` margins | none |
| 15.3.9 "Margin collapsing quirks" (153568-153621) | zero margins of the first/last block child of `body`, `td`, `th` | M13 |

## Modifications

| Id | Change | Reason |
|---|---|---|
| M1 | (slice) Subset only. Superseded by the full sheets below. | Slice scope (R-16). |
| M2 | Added `body { margin: 8px; }`. | The source states the 8px default in the presentational-hint algorithm, not as a CSS rule. |
| M3 | `:heading` written as `h1, h2, h3, h4, h5, h6`; `:heading(n)` as `hn`; `:heading(6, 7, 8, 9)` as `h6`. | The engine has no `:heading` pseudo-class; for HTML documents it matches exactly `h1`-`h6` at those levels (no `aria-level` handling). |
| M4 | Added `button, input { display: inline-block; }`. | The source states the widget behaviour in prose ("expected to act as inline-block"). |
| M5 | Omitted `ruby { display: ruby }`, `rt { display: ruby-text }` (engine-unavailable, `css.display.ruby*`), `br { display-outside: newline }` and `wbr { display-outside: break-opportunity }` (`display-outside` is not a CSS property the engine parses; `br`/`wbr` are handled as special elements by 008/009), and `@media (scripting) { noscript }` (unknown media feature). Comments and whitespace re-flowed; licence header added. | Zero-diagnostic rule (006 section 8, U4T-200); attribution (CC-BY-4.0 section 3(a)). |
| M6 | `[hidden]:not([hidden=until-found i]):not(embed)` and `[hidden=until-found i]:not(embed) { content-visibility: hidden }` are written as one `[hidden]:not(embed) { display: none }`: `hidden="until-found"` acts as `hidden`. `content-visibility` is engine-unavailable; each `until-found` element is reported once per DOM revision as `UNSUPPORTED_FEATURE` `css.property.content-visibility` (traverse.rs). | FR-009; profile row `content-visibility`. |
| M7 | The `:dir()`-based rules (`[dir]:dir(ltr)`, `bdi:dir(...)`, `input[type=tel i]:dir(ltr)`) are omitted: `:dir()` is profile-excluded. The `direction` effect of the `dir` attribute is a presentational hint (T035). The ISO-8859-8 block is omitted (no encoding-conditional sheet). | Zero-diagnostic rule; selector-support.md section 2. |
| M8 | The sentence-rule for `th` ("a rule matching th elements that have a parent node whose computed text-align is its initial value, setting text-align: center") is approximated by `th { text-align: center }` with `inherit` for a `th` under a parent carrying an `align` attribute. A parent whose `text-align` is set by author CSS is not detected, unlike the sentence. Presentational-hint blocks (`table[align]`, `thead[valign]`, `table[rules]`, `table[border]`, `hr[align]`, `td[nowrap]`, ...) are omitted here and implemented by hints.rs (T034, T035) at author level, specificity 0. | The engine has no `-moz-center-or-inherit`; FR-010. |
| M9 | `dialog::backdrop` (`css.pseudo.backdrop`, Off, R-66) and the `[popover]` / `:popover-open` rules (status N) are omitted. | Zero-diagnostic rule; profile rows. |
| M10 | `details, summary { display: block }` and the `summary:first-of-type` disclosure marker (`list-style: disclosure-closed inside`, open state `disclosure-open`) follow section 15.5 prose; `details::details-content` (Off) is omitted. | Prose-only requirements written as rules; profile row. |
| M11 | Omitted from block 1: `overflow-clip-margin` (engine-unavailable), the `:autofill { field-sizing: fixed }` rule (`field-sizing` engine-unavailable; `:autofill` status N), `align-content` rules for inputs/buttons/`::file-selector-button` (no consumer in 008; `::file-selector-button` status N). | Zero-diagnostic rule. |
| M12 | Values of 012 `controls.md` section 4 stated as rules (ui4me-owned), including `select[multiple]` inset border and `input[type=range]` margin. Pixel appearance of `appearance: auto` parts is the OD-28 theme (010), not CSS. | T032; 012 contract. |
| M13 | "No substantial previous sibling" is expressed as `:first-child` (text nodes are not counted by the selector, so a non-whitespace text node before the element is ignored), "blank" as `:empty`; "no substantial following sibling" as `:last-child`. | No selector for "substantial" nodes; the T2 comparison against the reference browser decides whether to refine this. |
| M14 | The quirks sheet is part of the cascade only while the document is in quirks mode (`DocumentStylesheetSet::sync` adds or removes it when the mode changes); limited-quirks mode does not use it (the source rules are "in quirks mode"). | Section 15 text. |
| M15 | (L-LAYFIX-r8) `select, textarea, meter, progress { display: inline-block }`: the section 15.5 prose that makes `input` and `button` act as `inline-block` covers these controls too (the Chromium baseline resolves `inline-block`). | Chromium baseline, layout-replaced controls. |
| M16 | `meter, progress { vertical-align: -0.2em }`: the reference browser's computed UA alignment, independent of appearance and author-overridable. | Chromium control-baseline measurements at 16px and 24px. |
| M17 | `textarea { overflow: auto }`: default computed overflow matches the reference browser and the existing editing scrollport behavior. Explicit `visible` is adjusted to `auto` by Stylo patch P-006-13 after ordinary overflow fixup; authored `clip`, `hidden` and `scroll` are retained. | Chromium textarea overflow controls. |
| M18 | Checkbox and radio text colour defaults to `FieldText`, instead of inheriting the parent colour; author colours override it and it follows the used colour scheme. | Chromium control colour measurements in light and dark schemes. |
| M19 | Enabled text inputs, selects and textareas default to `FieldText`; enabled buttons use `ButtonText`. File and range inputs are excluded; disabled control colours remain a separate limitation. | Chromium light/dark control colour and cascade controls. |
| M20 | Disabled checkboxes and radios use the reference UA's light/dark disabled text colours. Other disabled control colours remain separate. | DevTools UA rule and independent controls in `verification/beta/diagnostics/disabled_control_color_evidence.md`. |
| M16 | (L-DARK-r9) `:root { color: CanvasText }` and the link colours `LinkText` / `VisitedText` / `ActiveText` replace the rendering section's hex values (`#0000EE`, `#551A8B`, `#FF0000`): the system colours resolve per used colour scheme (carried patch P-006-3 gives the reference browser's values; light values are the same hex colours). The root rule is the reference browser's initial text colour (`CanvasText`), so a dark scheme gets light text. | CSS Color Adjust 1; the T2 dark variants. |
| M21 | Select display size controls `overflow`: single-row uses `clip !important`, while multi-row defaults to `hidden` / `scroll` and remains author-overridable. The UA-only state selector uses HTML size parsing and updates on size/multiple mutation. | Chromium select control measurements. |
