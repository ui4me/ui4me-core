# INTERACT: input, window management, accessibility and vendored platform patches

> **Coordinator note.** `<scratch>/` paths were session-temporary; the probe source is retained in [../evidence/scripts/interact-probe](../evidence/scripts/interact-probe/).


Audited commit: c6fd71d (1.0.1). Area: INTERACT. Auditor ran only unit tests and headless engine probes. It had no Windows access and drove no desktop. Every Windows statement below comes from reading the code.

## 1. Scope and method

**Read (targeted):**
- `crates/ui4me-renderer/src/interaction/`: `sequences.rs`, `keyboard.rs`, `focus.rs`, `controls.rs` (select/range/wheel), `scroll.rs`, `input_queue.rs`, `dispatch.rs` (capture, listeners), `editing.rs` (shortcut and geometry use), `editing/{ime,clipboard}.rs`, `assistive.rs`, `activation.rs`.
- `crates/ui4me-renderer/src/a11y/{mod,build,metadata}.rs`.
- `document.rs`: `handle_input`, `invoke`, `apply_prevent_rules`, `viewport_rect`, `control_geometry`, `element_content_rect`, `text_lines`, `hit_test`, `render_into`, `poll_resources`, `accessibility_request`.
- `document/{hosted,application_api,drag,editable,selection,scroll_anim}.rs`, where relevant.
- `crates/ui4me-host/src/accessibility.rs` (whole file), `host/native.rs` (whole file), `host/document_native.rs` (whole file), `host/requests.rs::create_window`, `host/mod.rs::{pump,refresh,window_event_message}`.
- `platform/windows.rs::initialize_application`.
- `ui4me-engine-types/src/{input,clipboard}.rs`.
- `docs/guides/platforms.md`, `docs/concepts/html-css-scope.md` (events, links, transform), `third_party/README.md`.

**Vendored diffs** (`git diff <Vendor pristine> HEAD`):
- winit-wayland ba811c3: `data_transfer.rs`, `seat/mod.rs`.
- winit-win32 fda5f5d: `window.rs::request_ime_update`, `data_transfer.rs`, `event_loop.rs::fetch_data_transfer`. Also checked the upstream `WM_IME_*` handlers and the OLE `Drop`/`DragLeave`/`remove_drag` flow.
- softbuffer 5c9dc23: `backends/wayland/mod.rs`.
- accesskit_atspi_common e099b37: `adapter.rs` Drop, `context.rs` app name, `node.rs` states, attributes, relations and active descendant.

**PR #4:** `git diff main...origin/fix/fixed-ancestor-viewport-rect` (44ad791). It was also built and run in a temporary worktree, since removed.

**Commands run:**
- `cargo test -p ui4me-renderer -j 4 --locked -- a11y interaction focus_pointer ime accessib` → 52 passed.
- `cargo test -p ui4me-renderer -j 4 --locked` → **501 passed, 0 failed, 6 ignored** (measurement and fuzz harnesses).
- `cargo test -p ui4me-host -j 4 --locked -- accessib atspi ime drop clipboard` → 19 passed.
- `cargo deny --offline check advisories` (advisory DB of 2026-11-13) → only `unmaintained` informational advisories: `zvariant`, `zvariant_derive`, `zbus_names` (merged into zbus 6) and `ttf-parser`. No vulnerability advisories.

**Headless probe crate:** a separate workspace that uses only the public `ui4me_renderer` API.
- Source: `<scratch>/exp/INTERACT/probe/src/main.rs`.
- Build: `cd .../exp/INTERACT/probe && CARGO_TARGET_DIR=.../exp/INTERACT/target cargo build -j 4 --offline`.
- Run: `.../exp/INTERACT/target/debug/interact-probe <fixed|click|transform|tab|keys|capture|scale|wheel|link|layout|volume|tabscroll|select|fixtr|perf|anim>`.
- The renderer is a non-member path dependency here, so it builds at `opt-level=2` per the copied `[profile.dev.package."*"]`. That is closer to release than the repo's own debug tests.
- Outputs are in `.../exp/INTERACT/probe-run-final.txt`, `probe-perf.txt`, `probe-anim.txt` and `probe-pr4-run.txt`.

## 2. Strengths to preserve

- **Declarative default-action rules** (`Document::set_default_action`, `apply_prevent_rules`). They apply synchronously in the root capture listener with no IPC round trip. A cancelled `keydown` also suppresses the following `Input::Text` (`suppress_next_text`). This is the right answer to "backend cannot preventDefault synchronously", and the docs disclose it.
- **Native accessibility identity layer** (`host/accessibility.rs::TreeState`):
  - Monotonic native IDs per window and epoch-tagged actions.
  - Validation happens before mutation, and only changed nodes are published.
  - The action handler never blocks AT threads (`try_send` plus wake). Lock poisoning is tolerated.
  - The Windows window is created hidden and made visible after `SubclassingAdapter::new`, which is the ordering AccessKit requires.
