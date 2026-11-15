# F-01: stack cost per nesting level (Linux, release profile, ui4me-renderer)

Tool: `UI4ME_STACK_MEASURE=div,fieldset,table,flex,contents,grid,mixed cargo test --release -p ui4me-renderer --test stack_depth -- --ignored --nocapture stack_measure`.
Each cell is the smallest thread stack (binary search in child processes, ~1 % resolution) on which
parse + cold render + accessibility tree + element_rect + set_text + second render survives the document
whose deepest node is at DOM depth 64 / 256 (256 = MAX_DEPTH). Per-level = slope between the two.
table counts 4 DOM levels per nesting unit (table/tbody/tr/td); the slope is per DOM level.

## Before (box tree as of c6fd71d)

| kind | min stack @64 (KiB) | min stack @256 (KiB) | per DOM level (KiB) |
|---|---|---|---|
| div | 1535 | 5950 | 23.0 |
| fieldset | 2271 | 9085 | 35.5 |
| table | 1023 | 3839 | 14.7 |
| flex | 1535 | 5982 | 23.2 |
| contents | 863 | 3119 | 11.8 |
| grid | 1535 | 5982 | 23.2 |
| mixed | 1375 | 5214 | 20.0 |

## After (box-tree frames shrunk, commit 'F-01 (RENDER-02): shrink the box-tree recursion frames')

| kind | min stack @64 (KiB) | min stack @256 (KiB) | per DOM level (KiB) |
|---|---|---|---|
| div | 799 | 2943 | 11.2 |
| fieldset | 799 | 2943 | 11.2 |
| table | 623 | 2239 | 8.4 |
| flex | 815 | 2943 | 11.1 |
| contents | 239 | 319 | 0.4 |
| grid | 815 | 2943 | 11.1 |
| mixed | 687 | 2431 | 9.1 |

## Frames on the recursion path (gdb, $sp deltas at overflow, release)

Before, one <div> level = element 1600 + children 3696 + block_boxed 11504 + element_dispatch 6784 = 23.6 KiB;
<fieldset> adds html_rendering::fieldset 12624 (35 KiB/level). BoxStyle is 1448 B and Proto 1640 B; the
functions held several by-value copies across the recursive call.

After, the box tree costs < 1 KiB/level (display:contents, which only exercises the box tree, is 0.4 KiB/level);
the remaining ~11 KiB/level of div/fieldset/flex/grid is `layout::coordinator::host_tree::HostTree::add_node`
(one 11408-byte frame per box level, gdb). That file is outside this workstream's ownership and was not
changed; splitting it the same way is the next step if more headroom is ever needed (roadmap 8.5).

## Budget

ENGINE_STACK_BYTES = 32 MiB (host engine thread and Windows /STACK:33554432). Worst kind at depth 256
after the fix: 2.9 MiB, i.e. 11x headroom on the full stack and 5.6x on the half stack T1 uses.
Before the fix the worst (fieldset) needed 8.9 MiB: more than the Linux 8 MiB main thread (the audit's
fieldset-252 abort) and 9x the 1 MiB Windows main-thread reserve. 32 MiB keeps >= 3.5x headroom even for
the pre-fix fieldset cost, which covers MSVC codegen differences until Windows is measured.

## Baseline renderer at the stacks the 1.0.1 host actually had (half of them, as T1 requires)

```
baseline div depth256 stack=4096KiB: overflowed its stack
baseline fieldset depth256 stack=4096KiB: overflowed its stack
baseline table depth256 stack=4096KiB: PROBE_OK
baseline flex depth256 stack=4096KiB: overflowed its stack
baseline contents depth256 stack=4096KiB: PROBE_OK
baseline div depth256 stack=512KiB: overflowed its stack
baseline fieldset depth256 stack=512KiB: overflowed its stack
baseline table depth256 stack=512KiB: overflowed its stack
baseline flex depth256 stack=512KiB: overflowed its stack
baseline contents depth256 stack=512KiB: overflowed its stack
```

(4096 KiB = half the Linux 8 MiB main thread, 512 KiB = half the Windows 1 MiB reserve.)
