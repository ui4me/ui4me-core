# HTML and CSS scope

UI4ME renders HTML and CSS with its own native engine: an HTML5 parser, Servo's Stylo for the CSS cascade, Taffy and UI4ME layout code for block, inline, flex, grid and tables, and a CPU rasterizer. It is designed for desktop application interfaces, not for browsing the web. Most modern CSS that a desktop UI needs works; anything that would run code or load from the network does not exist.

This page describes the profile of UI4ME Core 1.0.x (`css_profile: "ui4me-desktop-1.0"` in the host capabilities). At runtime, the `css_profile` document command lists every profile feature with its state, and `explain` tells you why a property has no effect on an element. Use them, or the `ui4me-host render` [diagnostic](../reference/host-cli.md#render), when a page looks different from a browser.

## Rejected outright

A document or HTML fragment that contains any of the following **fails to load** (the previous document stays in place):

* `<script>`, `<iframe>`, `<object>`, `<embed>`;
* any `on…` event-handler attribute, or `srcdoc`;
* `contenteditable` with any value other than `false` (use `<input>` or `<textarea>`);
* `<input type>` `color`, `date`, `time`, `month`, `week`, `datetime-local` (no native picker);
* duplicate `id` values, or an `id` that contains whitespace or is longer than 256 bytes.

There is no JavaScript engine and no DOM scripting API: all logic runs in your Python or PowerShell process, which receives [events](#events-delivered-to-the-backend) and changes the page through the binding API.

## Elements

* **Structure and text:** the usual flow, sectioning, heading, list, phrasing, quotation, bidi and table elements, `hr`, `figure`, `details`/`summary` (including the exclusive `name` accordion), `fieldset`/`legend`, `dialog`. Default styles come from the WHATWG rendering rules.
* **Links:** activating `<a href>` never navigates. The backend receives a `link_activated` event with `href` and the resolved `url` and decides what to do, for example `desktop.open_external`.
* **Inert placeholders:** `<canvas>`, `<video>` and `<audio controls>` produce empty boxes (300×150 unless sized). They have no drawing API or playback. Use a [live bitmap](#images-and-resources) for custom pixels.
* **Ignored:** `noscript` is ordinary markup (scripting is off); declarative shadow roots are not attached; custom elements are plain elements.

## Forms and controls

| Control | Support |
| --- | --- |
| `<input>` | `text`, `search`, `tel`, `url`, `email`, `password`, `number` (arrow-key stepping, no spin buttons), `checkbox`, `radio` (arrow keys within a group), `range` (keyboard and pointer, mirrored in RTL), `button`, `submit`, `reset`, `image`, `hidden`, `file` (native file picker; paths only, at most 1024). Unknown types behave as `text`. |
| `<textarea>` | Multi-line editing, with `field-sizing` support |
| `<select>` | Drop-down drawn inside the window, list box with `size` > 1, `multiple` (Ctrl toggles, Shift extends), `optgroup` |
| `<button>`, `<label>` | Activation, label forwarding |
| `<progress>`, `<meter>` | Painted with accessibility roles; indeterminate progress is a static stripe |
| `<output>` | Plain text only |
| `<datalist>` | No suggestion popup |

**Editing** in text controls includes caret and selection, IME composition, clipboard (plain text), and per-control undo/redo; `edit_command` exposes `undo`, `redo`, `cut`, `copy`, `paste`, `select_all` and `delete`.

**Constraint validation:** `required`, `minlength`/`maxlength`, `pattern`, `type=email|url|number`, `min`/`max`, and `step` (numbers), plus custom validity messages, with the standard validity flags and `:valid`/`:invalid`. `pattern` uses Rust regular-expression syntax (anchored) rather than JavaScript syntax; an invalid pattern is ignored. Messages are fixed English strings and no validation bubble is shown — display your own.

**Submission** never navigates or sends anything. Submitting a valid form fires a cancelable `submit` event (with the `submitter`) to your backend; invalid controls get `invalid` events and the first one is focused. `get_form_data` returns the entries in document order (file inputs report file names and paths, never contents); `request_submit` and `reset_form` are available.

## Dialogs, popovers and selection

* **`<dialog>`:** non-modal and modal (top layer, `::backdrop`, `:modal`, rest of the page inert). Escape sends a cancelable `cancel` and then `close` with the `return_value`. Controlled with `show_dialog`, `close_dialog`, `get_dialog`.
* **Popovers:** `popover="auto"` (light dismiss, Escape) and `popover="manual"`, `popovertarget` on buttons, `beforetoggle`/`toggle` events, `:popover-open`. `popover="hint"` is not supported. CSS anchor positioning works with `anchor-name`, `position-anchor` and `anchor()`; `position-area`, `position-try` and `anchor-size()` are off.
* **Selection:** read-only document text can be selected and copied (`get_selection`, `select_text`, `copy_selection`).
* **In-page drag:** `draggable="true"` gives `dragstart`, `dragover`, `drop` and `dragend` events inside the document (no `DataTransfer`, no dragging out to other applications).

## CSS

**Selectors:** everything Stylo parses, including `:is()`, `:where()`, `:not()`, `:has()`, `:nth-child(An+B of S)`, `:dir()`, `:lang()`, nesting and `@scope` with an explicit root. State pseudo-classes driven by the engine: `:hover`, `:active`, `:focus`, `:focus-visible`, `:focus-within`, `:checked`, `:indeterminate`, `:disabled`, `:enabled`, `:required`, `:optional`, `:placeholder-shown`, `:valid`, `:invalid`, `:modal`, `:popover-open`, `:open`, `:link`, `:any-link`. `:visited` never matches. Pseudo-elements: `::before`, `::after`, `::marker`, `::first-line`, `::first-letter`, `::backdrop`. `::placeholder` and `::selection` use fixed theme colours and cannot be styled; `:host`, `::part` and `::slotted` do not apply (no Shadow DOM).

**At-rules:** `@media`, `@supports`, `@import` (local files), `@layer`, `@container` (size queries), `@font-face`, `@keyframes`, `@property`, `@namespace`, `@scope`, `@starting-style`. Not available: `@custom-media`, `@counter-style`, `@font-palette-values`, `@view-transition`, container style queries.

**Media features:** viewport `width`/`height`, `orientation`, `aspect-ratio`, `resolution`, `pointer`, `hover`, **`prefers-color-scheme`** and **`prefers-reduced-motion`**, which follow the desktop settings (or an application override with `set_environment`). `prefers-contrast` and `forced-colors` are not recognised; high contrast is observed and reported to the backend but no forced-colours palette is applied.

**Values:** custom properties and `var()`, `calc()`, `min()`, `max()`, `clamp()`, `color-mix()`, `oklch()` and other modern colour syntax, `light-dark()`, container query units, `fit-content()`, counters. Off: `random()`, `cross-fade()`, `shape()`, `anchor-size()`, `zoom`, `font-size-adjust`, `initial-letter`.

**Layout:**

| Feature | Status |
| --- | --- |
| `block`, `inline`, `inline-block`, `flow-root`, `list-item`, `contents`, `none` | Supported |
| Flexbox | Supported: wrap, `gap`, `order`, alignment; also `flex-wrap: balance` |
| Grid | Supported: templates and named areas, auto-placement, `subgrid` |
| Tables | Supported (`table` / `inline-table` and table elements) |
| Positioning | `relative`, `absolute`, `fixed`, `sticky`; floats (no `shape-outside`) |
| Overflow and scrolling | Scroll containers, smooth `scroll-behavior`, scroll snap, `scroll-margin`/`scroll-padding`, `overscroll-behavior`, scrollbar colour/width/gutter |
| Multi-column | Supported, with column breaks |
| Writing modes and bidi | Vertical writing modes, `text-orientation`, `text-combine-upright`, RTL and bidi isolation |
| Containment | `contain`, `content-visibility: hidden`, `contain-intrinsic-size` |
| `resize` | Supported, with a painted grip |
| Ruby, masonry | Not supported |

**Visual effects:** borders and `border-radius`, `box-shadow` (outer and inset), outlines (`outline-style: auto` draws a focus ring), linear, radial and conic gradients, opacity, 2D `transform` (3D transforms and `perspective` are not supported), `filter` functions and `drop-shadow`, `backdrop-filter` (blur ≤ 128 px), masks, `clip-path` basic shapes and `path()` (no `url()` references), `background-clip: text`, `accent-color`. **Transitions** and **`@keyframes` animations** run on the document clock with `cubic-bezier()`, `steps()` and `linear()` easing; scroll-driven animations and view transitions are not supported. Cursors map to the native standard set (`pointer`, `text`, `move`, `grab`, resize cursors, …); `url()` cursors fall back to the default.

## Text and fonts

* **System fonts** are used through fontconfig (Linux) or the Windows font system, with generic families (`sans-serif`, `serif`, `monospace`, `system-ui`, …) and per-character fallback. Noto Sans Regular is built in as the last resort, so text always renders.
* **`@font-face`** loads TTF, OTF, TTC, WOFF and WOFF2 from local files under the asset root, from `data:` URLs or from registered resources (≤ 32 MiB per font); `local()` names system fonts. `unicode-range` and feature/variation descriptors work; `font-display` behaves as `swap`. Metric overrides and `size-adjust` are not supported.
* **Shaping:** complex scripts, bidi, vertical text, variable fonts, OpenType features (`font-variant-*`, `font-feature-settings`), kerning, synthesis. Bitmap colour emoji (PNG glyphs) render.
* **Wrapping:** `text-overflow: ellipsis`, `text-wrap: balance`, `word-break`/`overflow-wrap`, `hyphens: auto` (English, German, French and Spanish; requires `lang`), `-webkit-line-clamp` with `display: -webkit-box`. Standard `line-clamp` and `text-wrap: pretty` are off.
* Glyphs are rendered unhinted with greyscale anti-aliasing.

## Images and resources

Resources load only from three places:

| Source | Rules |
| --- | --- |
| Local files, by relative URL or `file:` | Must be inside the document's **asset root**: the directory of the loaded HTML file, or the `asset_root` you pass when loading. The real path is checked after symlinks are resolved. HTML given as a string without an asset root cannot reference local files. |
| `data:` URLs | Up to 4 MiB |
| `ui4me-res://<bundle>/<path>` | In-memory bundles registered by the backend (`document.register_resources(...)` in Python, `Register-UI4MEResources` in PowerShell; protocol operation `register_resource_bundle`): up to 64 bundles, 16 MiB in total, at most 1 MiB and 256 entries per registration request |

Every other scheme, including `http:` and `https:`, is refused. There is no network setting to turn on.

* **Raster formats:** PNG, JPEG, GIF, WebP and BMP (detected by content, EXIF orientation applied). Animated GIF, WebP and APNG show their first frame. AVIF, TIFF and ICO are not supported, and there is no ICC colour management.
* **`<img>`** supports `srcset`, `sizes`, `<picture>` with `media`/`type`, `object-fit` and `alt` text for broken images; `loading="lazy"` has no effect. The same loader serves CSS `background-image`, `<link rel="stylesheet">` and `@import`.
* **SVG** is static: as an `<img>` or CSS image, and as inline `<svg>` (up to 2,000 elements and 512 KiB of markup, with a limited subset of page CSS such as `fill`, `stroke` and `currentColor`). Not supported: SVG `<text>`, SMIL animation, `foreignObject`, SVGZ, references from an SVG to external files, and per-shape hit testing.
* **Live bitmaps:** `<img src="ui4me-bitmap://preview">` displays RGBA pixels streamed by the backend (up to 4096×4096 per bitmap, 8 bitmaps and 64 MiB per document). See `LiveBitmap` in the [Python API](../reference/python-api.md), `New-UI4MELiveBitmap` in the [PowerShell commands](../reference/powershell-api.md) and the [protocol](../reference/protocol.md#live-bitmaps).
* **Budgets:** an image may decode to at most 16,777,216 pixels; each document has bounded decoded-image and cache memory.

## Desktop attributes

| Attribute | Effect |
| --- | --- |
| `data-ui4me-drop="files text"` | **Opt-in** target for drops from other applications. Tokens: `files`, `text`, `none`. The nearest element carrying the attribute receives the drop if its tokens include the dropped kind; `none`, or a disabled or inert ancestor, rejects it. Without the attribute, nothing accepts drops. The backend gets `drag_feedback` events while hovering and a `drop` event with `paths` or `text` (≤ 1024 local paths or 1 MiB of text). |
| `data-ui4me-drag` | Pressing on this region moves the native window (custom title bars, typically with a window created with `decorations=False` in Python or `-Decorations $false` in PowerShell). |
| `data-ui4me-resize="north-west"` | Pressing on this region resizes the window from that edge: `n`/`north`, `s`/`south`, `e`/`east`, `w`/`west`, `ne`/`north-east`, `nw`/`north-west`, `se`/`south-east`, `sw`/`south-west`. |
| `data-ui4me-no-drag` | Excludes a subtree from drag and resize regions. Buttons, links, form controls and `summary` are never drag regions. |

## Accessibility

The host builds a native accessibility tree from the document (AccessKit; AT-SPI on Linux, UI Automation on Windows). Native HTML semantics map to roles, plus about 60 ARIA roles (buttons, tabs, menus, trees, grids, landmarks, live regions, dialogs, …) and the common ARIA states and properties (`aria-label`, `aria-labelledby`, `aria-describedby`, `aria-expanded`, `aria-checked`, `aria-selected`, `aria-live`, `aria-busy`, `aria-hidden`, `aria-invalid`, …). Names follow a simplified accessible-name computation: `aria-labelledby`, `aria-label`, `<label>`, content, `title`, `placeholder`. Elements that are hidden, `display: none`, `aria-hidden` or inert behind a modal dialog are excluded. Platform-specific gaps are listed under [platforms](../guides/platforms.md#known-limitations).

## Events delivered to the backend

Document events reach Python callbacks (`document.on(...)`) and PowerShell callbacks (`Register-UI4MEDocumentEvent`) as `document.event` notifications:

| Group | `type` values |
| --- | --- |
| Pointer and mouse | `click`, `dblclick`, `auxclick`, `contextmenu`, `mousedown`, `mouseup`, `mousemove`, `mouseover`, `mouseout`, `mouseenter`, `mouseleave`, `pointerdown`, `pointerup`, `pointermove`, `pointercancel`, `gotpointercapture`, `lostpointercapture`, `wheel`, `scroll` |
| Keyboard and editing | `keydown`, `keyup`, `beforeinput`, `input`, `change`, `select`, `selectionchange`, `copy`, `cut`, `paste`, `compositionstart`, `compositionupdate`, `compositionend` |
| Focus | `focus`, `blur`, `focusin`, `focusout` |
| Forms | `submit`, `reset`, `invalid` |
| Dialogs, popovers, details | `toggle`, `beforetoggle`, `cancel`, `close` |
| CSS | `transitionrun`, `transitionstart`, `transitionend`, `transitioncancel`, `animationstart`, `animationiteration`, `animationend`, `animationcancel` |
| Drag and drop | `dragstart`, `dragover`, `drop`, `dragend`, `drag_feedback` |
| UI4ME | `link_activated`, `environment_changed` |

Each event carries `type`, `element` (the `id` of the nearest element with one), `node`, modifier flags and event-specific fields such as `value`, `checked`, `key`, `code`, `x`, `y`, `submitter`, `validity` or `return_value`. Callbacks run after the host has already handled the input. To cancel a default action (for example, a keyboard shortcut, `submit`, a context menu or a dialog `cancel`), register a rule ahead of time with `document.prevent_default(...)` (Python) or `Set-UI4MEPreventDefault` (PowerShell), which send the `set_default_action` operation; it applies without a round trip to the backend.

## Limits

| Item | Limit |
| --- | --- |
| HTML per document load or file | 1 MiB, UTF-8 |
| Nodes / nesting depth | 65,536 / 256 |
| Text per text or value update | 16 KiB |
| Stylesheets added at runtime | 64, each up to 1 MiB |
| Batch operations | 1024 per batch |
| Default-action rules | 256 per document |
| Live documents per host | 16 |
| Protocol message (including results and events) | 2 MiB after JSON encoding |

## Not supported

JavaScript and inline event handlers; frames and embedded objects; network loading of any kind and link navigation; audio and video playback; a canvas drawing API; WebGL/GPU rendering; printing and PDF export (the host reports `printing: false`); Shadow DOM and custom-element behaviour; `contenteditable` rich-text editing; colour and date/time pickers; datalist suggestions; HTML or image clipboard formats; dragging content out to other applications; browser storage, cookies and Web APIs.

### Known rendering gaps

- A `dashed` or `dotted` border on a box with `border-radius` is not drawn (the host logs an unsupported-feature diagnostic). Use a solid border with rounded corners, or a dashed border without them.
- `option:checked` colours in a `<select multiple>` list are ignored; selected rows use the engine's own highlight.

If you put user-provided text into a page, use the text operations (`set_text`, `set_value` in Python; `Set-UI4MEDocumentText`, `Set-UI4MEDocumentValue` in PowerShell); HTML-inserting operations parse markup and should only receive trusted HTML, with user values escaped by `html.escape()` or `[Net.WebUtility]::HtmlEncode()`.
