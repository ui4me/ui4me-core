# Core 1.0 capability scope

This is the frozen scope ledger for the completion candidate, not a desktop qualification report. It maps **every one of the 125 supplied audit rows**, retaining each original ID, name, current state, recommendation and source evidence. The machine-readable companion, [CAPABILITY_SCOPE.json](CAPABILITY_SCOPE.json), also preserves every original ownership, primitive, effort and completion-guidance field.

**Implementation, runtime availability and qualification are separate.** The candidate must pass the cloud checks and then the agreed sequential native runs: Linux KDE Wayland with Python first; Windows Server 2022 in a normal-user RDP session second; final regression/stabilization on both. Commit and push follow each successful platform gate. Existing v0.2 evidence is historical evidence, not a qualification result for changed code.

**Windows R05 remains a known qualification blocker:** the independent UI Automation text-selection/read reproducer must pass. NVDA speech working does not substitute for that reproducer. No statement in this ledger marks it fixed or downgrades it to a nonblocking issue.

## Dispositions

| Disposition | Rows |
| --- | ---: |
| Application/UI-library responsibility | 2 |
| Explicit profile boundary | 8 |
| Implemented in candidate; verification pending | 37 |
| Outside frozen Core 1.0 | 61 |
| Retained core | 17 |

**Outside frozen Core 1.0 does not mean “promised in the next release.”** It records a deliberate endpoint. Features can be reconsidered when a real application needs them. Components and virtualization belong in an application/UI library above Core. Basic static SVG stays; advanced SVG text/full cascade/animation, browser networking/JavaScript, printing and parallel rendering/style engines are not introduced.

## Important boundaries

- Standard `line-clamp` is explicitly off and reported; legacy `-webkit-line-clamp` remains supported.
- `anchor-size()` is explicitly excluded, including computed custom-property forms; existing anchor names/insets remain usable.
- `size-adjust`, `ascent-override`, `descent-override` and `line-gap-override` stay reported as unsupported. Font-face feature/variation defaults are implemented separately.
- `UI4ME_GLYPH_HINTING=1` is an optional process setting; unhinted grayscale remains the default.
- Higher precision rasterization requires `--features diagnostic-raster-quality` and `UI4ME_RASTER_QUALITY=high`. A request without the compiled feature returns an explicit error. It is not HDR, ICC management or wide-gamut presentation.
- Registered resource source bytes have a 16 MiB aggregate bound, separate from decoded caches. Purging unused cache entries does not unregister bundles or force a document reload.
- High-contrast preference observation is included; a complete forced-colors rendering palette is not. Windows IME purpose/hints are explicitly unavailable in the pinned backend, while ordinary composition is retained.

## Complete register

The “audit” text below is preserved from the supplied v0.2 assessment and describes that snapshot. Candidate behavior states the actual frozen decision; code/test paths identify the implementation, not a claimed test result. Audit line numbers are historical and can move after edits.

## Document, Python and accessibility

### DOC-01 — DOM mutation and streaming updates

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Text/value/checked/attribute/class changes and insert, replace, remove, move, append-text and trim-children operations are implemented and exposed over IPC.

**Audit baseline:** Text/value/checked/attribute/class changes and insert, replace, remove, move, append-text and trim-children operations are implemented and exposed over IPC.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:490-684,1108-1128; crates/ui4me-host/src/host/document_commands.rs:28-82,248-257,444-523

### DOC-02 — Named Python helpers for existing document operations

**Audit state:** Wrapper only · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Named Python helpers and stable Node objects cover the frozen document protocol, including focus, control selection/edit commands, forms, resources, geometry, styles and inspection. Transport preserves ordered request submission and the existing asynchronous native dispatch contract.

**Audit baseline:** Document.command(op, **fields) already reaches every documented low-level operation. Named convenience methods cover only the common subset.

**Boundary:** Python callbacks cannot retroactively prevent a native default action; use the declarative prevention rules.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** examples/python/application.py:191-222,268-330; crates/ui4me-host/src/host/document_commands.rs:25-310

**Candidate implementation:** [examples/python/application.py](../examples/python/application.py), [examples/python/client.py](../examples/python/client.py), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs).

