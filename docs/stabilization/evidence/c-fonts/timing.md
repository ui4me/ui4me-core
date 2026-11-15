# F-16a / F-16b / RENDER-07 / RENDER-08 timing (release, in-process)

Harness: `regression::append_cliff::append_cliff_timing` (ignored test in this branch).
Document: 2,000 rows `<div class=row><b>userN</b> message N ...</div>` + a status `<p>`, system fonts
(fontconfig, Noto/DejaVu installed), Linux, 16 cores shared with other builds (load avg 4-8).

    cargo test --release -p ui4me-renderer --lib append_cliff_timing -- --ignored --nocapture

Per configuration: cold first render, N unrelated `set_text`+render on the status `<p>`, then append
one row + render, then 5 scroll+render (full frame). Three runs each; raw lines in `raw/`.
"shaped" = `TextServices::paragraphs_shaped()` delta (new work counter).

Baseline = 1e49a6a with a test-only shim (counter + this harness; `raw/` names it), fixed = c955e44.

## Append one row after N unrelated updates (F-16a)

| Viewport | N | Baseline append+render | shaped | Fixed append+render | shaped |
| --- | --- | --- | --- | --- | --- |
| 1280x800 | 7 | 58-60 ms | 1 | 53-57 ms | 1 |
| 1280x800 | 20 | **691-750 ms** | **2001** | **54-55 ms** | **1** |
| 3840x2160 | 7 | 70-76 ms | 1 | 63-66 ms | 1 |
| 3840x2160 | 20 | **661-705 ms** | **2001** | **66-69 ms** | **1** |

With the fix the append pass also replays the unchanged rows (`UI4ME_PROFILE=1`: "replayed 2001",
`ifc_new` 4.7 ms) — on the baseline the same pass reported "replayed 1" (audit RENDER-05).

## Cold first render, 2,001 paragraphs shaped (F-16b / RENDER-06)

| Build | 1280x800 first render |
| --- | --- |
| Baseline | 828-930 ms |
| Memo only (`SYSTEM_GENERIC_LIMIT = usize::MAX`; measured while the two windowed soaks ran, so +5-10 % load) | 728-906 ms |
| Fixed (memo + 32-family truncation) | **273-334 ms** (~3x) |

The memo alone removes the BTreeSet/String churn in `resolve_families`; most of the cost was Parley
resolving the ~160-name list per span and setting it on every query, which only the truncation removes.

## Incremental `set_text` + render (RENDER-07, measured only)

| Viewport | Baseline median | Fixed median |
| --- | --- | --- |
| 1280x800 | 27.7-30.8 ms | 26.3-27.4 ms |
| 3840x2160 | 36.5-41.2 ms | 36.6-38.4 ms |

Breakdown of one fixed 1280x800 pass (`UI4ME_PROFILE=1`, `raw/profile-set_text-fixed.txt`): restyle
1.0, style bridge 2.0 (lang/dir 0.6), box build 3.0, `tree.add` 5.2-6.2 (2,004 host nodes, ifc_new
1.8-2.1, "replayed 1"), build_fragments 3.3-3.6, scene prepare 2.5 + build 2.3 (6,004 items, 1 subtree
reused, 3 built), raster 3.9-4.8 ms. Still O(document) in host-tree rebuild, fragments and scene
preparation, as the audit found; not redesigned here (roadmap Phase 8 / 08-non-recommendations).

## Full frame vs incremental at 4K (RENDER-08, measured only)

| Viewport | scroll+render (full frame) | set_text+render |
| --- | --- | --- |
| 1280x800 | 8.1-8.4 ms | 26.3-27.4 ms |
| 3840x2160 | 19.2-19.9 ms | 36.6-38.4 ms |

The 4K penalty of a tiny `set_text` (+10-11 ms over 1280x800) matches the full-surface conversion /
present the audit describes; damage-limited conversion is not implemented here.