- **Owner-thread seam for async completions** (`document_native.rs`). Every clipboard, drop or dialog completion carries `(window_id, document_id)` and is revalidated (`native_drop_accepts`), so a replacement cannot retarget a drop.
- **Frameless move/resize** runs synchronously in the press handler (`start_native_chrome_drag`), as Wayland's serial rules require.
- **Keyboard support:**
  - Tab order follows HTML: positive tabindex first, then tree order, skipping disabled controls, `tabindex=-1` and inert content. Shift+Tab wraps.
  - Radio arrow roving works, including wrap-around. So does select keyboard handling (Arrow, End, Alt+Down, F4, Escape, Enter).
  - Checkbox Space, range arrows and PageUp, Shift+wheel to horizontal, and Shift+F10/ContextMenu all work.
  - All verified by probe.
- **UA focus ring:** `:focus-visible { outline: auto }`, with browser-like focus-visible heuristics (`focus.rs::focus_update`).
- **Paste and transfers:** CRLF is normalized on paste (`editing/clipboard.rs:240`). Bounded 1 MiB native transfer reads. Wayland's async selection after the host's own write is deferred up to 1 s (`read_clipboard_now`).
- **Vendored patches are small and well documented.**
  - The win32 IME patch correctly releases the window-state lock before IMM calls. The upstream `WM_IME_*` handlers also take that lock only in temporaries.
  - The win32 DnD patch reads lazily, with no `RefCell` borrow held across the cross-process `GetData`.
  - The softbuffer patch bounds parked buffers to 4 and never blocks. A KDE bug confirms that KWin withholds `wl_buffer.release` for offscreen surfaces (bugs.kde.org 446978), so the patch addresses a real compositor behaviour.
- **The "AT-SPI `scroll_to` is a no-op on static text" note in memory is outdated.** `Document::scroll_element_of` maps text nodes to their parent, with unit tests `a11y_text_tests.rs:214,257`, since 670356f. This has not been re-verified natively.

## 3. Findings

### INTERACT-01: Moving focus never scrolls the focused element into view (Tab, backend `focus`, AT Focus)
- **Classification:** Confirmed defect (reproduced headless).
- **Component:**
  - `interaction/focus.rs::focus_update` (l.258 onward), `keyboard.rs::keydown_default` "Tab" arm (l.190-203).
  - `assistive::focus`, and `Document::focus` (`document.rs:1498`).
- **Evidence:**
  - None of these paths calls any scroll-into-view routine. `reveal_caret` is only used for caret moves inside text controls.
  - Probe `tabscroll`: a 300 px viewport, a button at y=1524 and another inside an `overflow:auto` box.
  - After Tab, Tab, render and advancing the clock, the output is `focused="b" viewport scroll=Ok(0.0) rect(b)=…y: 1524`.
  - The same happens for a text `<input>`, for backend `focus("b")` and for an AccessKit `Action::Focus` request. All of these leave scroll at 0.
- **Reproduction:** `interact-probe tabscroll`.
- **Severity / Likelihood:** High / High (any app whose content exceeds the window).
- **Impact:**
  - Keyboard-only and screen-magnifier users tab to invisible controls, which fails WCAG 2.4.7/2.4.11.
  - Validation focuses the "first invalid control" (`html-css-scope.md:42`), but it can stay off-screen.
  - Typing goes into a field the user cannot see.
- **Root cause:** HTML's focusing steps include "scroll into view (nearest)". That step was never implemented, and the module header of `keyboard.rs` lists related navigation work as not done.
- **Remediation:**
  - In `focus_update`, after `set_focus` succeeds, scroll the new focus target into view with `block: nearest, inline: nearest`, `instant`, unless `FocusSource::Pointer` is set. Pointer focus is already visible, and browsers do not scroll on click focus.
  - Reuse `Document::scroll_into_view_behavior` (`document/scroll_anim.rs:~350`) through a new `DispatchHost::reveal_element(doc, node)` hook, so the interaction core stays document-agnostic.
  - For text controls, reveal the caret after revealing the element.
  - Honour a future `preventScroll`-like flag on the protocol `focus` op, defaulting to scroll.
  - Tests:
    1. Tab to an off-screen button scrolls the viewport to make it fully visible.
    2. A nested `overflow:auto` container scrolls innermost first.
    3. AT Focus scrolls.
    4. Pointer focus does not scroll.
    5. Focus inside a fixed box does not scroll the page (needs INTERACT-02 geometry).
- **Alternatives:** Do it in the host only. That is worse, because injected and AT paths would diverge.
- **Complexity:** S–M. **Confidence:** High.

