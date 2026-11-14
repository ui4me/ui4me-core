"""Core 1.0 application-contract checks through real Python/host IPC.

Called by the reference app in both headless and native smoke modes. These
checks do not claim native accessibility, compositor or screen-reader results.
"""
import threading

from ui4me_core import StaleDocumentError
from ui4me_core import RpcError

REQUIRED_CHECKS = {
    "completion_window_contract", "completion_anonymous_nodes",
    "completion_selection_editing", "completion_form_contract",
    "completion_resources", "completion_preferences", "completion_stale_nodes",
}


def run_completion_checks(app, record):
    window = app.open_html("""<!doctype html><html><body>
      <form id="form"><input id="text" name="same" value="A😀B" required>
        <input name="same" value="second"><input name="skip" disabled value="bad">
        <input name="attachment" type="file"><button id="submit" name="action" value="save">Save</button>
      </form><main id="items"><p>Anonymous original</p></main>
      <img id="bundle-image" width="8" height="8" alt="Bundled green square">
    </body></html>""", title="UI4ME — Completion contracts", width=500, height=400,
        min_width=240, min_height=180, max_width=1000, max_height=800)
    doc = window.document
    assert doc is not None
    state = window.state()
    assert state["requested"]["size_constraints"]["min_width"] == 240
    assert "focused" in state["observed"] and "position" in state["observed"]
    window.set(min_width=None, min_height=None)
    assert window.state()["requested"]["size_constraints"]["min_width"] is None
    try:
        window.set(max_width=50, max_height=0)
    except RpcError:
        assert window.state()["requested"]["size_constraints"]["max_width"] == 1000
    else:
        raise AssertionError("Invalid window constraints were accepted")
    record("completion_window_contract")

    nodes = doc.query_nodes("#items > p")["nodes"]
    assert len(nodes) == 1 and nodes[0]["element"] is None
    anonymous = doc.node(reference=nodes[0]["node"])
    anonymous.set_text("Anonymous updated")
    assert doc.command("get_html", node=anonymous.id)["html"] == "Anonymous updated"
    before = doc.command("get_html", node=anonymous.id)["html"]
    try:
        doc.node_batch([{"op": "set_text", "node": anonymous.id, "text": "must roll back"},
                        {"op": "remove_element", "node": "invalid-node-reference"}])
    except RpcError:
        assert doc.command("get_html", node=anonymous.id)["html"] == before
    else:
        raise AssertionError("Invalid node batch was accepted")
    record("completion_anonymous_nodes")

    doc.focus("text")
    assert doc.focused_element()["element"] == "text"
    doc.set_control_selection("text", 1, 3, direction="backward")
    assert doc.control_selection("text")["direction"] == "backward"
    try:
        doc.set_control_selection("text", 2, 3)
    except RpcError:
        assert doc.control_selection("text")["start"] == 1
    else:
        raise AssertionError("Selection split a UTF-16 surrogate pair")
    result = doc.replace_control_range("text", "X", selection_mode="select")
    assert result["changed"] and doc.get_value("text") == "AXB"
    doc.edit("undo", "text")
    assert doc.get_value("text") == "A😀B"
    assert doc.control_selection("text")["direction"] == "backward"
    doc.edit("redo", "text")
    assert doc.get_value("text") == "AXB"
    doc.blur()
    assert doc.focused_element() is None
    record("completion_selection_editing")

    entries = doc.form_data("form", submitter="submit")["entries"]
    assert [entry["name"] for entry in entries] == ["same", "same", "attachment", "action"]
    assert entries[2]["kind"] == "file" and entries[2]["path"] is None
    assert entries[3]["value"] == "save"
    assert not doc.set_custom_validity("text", "Please review")["valid"]
    assert not doc.request_submit("form", submitter="submit")["submitted"]
    doc.set_custom_validity("text", "")
    submitted = threading.Event()
    unsubscribe = doc.on("submit", lambda _: submitted.set(), element="form")
    doc.prevent_default("form", "submit")
    assert doc.request_submit("form", submitter="submit")["submitted"]
    assert submitted.wait(5), "Programmatic submission did not emit the native submit event"
    unsubscribe()
    doc.reset_form("form")
    assert doc.get_value("text") == "A😀B"
    doc.edit("undo", "text")
    assert doc.get_value("text") == "A😀B", "Reset left an old undo transaction"
    record("completion_form_contract")

    image = b'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="green"/></svg>'
    doc.register_resources("runtime", {"pixel.svg": (image, "image/svg+xml")})
    doc.set_attribute("bundle-image", "src", "ui4me-res://runtime/pixel.svg")
    stats = doc.resource_stats()
    assert stats["bundles"]["count"] == 1 and stats["bundles"]["bytes"] == len(image)
    doc.purge_unused_resources()
    assert doc.resource_stats()["bundles"]["bytes"] == len(image)
    try:
        doc.register_resources("runtime", {"../escape.svg": (image, "image/svg+xml")})
    except RpcError:
        assert doc.resource_stats()["bundles"]["bytes"] == len(image)
    else:
        raise AssertionError("Invalid bundle path replaced a working resource")
    doc.unregister_resources("runtime")
    assert doc.resource_stats()["bundles"]["count"] == 0
    record("completion_resources")

    observed = app.desktop_preferences()
    assert {"color_scheme", "reduced_motion", "high_contrast", "available", "source"} <= observed.keys()
    doc.set_environment(color_scheme="dark", reduced_motion="reduce")
    assert doc.environment()["color_scheme_source"] == "app"
    assert doc.environment()["reduced_motion"] == "reduce"
    doc.set_environment(color_scheme="system", reduced_motion="system")
    assert doc.environment()["color_scheme_source"] in ("system", "default")
    record("completion_preferences")

    anonymous.remove()
    try:
        anonymous.set_text("stale")
    except RpcError:
        pass
    else:
        raise AssertionError("Removed node reference stayed writable")
    replacement = window.load_html("<p>Replacement</p>")
    assert replacement.id != doc.id
    try:
        anonymous.inspect()
    except StaleDocumentError:
        pass
    else:
        raise AssertionError("Old node facade survived document replacement")
    record("completion_stale_nodes")
    # The caller has not announced app.ready yet. Closing a startup window is
    # intentionally application-startup cancellation in the host lifecycle;
    # leave this verification window owned by the app until its orderly quit.