**Regression/verification entry points:** [tools/test_application.py](../tools/test_application.py), [tools/test_reference_client.py](../tools/test_reference_client.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-03 — Mutation and queries using stable node references

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Generation-checked node references support anonymous-element queries and every supported mutation operation through the atomic NodeOp batch. Stale, removed, wrong-document and replaced-document references are rejected.

**Audit baseline:** Generation-checked node references support inspection of anonymous elements and template content. Mutations and selector result IDs still depend on HTML id attributes.

**Boundary:** Atomicity covers the accepted DOM mutation batch. A larger transaction mixing arbitrary native state, control history and external side effects is outside this scope (DOC-05).

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/document/public_dom.rs:1-7,63-98,240-309; crates/ui4me-renderer/src/document.rs:431-434,1275-1294,2564-2566; crates/ui4me-host/src/host/document_commands.rs:474-490

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/document/public_dom.rs](../crates/ui4me-renderer/src/document/public_dom.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-04 — Clone and template instantiation

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Templates have separate inert DocumentFragments, mutation isolation, serialization and deep clone/import support in the model; template contents are inspectable. Public host commands do not expose clone/instantiate operations.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-document/src/parse/sink.rs:78-86; crates/ui4me-document/src/mutate/insert.rs:39-41,76-78; crates/ui4me-document/src/clone.rs:101-151; crates/ui4me-document/src/serialize.rs:38-39,111; crates/ui4me-renderer/src/document/public_dom.rs:272-273; crates/ui4me-host/src/host/document_commands.rs:25-310

### DOC-05 — Atomic batches including live control state

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Atomic DOM batches with rollback and a 1024-operation limit already work. Value, checkedness and focus are deliberately excluded because interaction state is outside the DOM transaction.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:625-684; crates/ui4me-host/src/host/document_commands.rs:312-418,513-523

### DOC-06 — Dynamic stylesheets and computed-style inspection

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Add, replace, remove and list stylesheets, computed-style queries and explain diagnostics are exposed.

**Audit baseline:** Add, replace, remove and list stylesheets, computed-style queries and explain diagnostics are exposed.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/document/public_dom.rs:169-238,360; crates/ui4me-host/src/host/document_commands.rs:114-145,298-309

### DOC-07 — Element hit testing at a viewport point

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Viewport-point hit testing returns a stable node identity and optional HTML ID for the actual hit element.

**Audit baseline:** Element rectangles, text-range rectangles and text-at-point queries are exposed. Document.element_at already supports native drops but is not a public document command.

**Boundary:** Coordinates are viewport CSS pixels. The result is a document element, not an individual SVG path.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/document_commands.rs:97-110,225-227; crates/ui4me-renderer/src/document/drag.rs:137-139; crates/ui4me-renderer/src/document.rs:1906-1928

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-08 — Blur and focused-element inspection

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Public focus inspection and blur work for ID-addressed and anonymous nodes. Focus results retain document/node generation.

**Audit baseline:** Programmatic focus is exposed; engine blur and focused-element state already exist.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:1400-1405,1617-1623; crates/ui4me-renderer/src/interaction/focus.rs:194-237

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-09 — Programmatic control caret, selection and replacement

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Text controls expose selection/caret inspection and updates with UTF-16 offsets; replacement supports preserve/select/start/end selection modes using the existing editing transaction.

**Audit baseline:** Text controls track UTF-16 selections and support accessibility selection/replacement. The public GetSelection command returns document-selection text rather than an input caret/range.

**Boundary:** Text controls are supported; this does not enable rich contenteditable.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-document/src/control_state.rs:332-364; crates/ui4me-renderer/src/interaction/assistive.rs:145-173; crates/ui4me-renderer/src/document.rs:922-929; crates/ui4me-host/src/host/document_commands.rs:622-627

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/interaction/editing.rs](../crates/ui4me-renderer/src/interaction/editing.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-10 — Undo, redo and edit-menu commands

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Undo/redo, select-all, delete and clipboard edit commands use the existing native editor/history and deferred clipboard service.

**Audit baseline:** Keyboard undo/redo uses a bounded per-control history with coalescing and normal edit events. No direct public editing-command facade is exposed.

**Boundary:** Clipboard is plain text. Clipboard operations can be unavailable or fail at runtime; GUI dispatch does not wait on Python callbacks.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/interaction/editing/history.rs:1-19,102; crates/ui4me-renderer/src/interaction/editing.rs:1395-1399; crates/ui4me-host/src/host/document_commands.rs:25-310

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/interaction/editing.rs](../crates/ui4me-renderer/src/interaction/editing.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-11 — Programmatic reset and request-submit

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Programmatic form reset and request-submit run the same reset, validation and event/default-action pathways as native controls, including externally associated controls.

**Audit baseline:** Reset and submit buttons already invoke form behavior, validation and events. Equivalent programmatic document commands are absent.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/interaction/activation.rs:435-442,500-564; crates/ui4me-document/src/control_values.rs:374; crates/ui4me-host/src/host/document_commands.rs:25-310

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/document/file_input.rs](../crates/ui4me-renderer/src/document/file_input.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-12 — Ordered form-data collection

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Ordered form entries preserve duplicate names, form ownership, selected options, the chosen submitter and selected-file metadata.

**Audit baseline:** Form ownership, current values, selected options, checkedness and chosen files exist. No form-data entry-list command was found.

**Boundary:** This is structured successful-control collection, not network form submission or automatic reading/uploading of file bytes.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-document/src/forms.rs:86-205; crates/ui4me-renderer/src/document.rs:490-551,1204-1250; crates/ui4me-renderer/src/document/file_input.rs:89-102; crates/ui4me-host/src/host/document_commands.rs:25-310

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/document/file_input.rs](../crates/ui4me-renderer/src/document/file_input.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-13 — Custom validity messages

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Custom validity messages participate in validity checks and submission blocking; clearing the custom error restores ordinary validation.

**Audit baseline:** The model stores custom validity messages and includes them in constraint results. Public commands expose get/check validity but not the setter.

**Boundary:** Existing Rust-regex pattern semantics remain a documented browser divergence.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-document/src/control_state.rs:365-370; crates/ui4me-document/src/constraint.rs:104-106; crates/ui4me-host/src/host/document_commands.rs:185-190,643-644

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-14 — Standard desktop form controls

**Audit state:** Available · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Text-like inputs/textarea, checkbox/radio, select/listbox, number/range, labels, details, dialogs and popovers are implemented. Actual constraint code implements minlength and pattern despite stale editing-module comments.

**Audit baseline:** Text-like inputs/textarea, checkbox/radio, select/listbox, number/range, labels, details, dialogs and popovers are implemented. Actual constraint code implements minlength and pattern despite stale editing-module comments.

**Boundary:** Stabilize the existing contract; no replacement subsystem is introduced.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/interaction/controls.rs; crates/ui4me-renderer/src/document.rs:1130-1250; crates/ui4me-document/src/constraint.rs:1-9,154-227; crates/ui4me-renderer/src/interaction/editing.rs:14-17

**Candidate implementation:** [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-renderer/src/interaction/controls.rs](../crates/ui4me-renderer/src/interaction/controls.rs), [crates/ui4me-renderer/src/layout/replaced/controls.rs](../crates/ui4me-renderer/src/layout/replaced/controls.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/controls_tests.rs](../crates/ui4me-renderer/src/document/controls_tests.rs). These are tests to run; their presence is not a pass record.

### DOC-15 — Color and date/time picker input families

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The lower model recognizes the types, but color, date, time, month, week and datetime-local inputs are explicitly rejected by renderer validation.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-document/src/control_semantics.rs:23-86; crates/ui4me-renderer/src/document.rs:2311-2325,2351-2352; crates/ui4me-renderer/src/layout/replaced/controls.rs:39

### DOC-16 — Datalist suggestion behavior

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Datalist structure and change hooks exist, but no suggestion-popup behavior was found. The datalist itself is excluded from the accessibility tree.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-document/src/parse/mod.rs:98-105; crates/ui4me-document/src/hooks.rs:215-218; crates/ui4me-renderer/src/a11y/build.rs:373-390; crates/ui4me-renderer/src/interaction/controls.rs

### DOC-17 — Forward existing edit, selection and mouse-boundary events

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Existing beforeinput/edit/selection/clipboard and mouse-boundary events are forwarded with stable node identity and relevant control state.

**Audit baseline:** The interaction engine generates beforeinput/select/clipboard and mouse-enter/leave/over/out events, but the fixed document forwarding list omits them.

**Boundary:** Native default-action prevention remains preregistered. This does not add a general event subscription/filter subsystem (DOC-20).

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:347-386,1777-1896; crates/ui4me-renderer/src/interaction/pointer.rs:260-297; crates/ui4me-renderer/src/interaction/editing.rs:686; crates/ui4me-renderer/src/interaction/editing/clipboard.rs

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/interaction/sequences.rs](../crates/ui4me-renderer/src/interaction/sequences.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-18 — Context menu, double-click and auxiliary-click events

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Double-click, auxiliary-click and context-menu events include native pointer sequencing and keyboard context-menu requests.

**Audit baseline:** Native pointer buttons are normalized and text editing has double/triple-click selection. General dblclick and auxclick are excluded, and no contextmenu event path was found.

**Boundary:** These are document events for application-rendered menus, not a native menu service.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/interaction/sequences.rs:5-12; crates/ui4me-renderer/src/interaction/editing.rs:3-7; crates/ui4me-host/src/host/document_native.rs:723-746

**Candidate implementation:** [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [crates/ui4me-renderer/src/interaction/sequences.rs](../crates/ui4me-renderer/src/interaction/sequences.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

### DOC-19 — Default-action cancellation without GUI blocking

**Audit state:** Available · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Bounded native rules preregister default prevention. Python event callbacks arrive after native dispatch and cannot retroactively cancel completed actions.

**Audit baseline:** Bounded native rules preregister default prevention. Python event callbacks arrive after native dispatch and cannot retroactively cancel completed actions.

**Boundary:** Stabilize the existing contract; no replacement subsystem is introduced.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:691-755,2544-2554; examples/python/application.py:224-230

**Candidate implementation:** [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs), [crates/ui4me-renderer/src/interaction/dispatch.rs](../crates/ui4me-renderer/src/interaction/dispatch.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/controls_tests.rs](../crates/ui4me-renderer/src/document/controls_tests.rs). These are tests to run; their presence is not a pass record.

### DOC-20 — Host-side subscriptions and event traffic control

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Python subscriptions filter locally while the renderer forwards a fixed event set and the host sends individual notifications.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** examples/python/application.py:224-256; crates/ui4me-renderer/src/document.rs:347-386; crates/ui4me-host/src/host/document_native.rs:118-145

### DOC-21 — Mouse pointer capture

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Mouse pointer capture is exposed with pointer ID 1, pressed-button requirements and cleanup on release/removal.

**Audit baseline:** Mouse pointer capture is exposed with pointer ID 1, pressed-button requirements and cleanup on release/removal.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:1369-1399; crates/ui4me-host/src/host/document_commands.rs:283-290,691-696

### DOC-23 — Standards-based grapheme and word boundaries

**Audit state:** Partial implementation · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Implemented in candidate; verification pending

Unicode grapheme and word movement/deletion use unicode-segmentation boundaries, including reverse movement through flags and ZWJ sequences.

**Audit baseline:** Actual code already implements visual bidi caret motion, selectionchange and constraints despite stale comments. Grapheme boundaries remain a hardcoded approximation and words use simplified character classes.

**Boundary:** UAX29 default word segmentation is not locale-specific dictionary segmentation for every language.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/interaction/editing.rs:14-17,242-340,921-983; crates/ui4me-renderer/src/document/selection.rs:614-618; crates/ui4me-document/src/constraint.rs:154-227

**Candidate implementation:** [crates/ui4me-renderer/src/interaction/editing.rs](../crates/ui4me-renderer/src/interaction/editing.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/interaction/editing.rs](../crates/ui4me-renderer/src/interaction/editing.rs), [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/unicode-segmentation/latest/unicode_segmentation/

### DOC-24 — Timed selection autoscroll

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Pointer movement can autoscroll selection/control content. Continuous timer-driven scrolling with a stationary pointer beyond the edge is not implemented in the inspected paths.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document/selection.rs:621-659,759; crates/ui4me-renderer/src/interaction/editing.rs:14-17; crates/ui4me-renderer/src/document.rs:1469-1486

### DOC-25 — Internal element dragging and full drag semantics

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Basic draggable-element threshold, dragstart/dragover/drop/dragend and cancellation exist using source/target IDs. This is not a full browser DataTransfer implementation.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document/drag.rs:1-7,53-134

### DOC-26 — Native file input and incoming file/text drops

**Audit state:** Available · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. File-input requests/results, multiple/accept handling and stale-target protection are connected. Incoming native file/text drops are reported to the document. The selected-file presentation now shows a basename or count and resets from authoritative selected-file state.

**Audit baseline:** File-input requests/results, multiple/accept handling and stale-target protection are connected. Incoming native file/text drops are reported to the document.

**Boundary:** Stabilize the existing contract; no replacement subsystem is introduced.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/document/file_input.rs:22-102; crates/ui4me-host/src/host/document_native.rs:368-395,854-905

**Candidate implementation:** [crates/ui4me-renderer/src/document/file_input.rs](../crates/ui4me-renderer/src/document/file_input.rs), [crates/ui4me-renderer/src/pipeline/mod.rs](../crates/ui4me-renderer/src/pipeline/mod.rs), [crates/ui4me-renderer/src/layout/box_tree/build.rs](../crates/ui4me-renderer/src/layout/box_tree/build.rs), [crates/ui4me-renderer/src/layout/box_tree/html_rendering.rs](../crates/ui4me-renderer/src/layout/box_tree/html_rendering.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs), [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs). These are tests to run; their presence is not a pass record.

### DOC-27 — Contenteditable and rich HTML clipboard

**Audit state:** Disabled feature · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Substantial inherited editing and rich clipboard code remains. Hosted documents reject active contenteditable including plaintext-only, and rich clipboard commands are excluded.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:2359-2364,2732-2738; crates/ui4me-renderer/src/document/editable.rs; crates/ui4me-renderer/src/document/rich_edit.rs; crates/ui4me-renderer/src/document/rich_clipboard.rs:83-114; crates/ui4me-host/src/host/documents.rs:590-599

### DOC-28 — Native accessibility tree and text/control actions

**Audit state:** Available · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Roles/names/descriptions, states, bounds, hierarchy, control text runs/selections and actions are implemented. Native publication uses stable IDs and changed-node updates. Windows R05 (independent UI Automation selection/read) remains a known open qualification blocker until the local reproducer passes.

**Audit baseline:** Roles/names/descriptions, states, bounds, hierarchy, control text runs/selections and actions are implemented. Native publication uses stable IDs and changed-node updates.

**Boundary:** Stabilize the existing contract; no replacement subsystem is introduced.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/a11y/build.rs:540-699; crates/ui4me-renderer/src/a11y/mod.rs:155-220,375-462; crates/ui4me-renderer/src/document.rs:890-949; crates/ui4me-host/src/host/document_native.rs:285-293

**Candidate implementation:** [crates/ui4me-renderer/src/a11y/mod.rs](../crates/ui4me-renderer/src/a11y/mod.rs), [crates/ui4me-renderer/src/a11y/build.rs](../crates/ui4me-renderer/src/a11y/build.rs), [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/a11y_text_tests.rs](../crates/ui4me-renderer/src/document/a11y_text_tests.rs), [crates/ui4me-renderer/src/a11y/tests.rs](../crates/ui4me-renderer/src/a11y/tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/accesskit/latest/accesskit/struct.Node.html

### DOC-29 — Live regions, busy and atomic announcements

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Accessibility projection carries aria-busy and aria-atomic with implicit atomic status/alert semantics alongside the existing live-region updates.

**Audit baseline:** Alert/status/log roles are mapped, but A11yNode and its adapter do not carry explicit live, busy or atomic properties.

**Boundary:** Announcement timing and actual speech require Orca/NVDA qualification; source metadata alone is not a speech guarantee.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/a11y/build.rs:291-293; crates/ui4me-renderer/src/a11y/mod.rs:155-192,375-462

**Candidate implementation:** [crates/ui4me-renderer/src/a11y/build.rs](../crates/ui4me-renderer/src/a11y/build.rs), [crates/ui4me-renderer/src/a11y/metadata.rs](../crates/ui4me-renderer/src/a11y/metadata.rs), [crates/ui4me-renderer/src/a11y/mod.rs](../crates/ui4me-renderer/src/a11y/mod.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/accesskit/latest/accesskit/struct.Node.html

### DOC-30 — Table/grid/tree metadata and semantic relationships

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Accessibility exports grid/treegrid/gridcell roles, table row/column indices/counts/spans, tree level/set-position metadata, ARIA relationships and selected-option active descendants.

**Audit baseline:** Basic table/row/cell/header/tree roles exist. Exported A11yNode lacks counts/indexes/spans, sort direction, set position/size and active-descendant/controls/owns relationships.

**Boundary:** Qualify actual assistive clients; no claim that every ARIA role or interaction pattern is implemented.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/a11y/build.rs:273-278,297-298; crates/ui4me-renderer/src/a11y/mod.rs:155-192

**Candidate implementation:** [crates/ui4me-renderer/src/a11y/build.rs](../crates/ui4me-renderer/src/a11y/build.rs), [crates/ui4me-renderer/src/a11y/metadata.rs](../crates/ui4me-renderer/src/a11y/metadata.rs), [crates/ui4me-renderer/src/a11y/mod.rs](../crates/ui4me-renderer/src/a11y/mod.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/accesskit/latest/accesskit/struct.Node.html

### DOC-31 — Assistive scroll-into-view action

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Assistive scroll requests retain the requested text-run identity/alignment and reveal it through inner text-control scrolling plus ancestor scrolling without moving focus or selection.

**Audit baseline:** The request decoder recognizes ScrollIntoView but renderer execution is a no-op. It is not advertised in the current A11yAction enum.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/a11y/mod.rs:123-137,591; crates/ui4me-renderer/src/document.rs:940; crates/ui4me-host/src/host/native.rs:270-292; crates/ui4me-host/src/host/document_commands.rs:608-618

**Candidate implementation:** [crates/ui4me-renderer/src/a11y/mod.rs](../crates/ui4me-renderer/src/a11y/mod.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-renderer/src/document/scroll_anim.rs](../crates/ui4me-renderer/src/document/scroll_anim.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py), [examples/python/completion_checks.py](../examples/python/completion_checks.py). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/accesskit/latest/accesskit/struct.Node.html

### DOC-32 — Inspector and diagnostics backend

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. DOM/node/HTML inspection, computed style, explain diagnostics, CSS profile and accessibility-tree commands already provide a substantial inspector backend.

**Audit baseline:** DOM/node/HTML inspection, computed style, explain diagnostics, CSS profile and accessibility-tree commands already provide a substantial inspector backend.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-host/src/host/document_commands.rs:83-145,298-309,727-738; crates/ui4me-renderer/src/document/public_dom.rs

### DOC-33 — Responsive images: srcset, sizes and picture

**Audit state:** Available · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. The renderer selects density and width candidates using sizes, viewport/DPR and preceding picture source media/type attributes. The older density-only request_img/request.rs comments describe a different path; they do not mean width descriptors or picture are missing.

**Audit baseline:** The renderer selects density and width candidates using sizes, viewport/DPR and preceding picture source media/type attributes. The older density-only request_img/request.rs comments describe a different path; they do not mean width descriptors or picture are missing.

**Boundary:** Stabilize the existing contract; no replacement subsystem is introduced.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/paint_extras.rs:989-1008; crates/ui4me-renderer/src/pipeline/resources.rs:256-290,526-546,652-673; crates/ui4me-assets/src/srcset.rs:3-13,545-588,688-714; crates/ui4me-assets/src/lib.rs:488-524

**Candidate implementation:** [crates/ui4me-renderer/src/pipeline/resources.rs](../crates/ui4me-renderer/src/pipeline/resources.rs), [crates/ui4me-assets/src/srcset.rs](../crates/ui4me-assets/src/srcset.rs).

**Regression/verification entry points:** [crates/ui4me-assets/src/srcset.rs](../crates/ui4me-assets/src/srcset.rs). These are tests to run; their presence is not a pass record.

### DOC-34 — Native loading=lazy image scheduling

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The active image path reads src/srcset/sizes and immediately requests the selected resource when looked up. It does not inspect loading=lazy or apply a visibility/proximity-based deferral policy.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/paint_extras.rs:989-1008; crates/ui4me-renderer/src/pipeline/resources.rs:321-345,526-546; crates/ui4me-assets/src/lib.rs:419-449

### DOC-35 — Progress and meter controls

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Progress/meter have actual control recognition, value normalization, layout, paint and accessibility roles/ranges. Indeterminate progress paints a static stripe pattern; meter selects colors from optimum/suboptimal regions.

**Audit baseline:** Progress/meter have actual control recognition, value normalization, layout, paint and accessibility roles/ranges. Indeterminate progress paints a static stripe pattern; meter selects colors from optimum/suboptimal regions.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-document/src/control_semantics.rs:147-163; crates/ui4me-document/src/control_values.rs:259-310; crates/ui4me-renderer/src/pipeline/update.rs:1689-1708; crates/ui4me-renderer/src/layout/box_tree/html_rendering.rs:47-70; crates/ui4me-renderer/src/scene/paint/controls.rs:387-453; crates/ui4me-renderer/src/a11y/build.rs:912-913,1416-1437

### DOC-36 — Output element value/reset and implicit status semantics

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Output participates in form ownership and renders ordinary text, but it is absent from control-state classification and form-reset handling. Its implicit accessibility role is not mapped.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-document/src/forms.rs:20-31; crates/ui4me-document/src/control_semantics.rs:147-163; crates/ui4me-document/src/control_values.rs:374-419; crates/ui4me-renderer/src/a11y/build.rs:885-953; crates/ui4me-renderer/src/layout/box_tree/html_rendering.rs:46-70

### DOC-37 — Shadow DOM and browser custom elements

**Audit state:** Intentional exclusion · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Declarative shadow roots are explicitly refused and remain inert template contents; style adapters return no shadow root. Custom-named tags can be ordinary styled elements, but there is no registry, lifecycle or encapsulated component tree.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-document/src/parse/sink.rs:69-88,294-297; crates/ui4me-document/src/parse/mod.rs:191-220; crates/ui4me-renderer/src/style/adapter/element.rs:524-528; crates/ui4me-renderer/src/layout/box_tree/html_rendering.rs:46-70

## CSS, layout, typography and rasterization

### CSS-02 — Logical CSS resize directions

**Audit state:** Partial implementation · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Implemented in candidate; verification pending

Logical resize inline/block directions map to the correct physical axis in horizontal, vertical and sideways writing modes.

**Audit baseline:** CSS resize works, but inline is mapped unconditionally to horizontal and block to vertical. That mapping ignores supported vertical writing modes.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/document/resize.rs:25-32; crates/ui4me-renderer/src/layout/writing_mode/mod.rs:243-244,272-310

**Candidate implementation:** [crates/ui4me-renderer/src/document/resize.rs](../crates/ui4me-renderer/src/document/resize.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

### CSS-03 — Visible CSS resize affordance

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Resizable elements paint a visible lower-right grip in the same 14 CSS-pixel corner used by input handling; it follows element transforms and disappears when resize is disabled.

**Audit baseline:** A 14 CSS pixel corner hotspot supports dragging, but no resize grip is painted.

**Boundary:** The grip is physical lower-right, consistent with the current input contract; it is not an independently styleable file/control pseudo-element.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/document/resize.rs:1-9,70-81; crates/ui4me-renderer/src/style/profile/table.rs:145

**Candidate implementation:** [crates/ui4me-renderer/src/pipeline/paint_extras.rs](../crates/ui4me-renderer/src/pipeline/paint_extras.rs), [crates/ui4me-renderer/src/scene/inputs.rs](../crates/ui4me-renderer/src/scene/inputs.rs), [crates/ui4me-renderer/src/scene/paint/scrollbar.rs](../crates/ui4me-renderer/src/scene/paint/scrollbar.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

### CSS-04 — High contrast and forced colors

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Explicit profile boundary

Existing media/preference observations remain usable, including OS-07. A complete forced-colors palette and every native high-contrast rendering rule are excluded.

**Audit baseline:** A renderer setter can report high contrast, but Core does not apply forced colors. Its known media list omits contrast/forced-colors and the pinned Servo Device returns ForcedColors::None.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:764-767,804; crates/ui4me-renderer/src/style/profile/filter.rs:84-102; patches/stylo/stylo/style/device/servo.rs:308-309; crates/ui4me-renderer/src/style/profile/table.rs:133

### CSS-05 — Touch and mixed-pointer media capabilities

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Pointer and hover media queries use default capabilities. The pinned default on Linux/Windows is fine pointer plus hover, so ordinary mouse desktops are represented correctly.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/device.rs:86-87; patches/stylo/stylo/style/device/servo.rs:346-371; patches/stylo/stylo/style/servo/media_features.rs:80-101

### CSS-06 — Intrinsic sizing keywords for flex-basis

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

The typed style-to-layout bridge carries min-content, max-content, fit-content (including limits/calcs) and stretch flex bases into the pinned Taffy dimensions.

**Audit baseline:** Auto, content and lengths/percentages are mapped. The UI4ME bridge reports min-content, max-content, fit-content and stretch values unsupported.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs:63-68; crates/ui4me-renderer/src/layout/coordinator/style_view.rs:454-457; crates/ui4me-renderer/src/layout/box_tree/mod.rs:335-341; patches/taffy/taffy/src/style/dimension.rs:414-483

**Candidate implementation:** [crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs](../crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs), [crates/ui4me-renderer/src/layout/coordinator/style_view.rs](../crates/ui4me-renderer/src/layout/coordinator/style_view.rs), [crates/ui4me-renderer/src/layout/box_tree/mod.rs](../crates/ui4me-renderer/src/layout/box_tree/mod.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://github.com/DioxusLabs/taffy/blob/main/CHANGELOG.md

### CSS-07 — Balanced flex wrapping

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

The Stylo balance preference and Taffy balancing feature are enabled. balance/wrap balance and wrap-reverse balance retain their typed semantics through the bridge.

**Audit baseline:** The Taffy flexbox_balance feature and Stylo balance preference are disabled. The typed bridge also rejects wrap_balance.

**Boundary:** This is the pinned experimental Flexbox 2 balance algorithm; browser fixtures must compare a browser version that implements the same feature.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** Cargo.toml:workspace.dependencies.taffy; patches/taffy/taffy/Cargo.toml:35-58; patches/taffy/taffy/src/style/flex.rs:21-24; crates/ui4me-renderer/src/style/profile/table.rs:119; crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs:43-45; crates/ui4me-renderer/src/style/view/layout_groups.rs:57,264

**Candidate implementation:** [crates/ui4me-renderer/src/style/view/layout_groups.rs](../crates/ui4me-renderer/src/style/view/layout_groups.rs), [crates/ui4me-renderer/src/style/profile/table.rs](../crates/ui4me-renderer/src/style/profile/table.rs), [crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs](../crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs), [crates/ui4me-renderer/src/layout/coordinator/style_view.rs](../crates/ui4me-renderer/src/layout/coordinator/style_view.rs), [Cargo.toml](../Cargo.toml).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/crate/taffy/0.15.0/features

### CSS-08 — Implicit @scope roots

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Explicit @scope roots remain usable. Implicit roots stay removed and reported because stylesheet ownership, reparenting and removal invalidation are not carried by this adapter; adding parser acceptance would be incorrect.

**Audit baseline:** Explicit @scope roots work. An implicit @scope block is removed and reported because the stylesheet adapter does not carry its owning style element.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/filter.rs:473-491; crates/ui4me-renderer/src/style/profile/mod.rs:48-50

### CSS-09 — Custom media aliases

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** @custom-media is explicitly off in the feature profile.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:93,188; patches/stylo/stylo/style/stylesheets/media_rule.rs:73-143; patches/stylo/stylo/style/stylesheets/rules_iterator.rs; patches/stylo/stylo/style/queries/condition.rs:809; crates/ui4me-renderer/src/style/profile/filter.rs:84-115

### CSS-10 — Container style queries

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Container size queries work. Style queries evaluate as unknown and are reported.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:146; crates/ui4me-renderer/src/style/profile/filter.rs:449-470; patches/stylo/stylo/style/queries/condition.rs:148-288; patches/stylo/stylo/style/sharing/mod.rs:967-987; crates/ui4me-renderer/src/style/container.rs:1-19

### CSS-11 — Offscreen skipping for content-visibility:auto

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Hidden skips descendants; auto applies containment but never skips content. This is mainly a missing optimization rather than necessarily invalid CSS behavior.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/view/layout_groups.rs:565-578; crates/ui4me-renderer/src/layout/box_tree/build.rs:563; crates/ui4me-renderer/src/document/tests.rs:1652-1686

### CSS-12 — Remembered contain-intrinsic-size auto dimensions

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** An explicit fallback length is consumed, but the previously measured natural size requested by auto is not remembered.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/view/layout_groups.rs:518-535; crates/ui4me-renderer/src/style/profile/table.rs:142-143

### CSS-13 — Anchor-based sizing

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Existing anchor names and anchor-based insets remain usable. anchor-size sizing/margins require typed expressions plus dependency/fallback resolution; they are explicitly excluded, reported (including computed var() forms), and false in supports queries.

**Audit baseline:** Anchor names and anchor-based insets work. anchor-size expressions can compute in Stylo but are mapped to auto or none by UI4ME.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-renderer/src/style/view/groups.rs:837-874; crates/ui4me-renderer/src/layout/coordinator/oof.rs:81-127; crates/ui4me-renderer/src/style/profile/mod.rs:98-101

### CSS-14 — Position-area and placement fallbacks

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** position-area and position-try values are dropped and reported despite partial anchor-positioning support.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:284-285,423-426; crates/ui4me-renderer/src/layout/coordinator/oof.rs; crates/ui4me-renderer/src/style/profile/mod.rs:98-101

### CSS-15 — Content-sized select controls

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

field-sizing:content sizes a dropdown from the selected option and a listbox from all visible rows (including group indentation), subject to normal CSS sizing constraints. Selection/option changes invalidate sizing.

**Audit baseline:** field-sizing:content works for text, number and textarea controls. Select controls are excluded from the content-size path.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/layout/coordinator/host_tree.rs:430-448; crates/ui4me-renderer/src/layout/box_tree/build.rs:2540; crates/ui4me-renderer/src/document/tests.rs:1691

**Candidate implementation:** [crates/ui4me-renderer/src/layout/coordinator/host_tree.rs](../crates/ui4me-renderer/src/layout/coordinator/host_tree.rs), [crates/ui4me-renderer/src/layout/replaced/controls.rs](../crates/ui4me-renderer/src/layout/replaced/controls.rs), [crates/ui4me-renderer/src/layout/box_tree/build.rs](../crates/ui4me-renderer/src/layout/box_tree/build.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

### CSS-16 — Last-baseline alignment

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** UI4ME reports last-baseline and falls back to safe end.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/layout/box_tree/typed_layout.rs:105-116; patches/taffy/taffy/src/tree/layout.rs:310-328; patches/taffy/taffy/src/style/alignment.rs:24-60

**Reference retained from audit:** https://github.com/DioxusLabs/taffy/blob/main/CHANGELOG.md

### CSS-17 — Font-face feature and variation defaults

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Font-face feature/variation defaults travel with face identity. A bounded second shaping pass applies them only to the actual selected font ranges, before element overrides; metric probes use the same defaults.

**Audit baseline:** Per-element font-feature-settings and font-variation-settings reach the shaper. The equivalent descriptors in @font-face are reported unsupported.

**Boundary:** Only documents using nonempty descriptors pay for the extra shaping pass. Face metric overrides and size-adjust remain outside the profile (CSS-18).

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/style/font_faces.rs:194-201; crates/ui4me-renderer/src/text/fonts/features.rs:18-35,53-91; crates/ui4me-renderer/src/layout/box_tree/build.rs:2288-2298

**Candidate implementation:** [crates/ui4me-renderer/src/style/font_faces.rs](../crates/ui4me-renderer/src/style/font_faces.rs), [crates/ui4me-renderer/src/pipeline/font_faces.rs](../crates/ui4me-renderer/src/pipeline/font_faces.rs), [crates/ui4me-assets/src/fonts/faces.rs](../crates/ui4me-assets/src/fonts/faces.rs), [crates/ui4me-renderer/src/text/fonts/faces.rs](../crates/ui4me-renderer/src/text/fonts/faces.rs), [crates/ui4me-renderer/src/text/fonts/environment.rs](../crates/ui4me-renderer/src/text/fonts/environment.rs), [crates/ui4me-renderer/src/text/fonts/features.rs](../crates/ui4me-renderer/src/text/fonts/features.rs), [crates/ui4me-renderer/src/text/paragraph/source.rs](../crates/ui4me-renderer/src/text/paragraph/source.rs), [crates/ui4me-renderer/src/text/metrics.rs](../crates/ui4me-renderer/src/text/metrics.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/parley/latest/parley/style/enum.StyleProperty.html

### CSS-18 — Font-face metric overrides and size adjustment

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Existing font-face loading/matching and Core 1.0 feature/variation defaults remain usable. size-adjust, ascent-override, descent-override and line-gap-override remain reported as unsupported: correct font baselines, struts and scaling require shaping/layout support that the pinned Parley API does not expose.

**Audit baseline:** Stylo parses size-adjust and ascent, descent and line-gap overrides, but UI4ME reports them unsupported.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-renderer/src/style/font_faces.rs:181-192; crates/ui4me-renderer/src/text/metrics.rs; crates/ui4me-renderer/src/text/fonts/environment.rs

**Reference retained from audit:** https://docs.rs/parley/latest/parley/style/enum.StyleProperty.html

### CSS-19 — Named OpenType alternates

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** @font-feature-values rules are removed; font-variant-alternates is not consumed. Explicit OpenType feature tags already work.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/filter.rs:435-447; crates/ui4me-renderer/src/layout/box_tree/build.rs:2303-2309; crates/ui4me-renderer/src/text/fonts/features.rs:83-91

**Reference retained from audit:** https://docs.rs/parley/latest/parley/style/enum.StyleProperty.html

### CSS-20 — Font-size-adjust for fallback readability

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The feature preference is off and the text projection lists font-size-adjust as unconsumed.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:110,271; crates/ui4me-renderer/src/layout/box_tree/build.rs:2303-2309; crates/ui4me-renderer/src/text/metrics.rs

**Reference retained from audit:** https://docs.rs/parley/latest/parley/style/enum.StyleProperty.html

### CSS-21 — Legacy word-break:break-word compatibility

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Legacy word-break:break-word is accepted and normalized to word-break:normal plus overflow-wrap:anywhere, including intrinsic width calculation.

**Audit baseline:** That spelling is disabled, while overflow-wrap:anywhere and break-word, and ordinary word-break values, already work.

**Boundary:** 

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:101,341,395; crates/ui4me-renderer/src/style/view/groups.rs:1267-1275; crates/ui4me-renderer/src/text/paragraph/snap.rs:364,724

**Candidate implementation:** [crates/ui4me-renderer/src/style/profile/table.rs](../crates/ui4me-renderer/src/style/profile/table.rs), [crates/ui4me-renderer/src/style/view/groups.rs](../crates/ui4me-renderer/src/style/view/groups.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://www.w3.org/TR/css-text-3/

### CSS-22 — Language-sensitive text transformations

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Manual Turkish/Azeri/Lithuanian rules exist. Greek accent handling, Dutch IJ and some half-width conversions are explicitly not performed, and source-cluster tracking uses a subset of Unicode rules.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/text/transform.rs:3-23,82-100; crates/ui4me-renderer/Cargo.toml:icu_properties

**Reference retained from audit:** https://docs.rs/icu_casemap/latest/icu_casemap/struct.CaseMapper.html

### CSS-23 — Additional hyphenation dictionaries

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Nine language sets are shipped: en-US, de, fr, es, pt, nl, sk, pl and uk, with region restrictions. Czech is not shipped; older comments naming four languages are stale.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/text/hyphenation.rs:29-97,112-138; crates/ui4me-renderer/src/text/data/hyphenation/NOTICES.txt

**Reference retained from audit:** https://github.com/hyphenation/tex-hyphen

### CSS-24 — Automatic hyphenation in bidi paragraphs

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Automatic hyphenation bypasses whole bidi paragraphs and reports the limitation. Bidi shaping and layout themselves are implemented.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/text/paragraph/hyphen.rs:51-83; crates/ui4me-renderer/src/text/paragraph/frozen.rs

**Reference retained from audit:** https://www.w3.org/TR/css-text-3/

### CSS-25 — Pretty paragraph wrapping

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** text-wrap:pretty is disabled. Bounded balancing of short paragraphs already exists.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:99-100,337-338; crates/ui4me-renderer/src/text/paragraph/snap.rs:409,901; crates/ui4me-renderer/src/style/view/groups.rs:479-484

### CSS-26 — Standard line-clamp coverage

**Audit state:** Partial implementation · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Explicit profile boundary

The profile now reports standard line-clamp as off, rejects it in CSS.supports/@supports, and reports its declaration; the implemented legacy -webkit-line-clamp path stays available.

**Audit baseline:** Legacy WebKit line clamp works under its vertical WebKit box conditions. The standard form is parsed but reported as unconsumed.

**Boundary:** This selected audit row is closed by an explicit profile boundary, not by claiming a new standard-clamp implementation.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/layout/box_tree/build.rs:2270-2275; crates/ui4me-renderer/src/style/profile/table.rs:96,280; crates/ui4me-renderer/src/style/view/groups.rs:479-490

**Candidate implementation:** [crates/ui4me-renderer/src/style/profile/table.rs](../crates/ui4me-renderer/src/style/profile/table.rs), [crates/ui4me-renderer/src/style/profile/filter.rs](../crates/ui4me-renderer/src/style/profile/filter.rs), [crates/ui4me-renderer/src/layout/box_tree/build.rs](../crates/ui4me-renderer/src/layout/box_tree/build.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

### CSS-27 — Ruby annotations and text emphasis

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Ruby layout and emphasis are disabled or unavailable. CJK shaping, fallback, complex-script segmentation and vertical writing already exist.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:130-131,222-226; crates/ui4me-renderer/src/text/paragraph/vertical.rs; crates/ui4me-renderer/Cargo.toml:parley

**Reference retained from audit:** https://github.com/linebender/parley/blob/main/parley_engine/README.md

### CSS-28 — Optional glyph hinting

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

UI4ME_GLYPH_HINTING=1 (or true/on) before process startup enables Vello glyph hinting for normal and synthetic-bold runs. The cached process default remains unhinted grayscale.

**Audit baseline:** Both ordinary and synthetic-bold glyph runs explicitly call hint(false).

**Boundary:** This is process configuration, not a CSS property or per-document switch; it does not enable subpixel color antialiasing.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/raster/lower/vello_cpu.rs:945-1006

**Candidate implementation:** [crates/ui4me-renderer/src/raster/lower/vello_cpu.rs](../crates/ui4me-renderer/src/raster/lower/vello_cpu.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://skia.googlesource.com/external/github.com/linebender/vello/+/da3f40e73685e9fdf12f0307d8e6d3f8c93dbbd1/glifo/src/glyph.rs

### CSS-29 — PNG-backed bitmap glyph rendering

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

The Vello PNG feature is enabled for embedded PNG bitmap glyphs. An original tiny sbix font tests red PNG pixels against its black outline fallback.

**Audit baseline:** The workspace disables Vello CPU default features and enables only std, text and u8_pipeline; png is absent. Ordinary image decoding is a separate path.

**Boundary:** This enables the existing PNG glyph path; it is not a guarantee that every installed emoji font format or every emoji sequence is supported.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** Cargo.toml:workspace.dependencies.vello_cpu; crates/ui4me-renderer/src/raster/lower/vello_cpu.rs:984-995

**Candidate implementation:** [Cargo.toml](../Cargo.toml), [crates/ui4me-renderer/src/raster/lower/vello_cpu.rs](../crates/ui4me-renderer/src/raster/lower/vello_cpu.rs), [crates/ui4me-renderer/test-data/fonts/bitmap/UI4MEBitmapTest.ttf](../crates/ui4me-renderer/test-data/fonts/bitmap/UI4MEBitmapTest.ttf), [crates/ui4me-renderer/test-data/fonts/bitmap/make_fixture.py](../crates/ui4me-renderer/test-data/fonts/bitmap/make_fixture.py).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/vello_cpu/0.3.0/vello_cpu/

### CSS-30 — Higher-precision snapshot rasterization

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Build with diagnostic-raster-quality and set UI4ME_RASTER_QUALITY=high before startup to select the f32 quality pipeline consistently for primary rendering, offscreen masks, filters and replays. Default builds/rendering stay u8.

**Audit baseline:** Only the u8 pipeline is compiled and render calls select OptimizeSpeed.

**Boundary:** Requesting high quality without the build feature returns an explicit raster UNSUPPORTED_FEATURE error. Output remains RGBA8/sRGB; this does not add HDR, wide-gamut presentation or ICC color management.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** Cargo.toml:workspace.dependencies.vello_cpu; crates/ui4me-renderer/src/raster/lower/vello_cpu.rs:833,1142,1796

**Candidate implementation:** [crates/ui4me-renderer/Cargo.toml](../crates/ui4me-renderer/Cargo.toml), [crates/ui4me-host/Cargo.toml](../crates/ui4me-host/Cargo.toml), [crates/ui4me-renderer/src/raster/lower/vello_cpu.rs](../crates/ui4me-renderer/src/raster/lower/vello_cpu.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/vello_cpu/0.3.0/vello_cpu/

### CSS-31 — Blurred inset shadows with nonuniform radii

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Blurred inset shadows use the exact per-corner/elliptical hole as a Gaussian-filtered complement clipped to the padding shape, instead of dropping the shadow.

**Audit baseline:** Generalized outer shadows use a blur layer. Blurred inset shadows on nonuniform or elliptical corners are omitted with a diagnostic.

**Boundary:** Existing raster/filter budgets and blur limits still apply.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-renderer/src/raster/lower/vello_cpu.rs:2034-2045,2060-2117

**Candidate implementation:** [crates/ui4me-renderer/src/raster/lower/vello_cpu.rs](../crates/ui4me-renderer/src/raster/lower/vello_cpu.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/pipeline/core1_tests.rs](../crates/ui4me-renderer/src/pipeline/core1_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://docs.rs/vello_cpu/0.3.0/vello_cpu/

### CSS-32 — Multithreaded CPU rendering

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The renderer is deliberately single-threaded and the dependency multithreading feature is not enabled.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** Cargo.toml:workspace.dependencies.vello_cpu; crates/ui4me-renderer/src/raster/lower/vello_cpu.rs

**Reference retained from audit:** https://docs.rs/vello_cpu/0.3.0/vello_cpu/

### CSS-33 — Parallel Stylo traversal

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Layout thread count is zero and style traversal passes no pool. Adapter safety comments rely on sequential execution.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:157-174; crates/ui4me-renderer/src/style/traverse.rs:5,80,793

### CSS-34 — True 3D CSS transforms and perspective

**Audit state:** Intentional exclusion · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Stylo can express 3D values, but UI4ME's scene is two-dimensional and reports unsupported nonplanar transforms.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/paint_extras.rs:458-548,875-890

### CSS-35 — Modern layout, selectors and responsive CSS

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. The implementation already includes block/inline, flex, grid, tables, floats, subgrid, media and size-container queries, containment, :has, explicit @scope and modern cascade facilities. Source presence and inherited tests do not replace pending native v0.2 qualification.

**Audit baseline:** The implementation already includes block/inline, flex, grid, tables, floats, subgrid, media and size-container queries, containment, :has, explicit @scope and modern cascade facilities. Source presence and inherited tests do not replace pending native v0.2 qualification.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** Cargo.toml:workspace.dependencies.stylo,taffy; crates/ui4me-renderer/src/style/profile/table.rs:57-151; crates/ui4me-renderer/src/style/profile/mod.rs:36-112; crates/ui4me-renderer/src/layout/coordinator/style_view.rs:487-490; docs/STATUS.md

**Reference retained from audit:** https://github.com/DioxusLabs/taffy/blob/main/CHANGELOG.md

### CSS-36 — Complex shaping, variable fonts and text layout

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Complex-script support is enabled; system font discovery, fallback, bidi, vertical text, variation axes, many OpenType features, text balancing and several hyphenation dictionaries are already implemented.

**Audit baseline:** Complex-script support is enabled; system font discovery, fallback, bidi, vertical text, variation axes, many OpenType features, text balancing and several hyphenation dictionaries are already implemented.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/Cargo.toml:parley,fontique,skrifa,icu_properties; crates/ui4me-renderer/src/text/fonts/features.rs:18-91; crates/ui4me-renderer/src/text/paragraph/vertical.rs; crates/ui4me-renderer/src/text/hyphenation.rs:29-97

**Reference retained from audit:** https://github.com/linebender/parley/blob/main/parley_engine/README.md

### CSS-37 — CPU effects and document-driven CSS animation

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Gradients, masks, basic-shape clipping, background-clip:text, shadows, filter/backdrop-filter functions, transitions, keyframes and starting styles already have implementation paths. Full filter URLs, SVG semantics and other excluded values remain distinct gaps.

**Audit baseline:** Gradients, masks, basic-shape clipping, background-clip:text, shadows, filter/backdrop-filter functions, transitions, keyframes and starting styles already have implementation paths. Full filter URLs, SVG semantics and other excluded values remain distinct gaps.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-renderer/src/style/profile/table.rs:85-112,189-205,245-249; crates/ui4me-renderer/src/style/profile/mod.rs:36-112; crates/ui4me-renderer/src/document/css_animation.rs; crates/ui4me-renderer/src/raster/lower/vello_cpu.rs

**Reference retained from audit:** https://docs.rs/vello_cpu/0.3.0/vello_cpu/

## Native host and operating-system integration

### OS-01 — Configurable minimum and maximum window size

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Window create/set accept paired minimum and maximum logical sizes, validated atomically (1..8192 px and min <= max); paired null clears the constraint.

**Audit baseline:** window.create accepts logical sizes from 160x120 through 8192x8192, but every native window inherits the diagnostic surface's fixed 480x320 minimum. No public min/max constraint fields exist. The diagnostic minimum is documented, so this is not automatically a broken v1 contract.

**Boundary:** Native application of constraints still needs desktop qualification on both target platforms.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:19-25,38-72,321-327; docs/PROTOCOL.md:117-128; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:204-218,1006-1025

**Candidate implementation:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-02 — Observed window state and richer state queries

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Window queries separate requested state from observed focus/minimize/maximize/fullscreen/resizable/visible/theme/position state. Unknown observations are null.

**Audit baseline:** window.set already requests visibility, resizability, minimization, maximization, borderless fullscreen, focus, and Windows placement. window.get reports a narrower set: title, size, visibility, native presence, scale, physical size, and closing. Focus events are emitted. Wayland placement and unminimization are explicitly rejected; native requests are documented as subject to compositor policy.

**Boundary:** Wayland position/minimized/visible and headless OS observations remain unknown where the backend cannot observe them. The pinned Wayland hide/show and focus setters are no-ops, so Linux native visible:false at creation, set-visible and focus requests return unsupported; headless request-state simulation and Windows behavior are distinct. Requested state is never presented as observed success.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:59-72,401-412,433-479; crates/ui4me-host/src/host/document_native.rs:697-705; docs/PROTOCOL.md:121-128; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1097-1219,1446

**Candidate implementation:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs), [crates/ui4me-host/src/host/native.rs](../crates/ui4me-host/src/host/native.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-03 — Monitor inventory and explicit fullscreen monitor selection

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The public contract reports per-window scale and physical size, and fullscreen always uses Borderless(None). There is no monitor-list or current-monitor service. Linux is built for Wayland, not X11.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:401-412,463-470; crates/ui4me-host/Cargo.toml:27-28; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1611-1635; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/event_loop/mod.rs:148-158

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-04 — Decoration controls and native move/resize actions

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Window creation exposes title, dimensions, visibility, and resizability, but not decoration flags, enabled caption buttons, native drag-to-move, drag-to-resize, or the OS window menu.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:38-72,321-327; crates/ui4me-host/src/host/document_native.rs:707-753; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1141-1149,1230,1575-1598

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-05 — Request user attention

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Attention requests accept critical/informational/null with explicit runtime support checks; results never claim the compositor actually presented an attention indicator.

**Audit baseline:** The host can request focus, but there is no separate non-focus attention request for a completed task or an urgent window.

**Boundary:** Wayland requires xdg-activation and explicitly rejects cancellation when it cannot be performed; compositor policy determines attention presentation.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:59-72,445-479; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1450-1461

**Candidate implementation:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs), [crates/ui4me-host/src/host/native.rs](../crates/ui4me-host/src/host/native.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-06 — Per-window or runtime window icons

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** An optional manifest app icon is loaded once at startup and assigned to each created window. There is no public per-window icon override or runtime update.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/config.rs:26-33; crates/ui4me-host/src/host/mod.rs:303; crates/ui4me-host/src/host/requests.rs:321-327; third_party/winit-wayland-0.31.0-beta.4/src/window/common.rs:184-187; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1265

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-07 — Track OS color scheme, reduced motion, and high contrast

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

A Linux appearance-portal observer and Windows registry/SPI/WM_SETTINGCHANGE observer track color scheme, reduced motion and contrast off the GUI thread with coalesced revisions; application overrides remain authoritative.

**Audit baseline:** App color-scheme and reduced-motion overrides work. ThemeChanged is forwarded, and document initialization reads window.theme(); the pinned Wayland getter only returns overrides. Reduced motion currently resolves from the app override or false, with no system field. A renderer high-contrast setter exists but has no native host caller. The public docs accurately state that system removes the override and may fall back to a default.

**Boundary:** Missing observations are null, not false. Observing high contrast does not implement a complete forced-colors CSS palette (CSS-04).

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/Cargo.toml:44; crates/ui4me-host/src/host/documents.rs:280-286; crates/ui4me-host/src/host/document_native.rs:670-695; crates/ui4me-renderer/src/document.rs:760-804; docs/PROTOCOL_DOCUMENTS.md:214-218; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ashpd-0.13.13/src/desktop/settings.rs:294-363; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:1476-1484

**Candidate implementation:** [crates/ui4me-host/src/platform/preferences.rs](../crates/ui4me-host/src/platform/preferences.rs), [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/platform/preferences.rs](../crates/ui4me-host/src/platform/preferences.rs), [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html

### OS-08 — Plain-text clipboard access and editing integration

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. clipboard.write_text and asynchronous clipboard.read_text are public. The host enforces a 1 MiB write bound, transfer deadlines, cancellation, and document generation checks, and bridges renderer copy/cut/paste requests to native data transfer. Linux ownership depends on a real focused input context; this is already acknowledged by the protocol.

**Audit baseline:** clipboard.write_text and asynchronous clipboard.read_text are public. The host enforces a 1 MiB write bound, transfer deadlines, cancellation, and document generation checks, and bridges renderer copy/cut/paste requests to native data transfer. Linux ownership depends on a real focused input context; this is already acknowledged by the protocol.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:234-247,268-278; crates/ui4me-host/src/host/native.rs:61-138; crates/ui4me-host/src/host/document_native.rs:321-367,401-420; docs/PROTOCOL.md:174-176,294-295

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/data_transfer/index.html

### OS-09 — HTML, image, URI, and other richer clipboard formats

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The public clipboard profile intentionally exposes text only. Rich clipboard contents are neither represented in the RPC nor connected to editor/default paste behavior.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:75-83,234-247; crates/ui4me-host/src/host/native.rs:61-138; docs/PROTOCOL.md:293-295; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/data_transfer.rs:125-158,459-499

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/data_transfer/index.html

### OS-10 — Linux primary selection and middle-click paste

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The host exposes the ordinary text clipboard and maps middle-button input, but does not expose or synchronize a separate primary-selection buffer.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/native.rs:61-138; crates/ui4me-host/src/host/document_native.rs:730-736; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/event_loop/mod.rs:355-368; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/event_loop/primary_selection.rs

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/event_loop/trait.ActiveEventLoop.html

### OS-11 — Element-level incoming drop policy and drag feedback

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Element-level incoming file/text drop policy uses data-ui4me-drop, gives Copy/reject feedback, and retains/revalidates the original document and node after asynchronous transfers.

**Audit baseline:** Incoming native text and file drops work. The host accepts Copy for text or URI offers at window level, tracks the pointer, and emits the completed drop to the hit element. File drops are limited to local Unicode paths and at most 1024 entries. There is no public per-element preflight policy, live enter/over/leave contract, or negotiated move/link behavior.

**Boundary:** Transfers are bounded to 1MiB text / 1024 paths. This does not enable outgoing native drags or general DataTransfer browser semantics.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/native.rs:728-773; crates/ui4me-host/src/host/document_native.rs:836-907; docs/PROTOCOL.md:295-296

**Candidate implementation:** [crates/ui4me-host/src/host/native.rs](../crates/ui4me-host/src/host/native.rs), [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs).

**Regression/verification entry points:** [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/event_loop/trait.ActiveEventLoop.html

### OS-12 — Outgoing native drags

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Incoming drop handling exists, but the public protocol and document actions do not initiate native drags out to other applications.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:132-284; crates/ui4me-host/src/host/native.rs:728-795; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/event_loop/mod.rs:271-324

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/event_loop/trait.ActiveEventLoop.html

### OS-13 — IME purpose and input hints beyond ordinary/password text

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

IME purpose/hint mapping accounts for inputmode/type, password/PIN, email/URL/number/phone/date/time, multiline and completion/spellcheck hints.

**Audit baseline:** Composition, caret geometry, surrounding text, surrounding deletion, focus changes, and password-sensitive hints are integrated. The host currently selects only Normal or Password purpose; it does not forward richer field intent such as email, URL, or numeric input.

**Boundary:** The pinned Windows winit backend cannot apply purpose/hints: that capability is explicitly false there. Normal Windows composition remains part of the retained core; native IME behavior must be qualified.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/document_native.rs:17-76,769-834

**Candidate implementation:** [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs), [crates/ui4me-renderer/src/document.rs](../crates/ui4me-renderer/src/document.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/ime/index.html

### OS-14 — Multi-contact touch, pen information, and native gestures

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The host maps primary pointer input into mouse-like renderer events, supports mouse buttons and wheel scrolling, and discards non-primary pointers. Pointer source, contact IDs, pressure/tilt information, and gesture identity are not part of the public document input contract. Primary touch may therefore produce basic pointer behavior, but that is not full touch or pen support.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/document_native.rs:707-767; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/event.rs:230-455,550-620

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/event/enum.WindowEvent.html

### OS-15 — Open files, save file, and choose folder

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. All three native dialog methods are public with window parenting, title, initial directory/name where applicable, extension filters, multiple-open selection, cancellation, and lifecycle checks. Linux uses a custom zbus FileChooser implementation plus ashpd Wayland parent export; Windows uses COM IFileDialog. The project does not depend on rfd.

**Audit baseline:** All three native dialog methods are public with window parenting, title, initial directory/name where applicable, extension filters, multiple-open selection, cancellation, and lifecycle checks. Linux uses a custom zbus FileChooser implementation plus ashpd Wayland parent export; Windows uses COM IFileDialog. The project does not depend on rfd.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** crates/ui4me-host/Cargo.toml:41-51; crates/ui4me-host/src/host/requests.rs:95-111,249-251,536-575; crates/ui4me-host/src/platform/dialogs_linux.rs:48-58,123-184; crates/ui4me-host/src/platform/dialogs_windows.rs:241-318

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html

### OS-16 — Parent-aware external open and reveal-in-file-manager

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

External-open accepts an optional originating window and uses the parent-capable OpenURI/Win32 path; chooser operations retain cancellation. Reveal uses FileManager1.ShowItems or SHOpenFolderAndSelectItems.

**Audit baseline:** desktop.open_external already opens approved URI/path targets through Linux OpenURI/OpenFile/OpenDirectory or Windows ShellExecuteW. It has no window_id: Linux sends an empty parent and Windows a null HWND. It reports OS handoff acceptance, not application completion. There is no separate reveal-and-select-file command.

**Boundary:** Reveal reports parent_forwarded:false because both reveal APIs lack an owner argument. Desktop/window-manager behavior is a native qualification item.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:85-88,219-232; crates/ui4me-host/src/platform/linux.rs:736-772; crates/ui4me-host/src/platform/windows.rs:987-1020; crates/ui4me-host/src/platform/dialogs_linux.rs:48-58; docs/PROTOCOL.md:110

**Candidate implementation:** [crates/ui4me-host/src/platform/external.rs](../crates/ui4me-host/src/platform/external.rs), [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/platform/external.rs](../crates/ui4me-host/src/platform/external.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html

### OS-17 — Owned popup, utility, child, and custom modal windows

**Audit state:** Intentional exclusion · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The public protocol explicitly excludes raw OS handles and arbitrary custom child/modal window classes. The generic factory creates top-level windows; native file dialogs already implement their own ownership and modality.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** docs/PROTOCOL.md:130-131; crates/ui4me-host/src/host/requests.rs:38-51,321-335; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-core-0.31.0-beta.4/src/window.rs:60-70,477-531

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/index.html

### OS-18 — Always-on-top window level

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Window level is not public. Although winit has a setter, the vendored Wayland implementation's set_window_level is an empty method, so enabling the field uniformly would manufacture success on Linux.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:38-72; third_party/winit-wayland-0.31.0-beta.4/src/window/common.rs:181-182

**Reference retained from audit:** https://rust-windowing.github.io/winit/winit/window/trait.Window.html

### OS-19 — Transparent windows, shaped native surfaces, and desktop blur

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The renderer composites its RGBA frame onto white before producing the host buffer. The pinned softbuffer public pixel contract is 0x00RRGGBB, with the highest byte required to be zero. The current presentation path therefore does not preserve window alpha.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:1435-1444; third_party/softbuffer-0.4.8/src/lib.rs:172-183; crates/ui4me-host/src/host/native.rs:439-456

**Reference retained from audit:** https://github.com/rust-windowing/softbuffer

### OS-20 — Native application and context menus

**Audit state:** New subsystem · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** There is no native menu model, command routing, accelerator registration, or platform menu lifecycle in the public host contract. Document-rendered menus can instead be built using the UI library and existing events.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:132-284; crates/ui4me-host/Cargo.toml:27-51

**Reference retained from audit:** https://github.com/tauri-apps/muda

### OS-21 — System tray icon and running without open windows

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Capabilities explicitly report tray=false. app.ready requires at least one window, and existing shutdown logic assumes window-based application lifetime. There is no background-mode policy or tray event API.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/mod.rs:782-789,813-819; crates/ui4me-host/src/host/requests.rs:152-161; crates/ui4me-host/src/config.rs:57-71

**Reference retained from audit:** https://github.com/tauri-apps/tray-icon/blob/dev/README.md

### OS-22 — OS notifications with activation actions

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The host has JSON-RPC notifications but no OS notification service. ashpd is present with its notification feature disabled; current Windows crate features do not provide an already-wired toast implementation.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/Cargo.toml:42-51; crates/ui4me-host/src/host/requests.rs:132-284; crates/ui4me-host/src/host/mod.rs:782-789; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ashpd-0.13.13/Cargo.toml:151

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Notification.html

### OS-23 — Keyboard shortcuts while the application is unfocused

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Window keyboard input and an emergency in-window Ctrl+Shift+Q path exist. They are not system-wide shortcut registration, and no public global-shortcut service exists.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/document_native.rs:569-635; crates/ui4me-host/src/host/requests.rs:132-284; crates/ui4me-host/Cargo.toml:44,51; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ashpd-0.13.13/Cargo.toml:129

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html

### OS-24 — File associations, URI schemes, and richer launcher metadata

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The project already installs/removes managed launchers, establishes app identity, and forwards launch args/cwd plus optional activation token to an existing instance. The Linux desktop entry has a fixed Development category and no MimeType or file/URI argument expansion; there is no general association registration contract.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** tools/create_launcher.py:57-66,114-205; crates/ui4me-host/src/config.rs:26-33; crates/ui4me-host/src/platform/windows.rs:941-949; docs/PROTOCOL.md:226

**Reference retained from audit:** https://specifications.freedesktop.org/desktop-entry/latest/recognized-keys.html

### OS-25 — Use the manifest application name in the Linux AT-SPI application node

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

The manifest application name initializes the Linux AT-SPI process identity before adapters are created. The existing teardown patches remain intact; combo active-descendant projection is also corrected.

**Audit baseline:** Native accessibility is already integrated through accesskit_unix and accesskit_windows, with per-window adapter lifetime. In the pinned accesskit_unix, the process-global application context derives its name from current_exe's filename; the public constructor used here does not accept the manifest's app.name.

**Boundary:** Actual Orca identity/selection speech remains unqualified for this candidate; this does not repair the separate open Windows UIA selection blocker.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/accessibility.rs:276-305; crates/ui4me-host/src/config.rs:26-33; crates/ui4me-host/Cargo.toml:30,46,50; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/accesskit_unix-0.24.1/src/context.rs:33-44

**Candidate implementation:** [crates/ui4me-host/src/host/native.rs](../crates/ui4me-host/src/host/native.rs), [third_party/accesskit_atspi_common-0.21.1/src/context.rs](../third_party/accesskit_atspi_common-0.21.1/src/context.rs), [third_party/accesskit_atspi_common-0.21.1/src/lib.rs](../third_party/accesskit_atspi_common-0.21.1/src/lib.rs), [third_party/accesskit_atspi_common-0.21.1/src/node.rs](../third_party/accesskit_atspi_common-0.21.1/src/node.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/document_native.rs](../crates/ui4me-host/src/host/document_native.rs). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://github.com/AccessKit/accesskit

### OS-26 — Distinguish implemented support, current availability, and qualified behavior

**Audit state:** Partial implementation · **Original recommendation:** Stabilize existing contract · **Core 1.0:** Implemented in candidate; verification pending

host.capabilities schema 2 separates implemented support, observed/unknown availability and qualification/scope. Headless or missing-service states do not automatically become successful native support.

**Audit baseline:** host.capabilities mostly derives clipboard, dialogs, external-open, DnD, IME, and accessibility flags from !headless. It already reports tray=false, printing=false, Windows-only positioning, and native_qualification='requires local evidence'. These flags do not probe portal availability, compositor extensions, assistive-technology activation, or RDP behavior. Linux dependency features enable Wayland only; native dialogs explicitly require a Wayland handle.

**Boundary:** Native qualification is evidence-driven. No new build is automatically declared qualified by a feature flag or dependency API.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-host/src/host/mod.rs:782-789; crates/ui4me-host/src/host/requests.rs:433-443; crates/ui4me-host/Cargo.toml:27-28,44; crates/ui4me-host/src/platform/dialogs_linux.rs:48-58; docs/PROTOCOL.md:121-128

**Candidate implementation:** [crates/ui4me-host/src/host/mod.rs](../crates/ui4me-host/src/host/mod.rs), [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs).

**Regression/verification entry points:** [crates/ui4me-host/src/host/requests.rs](../crates/ui4me-host/src/host/requests.rs), [tools/test_application.py](../tools/test_application.py). These are tests to run; their presence is not a pass record.

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/

### OS-27 — Scoped idle or automatic-sleep inhibition

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** There is no inhibition service or guard lifetime in the host. ashpd contains an Inhibit frontend, but its inhibit feature is disabled. This is not needed for ordinary document-style applications and is not an existing v1 promise.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/requests.rs:132-284; crates/ui4me-host/Cargo.toml:44,49-51; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ashpd-0.13.13/Cargo.toml:146; external cached upstream: /workspace/scratch/3c4bbc64a9f5/toolchain/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ashpd-0.13.13/src/desktop/inhibit.rs:158-261

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Inhibit.html

## Resources and assets

### RES-01 — Static raster images and EXIF orientation

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. PNG, JPEG, GIF, WebP and BMP decode through the image adapter. The pipeline already applies EXIF orientation; GIF/WebP/APNG animation is a separate limitation.

**Audit baseline:** PNG, JPEG, GIF, WebP and BMP decode through the image adapter. The pipeline already applies EXIF orientation; GIF/WebP/APNG animation is a separate limitation.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/decode/image_rs.rs:3–16,44–62,85–114; crates/ui4me-assets/src/decode/exif.rs; crates/ui4me-assets/src/worker.rs

**Reference retained from audit:** https://docs.rs/image/0.25.10/image/codecs/index.html

### RES-02 — Animated GIF, WebP and APNG

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Static raster support and existing first-frame handling remain usable. No GIF/WebP/APNG animation timeline, disposal compositor or frame cache is added.

**Audit baseline:** Only the first frame/default image is used and img.animation is reported.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-assets/src/decode/mod.rs:36–45; crates/ui4me-assets/src/decode/image_rs.rs:10–13,105–114; crates/ui4me-assets/src/worker.rs

**Reference retained from audit:** https://docs.rs/image/latest/image/trait.AnimationDecoder.html

### RES-03 — ICO document images

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The document image allowlist does not include ICO. This is separate from the host application's window-icon loader.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/decode/mod.rs:23–33; crates/ui4me-assets/src/decode/sniff.rs:25–37,89–96; crates/ui4me-assets/src/decode/image_rs.rs:44–62

**Reference retained from audit:** https://docs.rs/image/0.25.10/image/codecs/index.html

### RES-04 — TIFF document images

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** TIFF is outside the current document decoder allowlist.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/decode/mod.rs:23–33; crates/ui4me-assets/src/decode/sniff.rs:25–37,89–96; crates/ui4me-assets/src/decode/image_rs.rs:44–62

**Reference retained from audit:** https://docs.rs/image/0.25.10/image/codecs/index.html

### RES-05 — AVIF and additional image codecs

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** AVIF and specialist formats such as EXR/HDR/QOI are omitted.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/decode/image_rs.rs:44–62; crates/ui4me-assets/src/decode/sniff.rs:25–37

**Reference retained from audit:** https://docs.rs/image/0.25.10/image/codecs/index.html

### RES-06 — ICC color management and high-bit-depth display

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** ICC presence is detected but ignored; samples become premultiplied RGBA8 treated as sRGB, including 16-bit inputs.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/decode/image_rs.rs:10–14,105–114; crates/ui4me-assets/src/decode/mod.rs:42–45; crates/ui4me-renderer/src/document.rs:1435–1444

**Reference retained from audit:** https://docs.rs/image/0.25.10/image/codecs/index.html

### RES-07 — Public in-memory resource bundles

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Public per-document in-memory bundles use ui4me-res URLs, atomic replacement and dependency invalidation; registered source bytes are bounded across repeated calls and counted separately.

**Audit baseline:** ResourceManager implements bundle registration, directory snapshots, replacement invalidation and ui4me-res loading. Hosted documents do not expose those registration APIs.

**Boundary:** Host requests: at most 1MiB decoded bytes/256 entries. Registry: 64 bundles, 4096 entries per bundle, 16MiB aggregate registered bytes; replacement subtracts the old registration before checking the bound. No HTTP, directory-snapshot public API or application-wide shared cache was added.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-assets/src/lib.rs:1081–1138,1184–1185; crates/ui4me-assets/src/bundles.rs; crates/ui4me-renderer/src/document.rs:305–328

**Candidate implementation:** [crates/ui4me-assets/src/bundles.rs](../crates/ui4me-assets/src/bundles.rs), [crates/ui4me-assets/src/lib.rs](../crates/ui4me-assets/src/lib.rs), [crates/ui4me-renderer/src/pipeline/resources.rs](../crates/ui4me-renderer/src/pipeline/resources.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs), [examples/python/application.py](../examples/python/application.py).

**Regression/verification entry points:** [crates/ui4me-assets/src/bundles.rs](../crates/ui4me-assets/src/bundles.rs), [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py). These are tests to run; their presence is not a pass record.

### RES-08 — Programmatic font registration

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** File/byte registration and unregister exist internally. Local @font-face is already available and may be sufficient for most apps.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/lib.rs:876–904; crates/ui4me-assets/src/fonts/registry.rs; crates/ui4me-renderer/src/pipeline/font_faces.rs

### RES-09 — Application font fallback and deterministic font policy

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The font registry/policy has generic families, fallback and system-font controls, but hosted creation mostly uses defaults.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/fonts/registry.rs; crates/ui4me-assets/src/policy.rs; crates/ui4me-assets/src/lib.rs:902–904; crates/ui4me-renderer/src/document.rs:305–331

**Reference retained from audit:** https://docs.rs/fontique/0.12.0/fontique/

### RES-10 — Resource cache statistics and purge controls

**Audit state:** Unexposed primitive · **Original recommendation:** Useful next addition · **Core 1.0:** Implemented in candidate; verification pending

Statistics expose decoded cache counters, separate SVG raster counters and registered bundle source bytes. Purge releases unused decoded entries and (without a URL filter) unused SVG rasters while preserving live visuals and registered source bytes.

**Audit baseline:** ResourceManager has cache statistics, purge/invalidation and runtime diagnostics. Public document diagnostics expose only part of this surface.

**Boundary:** Exact-URL purge addresses decoded image entries; generation/size-keyed SVG rasters are purged only without a URL filter. Purge is not a file watcher, forced reload, total process-RAM measurement or bundle removal.

**Qualification:** Current candidate requires cloud checks and sequential Linux KDE/Python then Windows Server 2022/RDP qualification; no native pass is asserted here.

**Original code evidence:** crates/ui4me-assets/src/lib.rs:1188–1229; crates/ui4me-renderer/src/document.rs:1529; crates/ui4me-host/src/host/document_commands.rs

**Candidate implementation:** [crates/ui4me-assets/src/cache.rs](../crates/ui4me-assets/src/cache.rs), [crates/ui4me-assets/src/svg/raster_cache.rs](../crates/ui4me-assets/src/svg/raster_cache.rs), [crates/ui4me-assets/src/lib.rs](../crates/ui4me-assets/src/lib.rs), [crates/ui4me-renderer/src/pipeline/resources.rs](../crates/ui4me-renderer/src/pipeline/resources.rs), [crates/ui4me-renderer/src/document/application_api.rs](../crates/ui4me-renderer/src/document/application_api.rs), [crates/ui4me-host/src/host/document_commands.rs](../crates/ui4me-host/src/host/document_commands.rs).

**Regression/verification entry points:** [crates/ui4me-assets/src/svg/raster_cache.rs](../crates/ui4me-assets/src/svg/raster_cache.rs), [crates/ui4me-renderer/src/document/application_api_tests.rs](../crates/ui4me-renderer/src/document/application_api_tests.rs), [tools/test_application.py](../tools/test_application.py). These are tests to run; their presence is not a pass record.

### RES-11 — Configurable hosted resource limits

**Audit state:** Unexposed primitive · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Hosted construction selects fixed image/cache/SVG budgets and one document asset root; lower-level ResourcePolicyPatch supports broader configuration.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/policy.rs:69–102,170–214; crates/ui4me-renderer/src/document.rs:305–328; crates/ui4me-renderer/src/document/hosted.rs

### RES-12 — Shared asset caches and application memory budget

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Each Document constructs its own ResourceManager, with independent workers, caches and font resources. Per-document limits do not establish an application-wide cap.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:303–331; crates/ui4me-renderer/src/document/hosted.rs; crates/ui4me-assets/src/lib.rs:1136–1138,1209–1229

## SVG profile

### SVG-01 — Static SVG rendering

**Audit state:** Available · **Original recommendation:** Use now · **Core 1.0:** Retained core

Retain the existing core implementation and qualify it again after integration. Uses usvg/resvg 0.49 for static SVG pictures, including paths, gradients, clipping, masks and supported filters. Inline SVG mutations can produce a new serialized image.

**Audit baseline:** Uses usvg/resvg 0.49 for static SVG pictures, including paths, gradients, clipping, masks and supported filters. Inline SVG mutations can produce a new serialized image.

**Boundary:** Retained baseline profile and its documented limits apply.

**Qualification:** Available in the audited baseline; this label is not a new candidate-native qualification result.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/svg/usvg_adapter.rs:1–16,207–232; crates/ui4me-renderer/src/pipeline/inline_svg.rs:794–829

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

### SVG-02 — SVG text and font integration

**Audit state:** Disabled feature · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Static SVG graphics remain supported. SVG text and SVG font integration remain outside this frozen core; no duplicate text engine or unqualified font resolver is added.

**Audit baseline:** usvg text and font-discovery features are compiled out; SVG text is reported as unsupported. Inline serialization also drops CSS font/text properties from its allowlist, although presentation attributes pass through.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** Cargo.toml; crates/ui4me-assets/src/svg/usvg_adapter.rs:3–5,170–187,225–226; crates/ui4me-assets/src/fonts/registry.rs; crates/ui4me-renderer/src/pipeline/inline_svg.rs:43–73,263–282,606–649,665–688

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

### SVG-03 — Inline SVG CSS cascade integration

**Audit state:** Partial implementation · **Original recommendation:** Useful next addition · **Core 1.0:** Explicit profile boundary

Static inline SVG keeps its existing bounded attribute/style behavior. A complete shared HTML/SVG CSS cascade, inheritance, invalidation and animation model is excluded.

**Audit baseline:** Inline serializer uses a restricted property allowlist and its own CSS matching. It skips at-rules, strips !important priority and does not consume the full loaded stylesheet/cascade state.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/inline_svg.rs:198–241,275–279,307–315,469–505,794–827; patches/stylo/stylo/style/properties/longhands.toml:841–856,1295–1311,1971–1986

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

### SVG-04 — Approved local images referenced inside SVG

**Audit state:** Intentional exclusion · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** A local file.svg can load as an image. Inside that SVG, resolve_string refuses file/URL image references; embedded data images are allowed within limits.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/svg/usvg_adapter.rs:6–10,161–166,182–184; crates/ui4me-assets/src/policy.rs

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

### SVG-05 — Compressed SVGZ

**Audit state:** Disabled feature · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** SVGZ is explicitly refused in the pre-scan and usvg is built without svgz.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-assets/src/svg/prescan.rs:107–113; crates/ui4me-assets/src/svg/usvg_adapter.rs:3–5,195–197; Cargo.toml

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

### SVG-06 — Shape-level SVG hit testing and events

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Hosted hit testing returns the SVG root. Dormant path-hit code is approximate and lacks complete text/image/clipping/pointer-events behavior.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document/svg_hit.rs:23–26; crates/ui4me-assets/src/svg/hit.rs:53–180,216–256

**Reference retained from audit:** https://www.w3.org/TR/SVG2/interact.html

### SVG-07 — Accessible SVG descendants

**Audit state:** Partial implementation · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Keep the currently usable portion exactly within its existing profile; the completion described by the audit is excluded from this batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** An SVG root becomes an image node for a nonempty aria-label, direct title or explicit role=img/image; otherwise it is omitted. Explicit role can have an empty name. This branch does not apply aria-labelledby or desc and does not traverse descendants.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/a11y/build.rs:547–579; crates/ui4me-renderer/src/document/svg_hit.rs:23–26

**Reference retained from audit:** https://docs.rs/accesskit/latest/accesskit/struct.Node.html

### SVG-08 — SMIL and SVG document animation

**Audit state:** Intentional exclusion · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Animation elements are stripped/reported, and usvg/resvg is a static SVG engine. Ordinary CSS animations elsewhere already exist.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/inline_svg.rs:144–153; crates/ui4me-assets/src/svg/usvg_adapter.rs:14–15

**Reference retained from audit:** https://github.com/linebender/resvg/blob/main/README.md

### SVG-09 — SVG foreignObject HTML embedding

**Audit state:** Intentional exclusion · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** foreignObject is explicitly removed/reported.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/pipeline/inline_svg.rs:144–153

**Reference retained from audit:** https://docs.rs/usvg/0.49.0/usvg/

## Application-layer and excluded subsystems

### APP-01 — Reusable components and command routing

**Audit state:** New subsystem · **Original recommendation:** Useful next addition · **Core 1.0:** Application/UI-library responsibility

Reusable components, application commands and command routing belong in the application/UI library above Core. Core supplies DOM, events and native service contracts.

**Audit baseline:** Core exposes document, layout, focus, events, dialogs/popovers and inspection primitives; it is not yet a full higher-level widget/component catalogue.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-host/src/host/document_commands.rs; examples/python/application.py; crates/ui4me-renderer/src/a11y/mod.rs:155–192

### APP-02 — Virtualized long lists and data grids

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Application/UI-library responsibility

Virtualized lists and data grids belong above Core and can use scrolling, geometry and mutation primitives; no core virtualization subsystem is promised.

**Audit baseline:** Scrolling/geometry and containment exist, but content-visibility:auto does not perform offscreen skipping. Large application data sets still need deliberate UI virtualization.

**Boundary:** Outside the frozen Core 1.0 completion scope. This is not a promise of a later version.

**Qualification:** Excluded extension; retained baseline behavior still needs current-candidate regression qualification.

**Original code evidence:** crates/ui4me-host/src/host/document_commands.rs:97–110,225–227,513–523; crates/ui4me-renderer/src/style/view/layout_groups.rs:565–578

### APP-03 — Printing and PDF export

**Audit state:** Intentional exclusion · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** Hosted capability reporting explicitly says printing:false. Retained donor pagination/print code is not a complete hosted printing product.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-host/src/host/mod.rs:782–789; crates/ui4me-host/src/host/documents.rs; crates/ui4me-renderer/src/style/profile/table.rs

**Reference retained from audit:** https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Print.html

### APP-04 — JavaScript, web navigation and browser networking

**Audit state:** Intentional exclusion · **Original recommendation:** Keep outside core · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The application is a local HTML/CSS desktop shell with a language backend; hosted validation rejects script-bearing content.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** crates/ui4me-renderer/src/document.rs:2357,2730; crates/ui4me-assets/src/policy.rs; crates/ui4me-host/src/host/documents.rs

### APP-05 — Audio/video and custom drawing surfaces

**Audit state:** New subsystem · **Original recommendation:** Demand-driven later · **Core 1.0:** Outside frozen Core 1.0

Do not expose or implement this additional feature in the frozen Core 1.0 batch. The audit baseline description below records any underlying primitive or existing behavior; dependency support is not a public Core promise.

**Audit baseline:** The inspected stack is a document renderer and asset pipeline; the presence of HTML parsing or a 2D rasterizer is not a media player or public canvas API.

**Boundary:** No scheduled later release is implied. Revisit only if an actual application requirement justifies changing the frozen scope.

**Qualification:** Excluded extension; no implementation or native qualification is claimed.

**Original code evidence:** Cargo.toml; crates/ui4me-host/src/host/document_commands.rs; crates/ui4me-assets/src/decode/mod.rs