### INTERACT-02: PR #4 is correct but incomplete. Control, caret, IME and text-run geometry inside `position: fixed` is wrong after outer scrolling
- **Classification:** Confirmed defect (reproduced), plus an assessment of PR #4.
- **Component:** Every geometry path computes `document_rect − Σ scroll_chain(n)` without the fixed-ancestor stop:
  - `document.rs::control_geometry` (l.2137-2169), which feeds text pointer→caret, IME cursor area, `<select>` popup and listbox picking (`controls.rs::select_geometry` l.679, `reveal_listbox_option` l.211) and PageUp/PageDown motion (`editing.rs:537,989,1721`).
  - `element_content_rect` (l.2171-2195), which feeds the range slider (`controls.rs::set_range_from_point` l.1014).
  - `A11yInfo::text_lines` (l.3065-3083).
  - The contenteditable caret (`document/editable.rs::caret_rect` l.629 onward and `reveal_caret` l.649).
  - Document selection geometry (`document/selection.rs:174,629,1068`).
- **Evidence:** Probe `fixed`: a `position:fixed;top:50px` box with a textarea, page scrolled by 400. Results on main and on the PR #4 build:

  | Value | main | PR #4 | Expected |
  | --- | --- | --- | --- |
  | `element_rect` y | -350 | 50 (fixed) | 50 |
  | IME `cursor_area` y | -290 | -290 | 110 |
  | a11y TextRun y | -350…-290 | -350…-290 | 50…110 |
  | Click on visible line 3 (y=100) caret | 31 ("line four") | 31 | ~20 ("line three") |

  Probe `select`: a `<select>` in a fixed box, page scrolled. The popup opens (a11y `expanded:true`). Clicking the visible option "two" leaves the value at "one", both on main and on PR #4.
- **Reproduction:**
  - `interact-probe fixed select` gives the main-tree results.
  - The PR #4 build output is saved in `probe-pr4-run.txt`.
- **PR #4 assessment:**
  - `scroll_chain_until_fixed` is correct for the engine's model. Probe `fixtr` shows the engine paints fixed boxes as viewport-fixed even inside a transformed ancestor, so stopping at `Fixed` stays consistent with hit testing there.
  - Its test is good.
  - It fixes `viewport_rect`, and therefore element a11y bounds, `get_rect` and the AT Click centre.
  - The claimed `text_lines` sibling bug is real. So are four more call sites, which affect user-visible pointer and IME behaviour, not only a11y.
  - Merge PR #4 only together with, or followed by, the shared-helper change below.
- **Severity / Likelihood:** High / Medium-High. Modal `<dialog>`s and fixed headers or side panels with text fields or selects on scrolled pages are common.
- **Impact:**
  - Mouse users cannot pick `<select>` options or place the caret in textareas inside modals on scrolled pages.
  - The CJK IME candidate window is placed off-screen.
  - Screen readers get wrong text-run bounds.
- **Root cause:** Five copies of the "viewport offset of node" computation exist. PR #4 patched one of them.
- **Remediation:**
  - Introduce one `DocumentPipeline::viewport_offset(node) -> (dx, dy)`, or better a `node_to_viewport` transform (see INTERACT-03). It should sum sticky, anchor and scroll offsets up to and including the nearest fixed ancestor.
  - Replace every `for e in scroll_chain(n) { sx += …}` used for geometry with it: `viewport_rect`, `control_geometry`, `element_content_rect`, `text_lines`, `editable::caret_rect`, `selection::content_scroll`.
  - Keep `scroll_chain` for "what scrolls" decisions: wheel, keyboard scroll, scroll-into-view chains.
  - Add a consistency test. For each probed node in fixtures with fixed, sticky and nested scroll before and after scrolling, check that `hit_test(centre(viewport_rect(n)))` is `n` or a descendant, and that the IME cursor area lies inside `viewport_rect(focused)`.
- **Alternatives:** Patch each call site like PR #4. That is error-prone and is how the bug arose.
- **Complexity:** M. **Confidence:** High.

### INTERACT-03: CSS 2D transforms are ignored by all non-scene geometry (caret placement, select picking, IME area, a11y and get_rect bounds)
- **Classification:** Confirmed defect (reproduced).
- **Component:**
  - The same call sites as INTERACT-02.
  - `scene/hit_index.rs` is the only transform-aware path (it stores an `inverse` Affine2D per item).
- **Evidence:**
  - Probe `transform`: a button with `transform: translateX(200px) scale(2)`. The hit test at (250,25) finds it, while `element_rect` and the a11y bounds report `x:0,y:0,w:100,h:50`.
  - An `<input>` inside `translateX(150px)`: clicking visual char 5 (x=198) puts the caret at 16 (end). The IME cursor_area x is 160 in untransformed space; visually it should be about 310.
  - A `<select>` inside `translateX(150px)`: the popup is painted at x 150..300 (3562 non-white pixels there, 0 at x 0..150). Clicking the visible option "two" leaves "one".
  - Docs list 2D `transform` as supported (`html-css-scope.md:77`).
