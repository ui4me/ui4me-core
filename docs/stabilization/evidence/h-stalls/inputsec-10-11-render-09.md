# INPUTSEC-10, INPUTSEC-11, RENDER-09 (review results)

## INPUTSEC-11 (batch/decode.rs near-miss): FIXED

`Reader::need` computed `self.pos + n` unchecked and the command-length check computed
`start + length` unchecked; `length` and string lengths come from the input. Not reachable on 64-bit
(both are bounded by the batch byte limit), but a wrap would turn an out-of-range field into an
in-range one on a 32-bit target. Both now use `checked_add`. Test
`batch::decode::tests::a_field_length_never_wraps_the_reader_position` (baseline: `attempt to add
with overflow` panic in debug, a wrapped "in range" result in release). All decode tests pass.

## INPUTSEC-10 (Windows DLL search path): findings only (host files belong to workstream G)

Checked at this commit: no `SetDefaultDllDirectories` / `SetDllDirectory` call in
`crates/ui4me-host/src`, no `/DEPENDENTLOADFLAG` in `crates/ui4me-host/build.rs` (it only embeds
`host.rc`/`host.manifest` and sets `/STACK`), no `crt-static` (no `.cargo/config.toml`), so the
published exe imports `VCRUNTIME140.dll` dynamically.

* Static imports are resolved by the loader before `main`; only the PE `DependentLoadFlags`
  (`/DEPENDENTLOADFLAG:0x800` = LOAD_LIBRARY_SEARCH_SYSTEM32, honoured by Windows 10 1607+)
  restricts their search. With the default safe search order the application directory comes first
  (same trust as the exe), then System32; the current directory and PATH are reached only for a DLL
  missing from System32. The KnownDLLs (ole32, user32, kernel32, ...) are never searched for.
  `VCRUNTIME140.dll` is the realistic gap: on a machine without the VC++ redistributable it is
  looked up in the current directory and PATH (and the host fails to start if absent).
* Run-time `LoadLibrary` calls by dependencies (DirectWrite, UI Automation providers, AccessKit,
  winit) follow the process default search order; `SetDefaultDllDirectories(
  LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)` first thing in the Windows
  `main` removes the current directory and PATH from it.

Recommended (owner G, needs Windows verification): (1) `-C target-feature=+crt-static` for the
MSVC host target (drops the VCRUNTIME140 import; check `dumpbin /dependents ui4me-host.exe`);
(2) `cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800` in `build.rs` (check `dumpbin /loadconfig`
shows `Dependent Load Flag 0x800`); (3) `SetDefaultDllDirectories` at the top of the Windows main.
Verification: Process Monitor on the lab VM, launch the host from a directory containing a dummy
`dwmapi.dll` / `VCRUNTIME140.dll`, filter `Path ends with .dll`: no load from the current directory.

## RENDER-09 (insert_html validates the whole document): DEFERRED, measured

Release, `<p id=r{i}>row</p>` rows, 200 x `insert_html("list", "beforeend", "<p id=n{i}>new</p>")`:

| document | insert_html median | set_attribute median (resolve only) |
|---|---|---|
| ~2,000 nodes | 0.20 ms | 8 us |
| ~12,000 nodes | 1.09 ms | 49 us |
| ~40,000 nodes | 3.85 ms | 152 us |

So validation is ~95% of an insert on a large document (a 1,024-op batch at 40k nodes: ~4 s).
Making it proportional to the fragment is not clearly exact against the current semantics, so it
was not changed:

* the full walk counts nodes reachable from the document node (template contents included), which
  the arena's live count does not equal (retained and detached nodes), so the node limit needs a
  maintained reachable count;
* id uniqueness is checked across template contents too, while `element_by_id` (the only id
  lookup, itself a full scan) does not see template contents, so fragment ids cannot be checked
  against the existing document exactly without an id index;
* the walk also re-checks rules for the existing tree; other ops enforce most of them, but not all
  identically (for example ids inside template contents), so "the rest is already valid" is not an
  invariant today.

Plan: add an id -> nodes index (template contents included) and a reachable-node counter to
`ui4me-document`, maintained by insert/remove/attribute changes; then validate the fragment's
nodes, its ids against the index, its height against the container depth and the count against the
counter. That also makes `resolve` (O(n) today) constant time.
