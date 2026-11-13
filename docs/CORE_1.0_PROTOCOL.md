# Core 1.0 protocol additions

Transport framing remains version 1. Product version is `1.0.0-rc.1` until native
qualification. `PROTOCOL.md` and `PROTOCOL_DOCUMENTS.md` specify the retained
methods; this file specifies their extensions. Requests reject unknown fields.
Responses can contain additional observational/diagnostic fields.

## Native methods

| Method | Added parameters | Contract |
| --- | --- | --- |
| `window.create` | `min_width`, `min_height`, `max_width`, `max_height` | Logical dimensions, paired minimum and maximum, each 1..8192, minimum ≤ maximum |
| `window.set` | Same constraints | Missing pair leaves it unchanged; both dimensions `null` clear that constraint; validate before changing native state |
| `window.get` | None | Adds `requested` and `observed` objects below |
| `window.request_attention` | `window_id`, `kind` = `informational`, `critical`, or `null` | Returns request acknowledgement, `presented:null`, policy; Wayland cancellation is unsupported |
| `desktop.preferences` | Empty object | Current optional settings, availability/source/revision |
| `desktop.open_external` | Existing `target`, optional `window_id` | Parent is forwarded where the native service supports it; result separates acceptance from presentation |
| `desktop.reveal_file` | `path`, optional `window_id` | Reveal in file manager using platform services, with truthful parent support |

Window query extension:

```json
{
  "requested": {
    "visible": true,
    "resizable": true,
    "size_constraints": {
      "min_width": 320, "min_height": 240,
      "max_width": null, "max_height": null
    }
  },
  "observed": {
    "focused": true, "minimized": null, "maximized": false,
    "fullscreen": false, "resizable": true, "visible": null,
    "theme": "dark", "position": null
  }
}
```

`position`, when available, is `{x,y,unit:"physical_pixels"}`. An unsupported
Wayland global position is `null`. Requested state is not evidence that a
compositor applied it. Existing width/height/physical-size fields are retained.

Preference snapshots have `color_scheme:"dark"|"light"|null`,
`reduced_motion:bool|null`, `high_contrast:bool|null`, `available:bool|null`, `source`,
`revision` and `scope`. Availability is `null` while the initial observation is
pending, `false` after an unavailable/headless observation, and `true` once a
native source is available. Unknown individual settings remain `null` even when
other settings are available. `desktop.preferences_changed` sends the same structure
when observed state changes. `document.command.get_environment` reports app,
system and default resolution; `set_environment` accepts `system` to remove an
app override. High contrast is observed but is not a claim of CSS forced colors.

External-service results contain `accepted`, `cancelled`, `parent_forwarded`,
and `presented:null`. `desktop.reveal_file` currently cannot forward a parent to
the native reveal API. Headless mode reports unavailable/unsupported explicitly.

`host.capabilities` schema 2 distinguishes feature implementation, current
availability (`bool|null`), qualification and scope. Legacy top-level booleans
describe enabled API surfaces; they are not test results. The candidate's
qualification is unverified until the separately recorded native evidence says
otherwise. Software cannot discover that a human/computer-use test passed from
the presence of a backend alone.

## Document command envelope

All operations below remain inside:

```json
{
  "window_id": "opaque-window-id",
  "document_id": "opaque-document-id",
  "command": {"op": "get_focused_element"}
}
```

`T` below means exactly one of `element` (HTML id) or `node` (opaque node
reference). Node references contain their document's identity and the node
generation and can name anonymous nodes; a reference minted by a replaced or
other document is stale (code 4) and never names a node of the current one. Unless the row explicitly allows no target, omitting both is invalid.

| `op` | Fields | Result |
| --- | --- | --- |
| `query_nodes` | `selector`, `offset=0`, `limit=1024` (1..4096) | `{nodes:[{node,element:null|string}],total,offset}` |
| `node_batch` | `ops` (at most 1024) | Atomic generation-checked mutations; `{updated:true}` |
| `element_at_point` | Finite viewport `x`,`y` | `{node,element}` or `null` |
| `get_focused_element` | None | Current identity or `null` |
| `focus` | `T` | Focus that control; `{updated:true}` |
| `blur` | Optional `T` | Without target, blur current focus; targeting another live node is a no-op |
| `get_control_selection` | `T` | `{start,end,direction,length,node}` in UTF-16 units |
| `set_control_selection` | `T`, `start`,`end`,`direction=none` | Updated selection; direction is `none`, `forward`, or `backward` |
| `replace_control_range` | `T`, `replacement`, optional paired `start`,`end`, `selection_mode=preserve` | Updated selection plus `changed` |
| `edit_command` | `command`, optional `T` (otherwise current focus) | `{accepted,clipboard_pending}` |
| `reset_form` | `T` naming a form | `{reset,default_prevented}` |
| `request_submit` | Form `T`, optional `submitter` or `submitter_node` | `{submitted,default_prevented}` |
| `get_form_data` | Same form/submitter targeting | `{entries,submitter}` preserving order and duplicate names |
| `set_custom_validity` | Control `T`, `message` | Validity state; empty message clears the custom error |
| `register_resource_bundle` | `name`, `entries:[{path,data_url}]` | `{name,replaced}` |
| `unregister_resource_bundle` | `name` | `{name,removed}` |
| `get_resource_stats` | None | Decoded cache, SVG raster cache and source-bundle counters |
| `purge_unused_resources` | Optional exact `url` | `{purged,stats}`; preserves live references and registered bytes |