- **Reproduction:** `interact-probe transform select`.
- **Severity / Likelihood:** High / High. `position:fixed; left:50%; top:50%; transform:translate(-50%,-50%)` is the standard way to centre a modal, and slide-in drawers animate `transform`.
- **Impact:**
  - Text editing and select picking break inside centred modals and drawers.
  - The range slider maps pointer to value wrongly (from code, `set_range_from_point`).
  - AT bounds and highlight rectangles are wrong. Windows UIA bounding-rectangle clicks and Narrator focus rectangles miss.
- **Root cause:** The layout fragment tree has no transforms. Paint and hit testing apply them in the scene, but geometry consumers read layout rects.
- **Remediation:**
  - Expose from the committed scene or hit index a per-element `local_to_viewport: Affine2D`, including scroll, sticky, fixed and transforms.
  - Build `viewport_rect` (an axis-aligned bounding box of the transformed rect) and `control_geometry` on it. Callers map points with the inverse: `row_at`, `set_range_from_point` and editing pointer mapping should map `(x, y)` into the control's local space with `inverse.map_point` instead of subtracting an origin.
  - Map the IME cursor area and a11y text-line bounds forward.
  - For a11y, set an AccessKit `transform` on the transformed element's node, or report the transformed bounding box.
  - Tests: the probe scenarios above as renderer unit tests (caret index after a click, the select value after clicking a visible option, `element_rect` equal to the transformed AABB).
- **Alternatives:** Document "no editing inside transformed elements". That is unacceptable given how common centred modals are.
- **Complexity:** M–L. **Confidence:** High.

### INTERACT-04: The full accessibility tree is rebuilt on every repaint tick (CSS animation frames, caret blink, smooth scroll) even when no assistive technology is active
- **Classification:** Reproducible risk. The per-tick cost was measured on the engine API; the host loop path is proven from code and was not run end to end.
- **Component:**
  - Path: `host/document_native.rs::flush_document_refresh` l.381-394 → `if repaint || document.a11y.is_none() { accessibility_tree_titled(...); update_document(...) }`.
  - `repaint` is `needs_repaint()`, which `Document::poll_resources` sets on every animation tick and every caret blink (`document.rs:1620,1626`).
  - `poll_documents` → `document_changed` → `refresh` sets `native_refresh_pending`.
  - `accessibility.rs::TreeState::replace` then clones and diffs every node.
- **Evidence:** Probe `anim`: a spinner with `@keyframes` and N list items; each tick does `advance_clock(16)`, then a flush, then a tree build plus `full_update`. Average per tick:
  - N=300: flush 9.5 ms, a11y 5.2 ms, render 1.4 ms.
  - N=1000: flush 33.7 ms, **a11y 18.5 ms**, render 2.6 ms.
  - N=3000: flush 102 ms, **a11y 57 ms**, render 6.9 ms.
  - The a11y build cost is identical when nothing changed (`perf`: about 100 ms for 15k a11y nodes, `build_again_nochange` the same).
  - The flush cost on a transform-only animation belongs to the RENDER area. It is cross-referenced here, not counted.
- **Reproduction:** `interact-probe anim perf`.
- **Severity / Likelihood:** Medium / High for documents above about 1k elements with any animation or a focused caret.
- **Impact:**
  - 60 Hz animations become impossible in mid-size documents, with CPU pegged and battery drain.
  - The same work runs on Wayland-occluded windows, where `repaint` stays true because the frame was skipped.
  - About 99 % of users run no AT.
- **Root cause:**
  - The a11y build is keyed on "needs repaint" instead of "semantics or geometry changed".
  - The build is unconditional because `ActivationHandler` serves the cached full tree synchronously.
- **Remediation:**
  1. Track a semantic epoch in `Document`, bumped by DOM, attribute, value, selection, focus, layout-geometry and scroll changes but not by caret blink or paint-only animation ticks. Rebuild only when it changed. Throttle geometry-only rebuilds (scroll, animations that move layout) to at most 10 Hz.
  2. Skip building entirely while no AT is active. Track activation: Linux has `DeactivationHandler`; on Windows, the first `request_initial_tree` marks activation. Let `request_initial_tree` build lazily by returning `None`, waking the owner thread and publishing via `update_if_active`; AccessKit supports a `None` initial tree.
  3. Longer term, build incrementally per dirty subtree.
- **Tests:**
  - A host unit test counts `accessibility_tree_titled` calls across 60 animation ticks; expect at most 1 when semantics are unchanged.
  - An adapter-inactive test expects 0 builds.
- **Alternatives:** Only throttle. That is simpler, but caps AT freshness.
- **Complexity:** M. **Confidence:** High (cost measured; path from code).

