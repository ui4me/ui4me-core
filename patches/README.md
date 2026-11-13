# Carried renderer dependency sources

These sources were migrated from the supplied UI4ME renderer archive, together
with their patch files. The workspace uses direct `path` dependencies into these
trees. A build does not fetch a second Git copy of Stylo or Taffy. Other registry
dependencies are fixed by the root `Cargo.lock`.

| Tree | Upstream base | Carried changes | License |
| --- | --- | --- | --- |
| `stylo/stylo` | `servo/stylo` revision `37c1ed3b707c8f333304de2107c13b68fbc1057c` | `P-006-1` through `P-006-10`, then `P-006-12` through `P-006-20` (no fabricated number 11) | MPL-2.0 for Stylo; component declarations retain their own terms |
| `taffy/taffy` | `DioxusLabs/taffy` revision `fca167cf659b8bf48019d0dea90624109a78103f` | `P-008-1` through `P-008-6` | MIT, full upstream `LICENSE` retained |

The migrated source and patch files are unchanged by host integration, except
`P-006-20`, added during Core 1.0 desktop qualification (2026-11-13): an element
whose used `color-scheme` is `normal` renders with the light user-agent scheme,
as the reference browser does, so a page that never opted into dark colours no
longer gets light text in default form controls on a dark desktop. The
verification scripts accept a pristine checkout through `UI4ME_UPSTREAM_SOURCE`;
otherwise they search the normal Cargo Git checkout cache. Because direct path
dependencies no longer fetch that cache, a missing pristine checkout is an
explicit verification prerequisite, not a reason to modify the build graph.

The complete MPL-2.0 text is included in `licenses/MPL-2.0.txt` (identical license
text supplied by the cached cssparser 0.38.0 source package). Upstream copyright
headers, component notices and vendored Python notices remain in the sources.
Changes to these upstream files should be carried as reviewable patches, with
an upstream issue/PR recorded when available. No upstream submission is claimed.

The qualified host's Wayland/softbuffer/AccessKit patches are under
`third_party/`. In particular the AccessKit teardown and renderer state mappings
are combined in a single pinned package, not competing Cargo patches.