### Node transactions

`node_batch` mirrors the existing DOM mutation batch, replacing `element` with
`node` and a move's `target` with `target_node`. Operations are `set_text`,
`set_attribute`, `remove_attribute`, `add_class`, `remove_class`, `insert_html`,
`replace_children_html`, `remove_element`, `move_element`, `append_text`, and
`trim_children`. Their non-target fields are unchanged. The entire transaction
is validated against a staged document; failure applies no prefix. Node identity
survives a move but not removal/recreation. Live control-value transactions are
not implicitly added to these structural batches.

### Selection and editing

Offsets are UTF-16 code units, not UTF-8 bytes or Python indices. For `A😀B`,
the emoji occupies offsets 1..3; an offset of 2 is rejected because it splits a
surrogate pair. Selection offsets beyond the value length are clamped; reversed
selection ranges collapse to their end as specified by the control API.
Replacement uses the existing selection if both range fields are absent. Modes
are `preserve`, `select`, `start`, and `end`. Programmatic editing preserves the
native undo/redo and `beforeinput`/`input` path rather than setting DOM attributes.

Editing commands are `undo`, `redo`, `cut`, `copy`, `paste`, `select_all`,
and `delete` as accepted by the renderer. A deferred
clipboard request is only accepted at response time; later native failure is an
error notification, not a successful edit. Replacement or focus changes can
invalidate a pending clipboard completion. Password exposure restrictions and
read-only/disabled controls follow the editor's existing policy.

### Form data

Text entries are `{name,kind:"text",value}`. File entries are
`{name,kind:"file",path,filename}`; an empty file input has `path:null` and an
empty filename. A submitter must be a submit-capable control associated with
the named form. Entries preserve repeated names and document order, including
externally associated controls. File data is not copied into an RPC message.
The backend is trusted application code and receives paths chosen by the user.

`request_submit.submitted` means a submit event was dispatched. It can be true
with `default_prevented:true`; a Python application normally pre-registers
submission prevention and handles the event itself. Invalid forms do not emit
that successful submit. `reset_form` respects the registered reset cancellation
rule and clears the controls' edit history when reset proceeds.

### Resource bundles

Paths are relative exact keys with `/` separators. Empty, dot, parent, absolute
and escaping segments are rejected. Resource URLs use `ui4me-res://name/path`.
Each registration is a complete immutable replacement, not an append operation.
All entries decode and validate before the previous bundle is replaced.

Per request: at most 256 entries and 1 MiB aggregate decoded bytes, still within
the 2 MiB encoded frame limit. Per document registry: 16 MiB aggregate registered
bytes, 64 bundles, at most 4096 entries per bundle, with bounded names and paths.
These are source-byte limits, not a total application RAM claim. Image decode,
font and cache policies apply independently.

Cache statistics contain `cache` (entries, bytes, referenced_bytes, hits, misses,
evictions, reserved), `bundles` (count, entries, bytes), and `svg_rasters`.
Unfiltered purge can release unreferenced decoded/SVG raster entries. A URL
filter targets the decoded cache key; it does not remove a registered bundle.

## Python facade

`Application`, `Window`, `Document` and `Node` in `examples/python/application.py`
are the supplied reference API. Application helpers cover capabilities,
preferences, clipboard and external services. Window helpers cover constraints,
observed state, attention, lifecycle and native pickers. Picker methods with
`wait=False` return the real transport future; `future.cancel()` requests native
cancellation. `wait=True` has no arbitrary deadline while the user chooses.

`Document.command()` remains available for every typed operation. Named methods
cover normal editing/forms, stylesheets, selection, geometry, dialogs/popovers,
resources and node operations. `Node` rejects retired documents locally; the
host also validates the node generation. Close policy callbacks are routed per
window, so adding a callback for one window cannot replace another's policy.

## Events and incoming drops

Native input stays local. Event notifications include the existing window and
document identity and target/path metadata. New mouse-boundary/context/auxiliary
and edit/selection events preserve native ordering. `drag_feedback` includes
phase (`enter`,`over`,`leave`), accepted kind, node/element, viewport coordinates
and `external:true`; it is emitted for policy/target transitions rather than
every pointer movement. A completed drop retains the original captured target.
Set `data-ui4me-drop="files"`, `"text"`, or `"files text"` on a receiving element;
`none` blocks that subtree. Async completion rechecks both identity and policy.