### INTERACT-05: Enter on a focused `<a href>` does nothing (no `click`, no `link_activated`)
- **Classification:** Confirmed defect (reproduced).
- **Component:** `interaction/keyboard.rs::is_button_like` (l.140-146) limits Enter activation to `button | input | summary`, so `has_activation_behaviour`'s `"a" | "area"` arm is never reached for keys.
- **Evidence:** Probe `link`: `Enter on focused <a href> -> ["focus@l","focusin@l","keydown@l","keyup@l"]`. There is no `click` or `link_activated`. Mouse clicks and AT Click do activate links (`activation.rs:427`).
- **Reproduction:** `interact-probe link`.
- **Severity / Likelihood:** Medium / High for apps using links for navigation (docs advertise `link_activated`, `html-css-scope.md:22`).
- **Impact:** Keyboard-only users cannot follow links (WCAG 2.1.1, Level A).
- **Remediation:**
  - In the Enter arm, also dispatch the synthetic `click` for `a[href]` and `area[href]`. Enter fires on keydown and Space must not activate links, as in browsers.
  - Test: Enter on a focused link yields `click` then `link_activated`. Space on a link yields only scroll and no activation.
- **Complexity:** S. **Confidence:** High.

### INTERACT-06: Editing and clipboard shortcuts match only the logical key, so they fail on non-Latin keyboard layouts
- **Classification:** Confirmed defect in the engine. Platform key reporting is inferred from winit behaviour and was not reproduced natively.
- **Component:**
  - `interaction/editing/clipboard.rs::command_of` (l.51-77).
  - `editing.rs:1496,1507,1511` (Ctrl+A, Z, Y).
  - `host/document_native.rs:686-692` (Ctrl+Shift+Q emergency quit).
  - None of them consults `KeyData.code`.
- **Evidence:** Probe `layout`, an input `hello world`:
  - Latin `key:"a"/"c", code KeyA/KeyC` gives the selection 0..11 and a `copy` event.
  - Russian layout `key:"ф"/"с"` with the same codes gives no selection and no copy.
  - winit derives `logical_key` from the active layout: an xkb keysym on Wayland and the layout's character on Win32.
- **Reproduction:** `interact-probe layout`.
- **Severity / Likelihood:** Medium / High for Cyrillic, Greek, Hebrew, Arabic and other layout users.
- **Impact:** Copy, paste, cut, select-all, undo and redo do not work on those layouts, and neither does the documented emergency quit.
- **Remediation:**
  - Add `fn shortcut_letter(key: &str, code: &str) -> Option<char>`. When Ctrl/Meta is held and `key` is not ASCII, derive the letter from `code` (`KeyA`..`KeyZ` → a..z). This is the Chromium, GTK and Qt approach, applied only when the layout produces no Latin letter.
  - Use it in `command_of`, the select-all, undo and redo checks, and the host quit check.
  - Tests: the Russian-layout key values above.
- **Alternatives:** Have the host send `key_without_modifiers` for a Latin layout. That is not available cross-platform in winit.
- **Complexity:** S. **Confidence:** High (engine) / Medium (exact winit key value on Windows).

### INTERACT-07: `click` requires the press and release targets to be the same node (no common-ancestor rule)
- **Classification:** Confirmed defect (reproduced).
- **Component:** `interaction/sequences.rs:466`: `let same = press.0 == Some(target) && press.1 == Some(p.button);`.
- **Evidence:** Probe `click`: pressing on a `<span>` inside a `<button>` and releasing 16 px away on the button's padding gives `pointerdown@s, mousedown@s, focus@b, …, mouseup@b` with **no click**. Press and release on the span gives a click.
- **Reproduction:** `interact-probe click`.
- **Severity / Likelihood:** Medium / Medium. Jitter between press and release on icon or label children, touchpads, and `<label>text <input></label>` all trigger it.
- **Impact:** Intermittently "dead" buttons and checkboxes.
- **Remediation:**
  - On release, compute the nearest common inclusive ancestor of `press_target` and the release target in the flat tree. If both are connected, dispatch `click`, `auxclick` and `contextmenu` there, per UI Events and Pointer Events.
  - Keep the disabled check on the common ancestor.
  - The dblclick bookkeeping (`last_click`) should key on the click target.
  - Tests: a span→button release produces `click@b`, and a release outside the button produces no click.
- **Complexity:** S. **Confidence:** High.

### INTERACT-08: Input event amplification feeds Issue #1 (a backend disconnect on a saturated outbound queue)
- **Classification:** Reproducible risk. Volume was measured; the disconnect itself is Issue #1 and was not re-run here.
- **Component:**
  - The root capture listener list (`document.rs:345-399`) forwards every event type.
  - `pointermove` and `mousemove` are both sent for every motion, uncoalesced.
  - `host/native.rs:656` sends `window.event resized` for every `SurfaceResized`.
  - `document_keyboard` sends `window.event key` in addition to `keydown`/`keyup`.
  - The outbound queue is `transport.rs:17` `MAX_OUTBOUND_MESSAGES = 128`.
