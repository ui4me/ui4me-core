# Workstream D (d-interact): evidence

Branch `stab/d-interact`, base `1e49a6a` (audit commit on top of 1.0.1 `c6fd71d`). All runs are
local Linux, headless renderer/host unit tests unless stated. No Windows VM, no desktop session.

## Reproduction against the baseline

Each regression test was run against the tree without its fix (fix files reverted, tests kept;
see the commit messages for which files). Results:

| Finding | Tests | Baseline | After |
| --- | --- | --- | --- |
| F-08 / INTERACT-01 | `document::interact_tests::{tab_to_an_off_screen_button_scrolls_it_into_view, tab_to_an_off_screen_text_input_scrolls_it_into_view, backend_focus_scrolls_the_target_into_view, assistive_focus_scrolls_the_target_into_view, nested_scroll_containers_scroll_innermost_first, focus_inside_a_fixed_box_does_not_scroll_the_page, tab_inside_a_scrolled_modal_dialog_reveals_the_control}` | 7 FAILED | pass |
| F-08 (guard) | `pointer_focus_does_not_scroll` | pass | pass |
| F-08 follow-up | `closing_a_dialog_restores_focus_without_scrolling` | FAILED with reveal on restore | pass |
| F-17 / INTERACT-05 | `enter_on_a_link_clicks_it_and_activates_it`, `enter_on_a_link_honours_a_prevented_click` | 2 FAILED | pass |
| T10 matrix guards | button, submit, checkbox, radio, summary, Space on link, role=button | pass | pass |
| F-17b / INTERACT-06 | `shortcuts_fall_back_to_the_physical_key_on_a_non_latin_layout`, `document_clipboard_shortcuts_fall_back_to_the_physical_key` | 2 FAILED | pass |
| F-17b / INTERACT-07 | `press_on_a_child_and_release_on_the_button_clicks_the_button_once`, `press_and_release_on_different_children_of_one_button_click_it_once`, `press_on_label_text_and_release_on_its_checkbox_toggles_it` | 3 FAILED | pass |
| INTERACT-07 (guard) | `release_outside_the_button_produces_no_click` | pass | pass |
| F-07 / INTERACT-02, -03 (T5) | all 10 of `document::geometry_tests` that compile on the old API (the listbox test needs the new `EditGeometry::to_viewport`) | 10 FAILED (tree with PR #4) | 12 pass |
| F-07 selection | `document_selection_highlight_follows_fixed_and_translated_text` with the old `selection.rs` | FAILED: highlight y -460 vs element y 40 | pass |
| INTERACT-04 (T13) | host `document_native::tests::accessibility_tree_is_built_only_for_an_active_at_and_semantic_changes` | old rule: 100 builds / 100 ticks | 0 builds / 100 ticks without AT |
| INTERACT-10 | `window_focus_loss_cancels_the_pointer_and_releases_capture` | FAILED (no events, capture kept) | pass |

Selected baseline failure messages (T5, tree with PR #4 but without the geometry service):

```
element_rect_and_accessibility_bounds_are_the_transformed_box: (0.0, 0.0, 100.0, 50.0) vs (150.0, -25.0, 200.0, 100.0)
a_textarea_in_a_fixed_box_keeps_its_geometry_after_the_page_scrolls: IME y -350.0 vs 50.0
a_modal_dialog_on_a_scrolled_page_places_the_caret_under_the_click: IME area (240.0, -1011.0, 1.0, 22.0) outside the input at y 189
a_click_on_glyph_k_of_a_translated_input_puts_the_caret_at_k: glyph 0 at x 153 -> caret 15
a_click_in_a_scaled_input_maps_through_the_inverse_transform: glyph 1 -> caret 3
select_options_are_picked_where_they_are_painted: fixed + scrolled picks "one", want "two"
a_range_slider_in_a_translated_box_follows_the_pointer: element x 0.0 vs 200.0
element_rects_are_hit_where_they_are_reported (proptest): translate/scale fixture, centre (80, 330) of the reported rect hits "tall"
```

## Audit probe after the fix

`docs/audit/evidence/scripts/interact-probe/main.rs`, built temporarily as an example of the
renderer crate on this branch (debug build; not committed) and run with
`fixed click transform capture link layout select fixtr tabscroll keys tab`. Compared with the
audit's baseline output (INTERACT.md section 3):

| Probe | Baseline (audit) | After |
| --- | --- | --- |
| `fixed` element_rect y / IME y / a11y runs / caret on line 3 | -350 / -290 / -350..-290 / 31 ("line four") | 50 / 110 (caret at end, same before and after scrolling) / 50..110 / 20 (in "line three") |
| `click` span press, padding release | no click | `click@b` |
| `transform` element_rect, a11y bounds | 0,0,100,50 | 150,-25,200,100 |
| `transform` input click at visual char 5 | caret 16 | caret 5, IME x 200 |
| `select` translated popup, visible option "two" | value "one" | "two" |
| `select` fixed + scrolled, visible option "two" | value "one" | "two" |
| `link` Enter on `<a href>` | no click, no link_activated | `keydown, click, link_activated, keyup` |
| `layout` Russian Ctrl+A / Ctrl+C | nothing | select-all, `copy` |
| `tabscroll` Tab / backend focus / AT focus to y=1524 in a 300 px viewport | scroll 0 | scroll 1248 (button at the bottom edge) |
| `tabscroll` Tab into an `overflow:auto` box | scroll 0 | viewport 1348, box 1424 |
| `capture` focus loss while captured | capture kept, `pointermove@c` | capture `None`, move goes to the hit element |
| `keys`, `tab`, `fixtr` | as audit | unchanged (radio roving, select keys, tab order, fixed-in-transform hit) |

## Accessibility rebuild counter (INTERACT-04)

Host test above, headless host with a document holding a CSS transform animation, a colour
animation and a focused input (caret blink polled):

- 1.0.1 rule (`repaint || a11y.is_none()`): 100 builds in 100 ticks of 16 ms.
- New rule, no AT connected: 0 builds after the document's first tree.
- AT connected: one rebuild on activation (stale cache), 0 for an unchanged document, 1 per
  semantic change (value), 1 for a window title change. A restyling animation still rebuilds
  on its ticks while an AT is connected (not throttled).

The audit measured 18.5 ms per tick for the a11y build at 1,000 list items and 57 ms at 3,000;
without an AT that cost is now gone. No wall-clock measurement was taken here.

## Verification commands

`tools/verify.py --keep-going --jobs 3` on 9e4f9f2: fmt, check, test (renderer 544 passed / 6 ignored,
host 96 passed), headless, renderer, faults PASS; python FAIL only on
`test_checked_manifest_matches_entire_source` (F-09, expected) and
`test_no_references_to_retired_documents`, whose hits are all in the committed `docs/audit/` files
(pre-existing on the base commit, not touched here). Files: `t10-baseline.txt` (interaction tests on
the tree without the F-08/F-17/F-17b fixes), `t5-baseline-failures.txt`, `interact-probe-after.txt`.

Commands:

```
export CARGO_BUILD_JOBS=3
cargo test -p ui4me-renderer --locked -j 3
cargo test -p ui4me-host --locked -j 3
cargo clippy --locked --workspace --all-targets -j 3 -- -D warnings
python3 tools/windows_source_check.py --jobs 3      # Windows clippy -D warnings, all targets: PASS
python3 tools/verify.py --keep-going --jobs 3
```
