# F-04 / RENDER-01 in-process evidence (T4)

Linux, system fonts (fontconfig; NotoSans-Regular / -SemiBold / -Bold, DejaVu installed).

## Unit test (stable identity across a forced source-cache prune)

    cargo test -p ui4me-renderer --lib font_identity_tests

`system_font_identity_is_stable_across_source_cache_prunes`: shape a weight-600 paragraph, shape 300
regular paragraphs (300 Parley builder calls; Parley prunes files unused for 128 calls), shape the
bold paragraph again; six cycles; assert the published system-font id is unchanged and the
system-font table does not grow.

- Baseline (1e49a6a + shim exposing `system_blobs.len()`): FAILS in cycle 0 —
  `left: [4611686018427387906] right: [4611686018427387904]` (new identity after the prune),
  `raw/baseline-T4-unit.txt`.
- Fixed: passes (also `unused_system_font_entries_are_dropped_and_come_back_under_the_same_identity`,
  `fingerprint_distinguishes_content_and_length`).

## Counting-allocator soak (gate R4a)

    cargo test -p ui4me-renderer --test font_identity_soak -- --nocapture

Issue #2 op stream (`tests/data/issue2/{doc,region}.html`, `.u4-badge { font-weight: 600; }`,
region toggled every 50 ticks, layout every tick via `element_rect`), 2,000 ops warm-up then
20,000 ops (131 insert/remove cycles); live heap sampled right after each removal.

| Build | live heap at op 2,128 | at op 21,888 | growth | gate (< 1 MiB) |
| --- | --- | --- | --- | --- |
| Baseline | 12.7 MiB | 89.1 MiB | **+78.3 MiB** (~600 KiB/cycle) | FAIL (`raw/baseline-T4-soak.txt`) |
| Fixed (c955e44) | 4.58 MiB | 4.67 MiB | **+95 KiB** | PASS |

Debug-build runtime of the soak: 36-85 s depending on machine load.