- **Evidence:** Probe `volume`: 100 pointer motions across 10 boxes produce 301 `document.event` notifications (pointermove 100, mousemove 100, over/out/enter/leave 101). So about 43 un-drained motions already exceed 128 messages, which is 40–340 ms of backend stall at 125–1000 Hz mice. A Windows modal resize sends a `resized` notification per `WM_SIZE`.
- **Reproduction:** `interact-probe volume`.
- **Severity / Likelihood:** Medium / Medium. Combined with Issue #1 it is High.
- **Remediation:**
  1. Coalesce `pointermove`/`mousemove` per document per frame or pump, keeping only the latest. Browsers align these to animation frames.
  2. Coalesce `window.event resized` and `scale_changed` per window per pump.
  3. Drop the duplicate `window.event key` while a document is loaded, or make it opt-in.
  4. Longer term, forward only event types that have a backend subscription (the Issue #1 fix direction).
  - Test: 1000 motions in one pump produce at most 1 `pointermove` and at most 1 `mousemove` per target change.
- **Complexity:** M. **Confidence:** High.

### INTERACT-09: On Windows, password fields keep IME composition enabled, so the IME window may show the typed text in clear
- **Classification:** Plausible concern; needs native verification.
- **Component:**
  - `host/document_native.rs::update_ime` and `ime_policy`: password sends `ImeRequest::Enable` with `HIDDEN_TEXT | SENSITIVE_DATA` and `ImePurpose::Password`.
  - The pinned winit-win32 ignores purpose and hints. `platforms.md` and `ime_policy`'s comment say so.
- **Evidence:** From code only. Browsers on Windows disable or scope IME in password fields (input scope `IS_PASSWORD`).
- **Severity / Likelihood:** Low-Medium (privacy) / Low (CJK users typing passwords with the IME on).
- **Remediation:**
  - On `cfg(windows)`, when `ime.password` is set, send `ImeRequest::Disable`, or never enable, so keys arrive as plain `KeyboardInput` text.
  - Document the behaviour.
  - Native test: Microsoft Pinyin active, focus a password input, type → no composition window.
- **Complexity:** S. **Confidence:** Medium.

### INTERACT-10: Pointer capture survives window focus loss while pressed-button state is reset
- **Classification:** Confirmed (reproduced), minor.
- **Component:** `sequences.rs:671-681` (`WindowFocus{false}` resets `PointerState` and `set_active`, but not `capture`). `Document::handle_input_inner` zeroes `pointer_buttons`.
- **Evidence:** Probe `capture`: capture is set during a press, then `Focus(false)` arrives. `pointer_capture()` still returns `Some("c")`, and the following buttonless move is dispatched `pointermove@c`. A range drag does stop correctly.
- **Severity:** Low / Low.
- **Remediation:**
  - On `WindowFocus{false}`, if a capture or a pending capture exists, dispatch `pointercancel` then `lostpointercapture` and clear `capture`, as browsers do when the OS steals the pointer.
  - Test: the probe scenario expects `capture=None` and `lostpointercapture`.
- **Complexity:** S. **Confidence:** High.

### INTERACT-11: Frameless drag regions exclude only a fixed tag list, and double-click timing is hard-coded
- **Classification:** Design gap (from code).
- **Component:**
  - `document/application_api.rs::native_chrome_hit` (l.252-300) excludes only `input|textarea|select|button|a|summary`.
  - `document_native.rs::start_native_chrome_drag` uses 350 ms and 5 px.
- **Impact:**
  - A custom `<div role=button tabindex=0>` close or minimize control, `<label>`, a contenteditable host or a `[tabindex]` element inside `data-ui4me-drag` starts a window move. The document never receives `pointerdown`, so the click is lost.
  - Double-click-to-maximize ignores the OS double-click time (`GetDoubleClickTime`, or the compositor's).
- **Severity:** Low / Medium for custom title bars.
- **Remediation:**
  - Also stop at `is_focusable()` elements, `label`, editing hosts and explicit interactive ARIA roles. `data-ui4me-no-drag` already exists for anything else.
  - Read the platform double-click interval where available.
- **Complexity:** S. **Confidence:** High.

### INTERACT-12: The 250 ms "IME quiet" window can drop the first IME commit or preedit after an interrupted composition
- **Classification:** Plausible concern (from code).
- **Component:**
  - `document_native.rs::update_ime` (l.20-28) and `refresh_document` (l.296-300) set `ime_quiet_until` = now + 250 ms.
  - `document_window_event` drops `Preedit/Commit/DeleteSurrounding` inside the window (l.931-940).
  - Only a `KeyboardInput` press clears it (l.669). With a Wayland input method the IM consumes keys, so no key event reaches the client.
- **Severity:** Low / Low (fast typists clicking into another field mid-composition).
- **Remediation:**
  - Instead of time-based suppression, drop only events that predate the reset. Track an "IME generation", bump it on `Disable`, and accept events once an `Ime::Enabled` arrives for the new generation.
  - Alternatively, clear the quiet window on the first `Ime::Enabled` after re-enable.
- **Complexity:** S–M. **Confidence:** Medium.

### INTERACT-13: Wheel line deltas use a fixed 40 px and ignore the OS wheel-scroll setting
- **Classification:** Design tradeoff / platform divergence.
- **Component:** `document_native.rs` MouseWheel arm (l.918-929): `LineDelta * 40.0`.
- **Impact:**
  - On Windows one notch scrolls 40 px versus Chromium's 100 px (3 lines).
  - `SPI_GETWHEELSCROLLLINES`, which users raise for accessibility, is ignored.
  - On Linux the behaviour is consistent but slower than GTK/Qt.
- **Severity:** Low.
- **Remediation:**
  - Windows: `lines * SPI_GETWHEELSCROLLLINES * ~33 px` (or treat the page setting `-1` as a page).
  - Linux: keep 40–50 px.
  - Disclose in `platforms.md`.
- **Complexity:** S. **Confidence:** High.

### INTERACT-14: Vendored patch review: maintenance notes and small risks
- **Classification:** Maintenance problem / minor risks.
- **winit-wayland:**
  - The MIME table and keyboard-only seat selection devices are correct. Device lifetime keyed on `keyboard_state` and `pointer` is consistent in both removal arms.
  - Both changes are upstreamable as-is: canonical MIME spellings, and "data device per seat with any input capability".
  - The primary-selection device is now also created for keyboard-only seats, but the host never uses primary selection (documented). Harmless.
- **winit-win32 IME (`window.rs:1091-1135`):**
  - Correct. The remaining capability pre-check outside `execute_in_thread` is upstream's racy check.
  - Upstreamable.
- **winit-win32 DnD:**
  - `OleDataObject` AddRef/Release are balanced (Clone re-AddRefs, Drop releases).
  - `fetch_data_transfer` clones out of the `Ref` before the cross-process read.
  - External drops read inside `Drop` before `remove_drag`. Self-drops are deferred upstream, and the host never starts outgoing drags.
  - Risk: `read_unicode_text` and `read_png` copy the whole `HGLOBAL` before the host's 1 MiB `bounded_transfer` limit applies, so an oversized drop costs a full copy. Low.
  - Upstreamable, with moderate effort because it changes `DataObject`'s shape.
- **softbuffer:**
  - Sound. The new buffer gets full-frame content because `render_into` writes every pixel. Parked buffers are destroyed only after release. Memory is bounded at 6 buffers per surface, about 200 MB at 4K.
  - `dispatch_pending` only processes releases that another reader has already pulled off the shared socket. Under winit its event loop does this. A caller without another socket reader would never see releases, so it would get the "holds every buffer" error permanently instead of hanging.
  - Upstream status could not be verified offline. The fix is upstreamable as an opt-in non-blocking mode.
- **accesskit_atspi_common:**
  - The teardown, app-name `OnceLock`, expanded/invalid states, metadata attributes, relations and `ActiveDescendantChanged` patches look correct.
  - `level + 1` matches AccessKit's zero-based level, and the renderer subtracts 1 (`a11y/mod.rs:481-483`).
  - The `container-live` walk is O(depth) per `attributes()` call. Acceptable.
  - The unrelated upstream accesskit_windows writes zero-based `level` into AriaProperties (`node.rs:500`). That is cosmetic and should be reported upstream.
- **Advisories:**
  - `zvariant`, `zvariant_derive` and `zbus_names` are marked unmaintained (merged into zbus 6.0). No vulnerabilities.
  - Moving to zbus 6 is blocked on accesskit_unix, atspi and ashpd releases. Add a `deny.toml` that acknowledges these by ID, so a new real advisory is not masked by permanent noise.
- **Stale docs:**
  - `third_party/README.md` references `docs/BASELINE_0.1_STATUS.md`, which no longer exists.
  - `keyboard.rs`'s header says radio roving and arrow keys in controls are "Not done", but they are implemented (probe `keys`).
- **Severity:** Low. **Complexity:** S. **Confidence:** High.

### INTERACT-15: Undisclosed or under-disclosed platform divergences
- **Classification:** Documentation gap.
- **Undisclosed or under-disclosed in `platforms.md`:**
  - Compositors without server-side decorations (GNOME/Mutter, Weston) get the sctk `FallbackFrame`: a plain bar with no title. winit is built without `wayland-csd-adwaita` (`Cargo.toml` features). Only KDE is mentioned.
  - Windows password fields and the IME (INTERACT-09).
  - Wheel speed and OS setting (INTERACT-13).
  - Shortcut behaviour on non-Latin layouts (INTERACT-06, until fixed).
  - Wayland occluded or minimized windows skip frames until the compositor releases buffers. a11y and IME geometry still update.
  - The keyboard-only seat clipboard requirement and patch appear only in `third_party/README.md`.
  - Fractional scaling (1.25/1.5) on Wayland is not stated as qualified or unqualified. Windows DPI changes are listed as unqualified.
- **Correctly disclosed:** AT-SPI physical-pixel extents (still true: tree `scale` is the root transform and `set_root_window_bounds` uses physical sizes, `accessibility.rs:356-377`), Wayland focus, position and visibility refusals, IME hint scope, no primary selection, Pinyin consuming Tab, and right-click not focusing.
- **Remediation:** Add a "Behaviour differences" list.
- **Severity:** Low. **Complexity:** S.

## 4. Test gaps and highest-value tests to add

1. **Geometry consistency property test** (renderer). Cover INTERACT-02 and 03.
   - Fixtures: fixed, sticky, nested `overflow:auto`, `transform: translate/scale`, a centred modal, before and after scrolling.
   - Assertions:
     - `hit_test(centre(element_rect(n)))` returns `n` or a descendant.
     - `control_geometry` row hit for each visible `<option>` returns that option.
     - A click at the visual position of char k gives caret k.
     - The IME `cursor_area` lies inside the focused control's transformed rect.
     - Every a11y TextRun bound lies inside its field's bound.
2. **Focus reveal tests** (INTERACT-01): Tab, AT Focus and backend focus on off-screen targets; pointer focus does not scroll.
3. **Keyboard activation matrix:** Enter and Space on button, link, summary, checkbox, radio, submit, `role=button`, against browser expectations. Shortcuts on non-Latin `key` values with Latin `code`.
4. **Click target semantics:** press and release on different descendants of one activatable ancestor; a label wrapping an input.
5. **A11y rebuild budget** (host): count tree builds across N animation ticks and caret blinks. Expect 0 while the adapter is inactive and at most 1 when semantics are unchanged while active.
6. **Event-volume budget:** notifications per pointer motion and per resize, after coalescing.
7. **Native, not covered by any automated test today:**
   - An Orca journey: initial focus, Tab to an off-screen control, live-region announcement on `set_text`, combobox active-descendant.
   - NVDA/Narrator on Windows.
   - A Pinyin password field on Windows.
   - KDE at 1.25 and 1.5 fractional scale (crispness, hit testing, AT-SPI extents).
   - A Windows per-monitor DPI move.
   - A Russian layout with Ctrl+C/V on both OSes.
   - The softbuffer occluded-window path on GNOME, where frame callbacks are throttled.
   - Live regions are only unit-tested through AccessKit's name-change rule. `aria-relevant` removals are not announced by AccessKit at all.

## 5. Recommend NOT changing

- **Declarative `set_default_action`** instead of synchronous backend `preventDefault`. A synchronous IPC wait would reintroduce GUI stalls and deadlocks.
- **`TreeState` native-ID remapping and epoch tagging**, the non-blocking `try_send` action path, and creating the window hidden before the Windows adapter.
- **The softbuffer non-blocking patch and the win32 IME and lazy-DnD patches.** They fix real deadlocks and stalls. Keep them until upstream equivalents exist.
- **`start_native_chrome_drag` in the native press handler.** Wayland serials require it.
- **The Wayland clipboard read deferral after own writes**, CRLF normalization and bounded transfers.
- **The PR #4 approach of stopping at the fixed ancestor.** It is consistent with the engine's (non-spec) treatment of fixed inside transforms. Generalize it into one helper; do not revert it.

## 6. Open questions

- **Upstream status** of each vendored patch (newer winit 0.31 betas, softbuffer > 0.4.8) could not be checked offline. Only 0.4.8 and 0.31.0-beta.4 are in the local registry.
- **Exact winit-win32 `logical_key` for Ctrl+letter on a Russian layout.** Expected Cyrillic, but needs a native check.
- **Native AT-SPI ScrollTo on static text** after the 670356f fix (unit-tested only).
- **Fixed inside a transformed ancestor:** the engine treats it as viewport-fixed (probe `fixtr`), unlike CSS Transforms, where the transformed element is the containing block. This is a RENDER-area question. If it is fixed in layout, `scroll_chain_until_fixed` must stop at the fixed box's actual containing block instead.
- **Layout flush cost:** a single `set_text` in a 5000-item list costs about 2.0 s of flush (`probe-perf.txt`), and a transform-only animation costs a full flush per tick. These dominate input latency in large documents and belong to RENDER. Flagged here because they make every interaction in such documents laggy.
- **Whether the Windows modal move/resize loop starves `about_to_wait`** (pump and a11y flush) during long drags. Not verifiable without Windows.
