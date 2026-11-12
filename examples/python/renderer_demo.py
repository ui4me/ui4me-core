"""A real HTML/CSS desktop application and its protocol acceptance fixture.

Run through tools/run_renderer.py. The same application is copied by install.py.
All assets are local; callbacks use the private host protocol, with no web server.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import html
import json
import os
from pathlib import Path
import threading
import time

from application import Application, StaleDocumentError
from client import DisconnectedError, RpcError

HERE = Path(__file__).resolve().parent
UI = HERE / "ui"
REQUIRED_CHECKS = {
    "authenticated_renderer", "local_html_loaded", "document_queries",
    "unicode_values", "atomic_updates", "atomic_failure_rollback",
    "dynamic_fragments", "stylesheet_update", "focus_callback_nested_rpc",
    "two_documents", "stale_document_rejected", "application_ready",
    "shutdown_received", "document_load_rollback", "document_inspection",
    "document_overlays",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--headless-verify", action="store_true")
    parser.add_argument("--native-verify", action="store_true",
                        help="Native creation/rendering smoke, not complete desktop qualification")
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--hold-callback", action="store_true",
                        help="Diagnostic: the Add row callback waits indefinitely; use native Ctrl+Shift+Q")
    parser.add_argument("--crash-after-ready", type=float,
                        help="Diagnostic: exit abruptly after the supplied number of seconds")
    parser.add_argument("--updates", type=float,
                        help="Diagnostic: update document status every N seconds, including while covered or minimized")
    args = parser.parse_args()
    if args.headless_verify and args.native_verify:
        parser.error("Choose one verification mode")
    if args.updates is not None and args.updates < 0.05:
        parser.error("--updates must be at least 0.05 seconds")
    results = []

    def record(name, **detail):
        results.append({"test": name, "status": "PASS", **detail})
        print(f"PASS {name}", flush=True)

    def save():
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(json.dumps({
                "schema": 1, "version": "0.2.0", "time_utc": datetime.now(timezone.utc).isoformat(),
                "native_desktop_qualified": False,
                "mode": "headless" if args.headless_verify else "native-smoke" if args.native_verify else "interactive",
                "results": results,
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    try:
        with Application.from_environment() as app:
            record("authenticated_renderer", host=app.rpc.hello)
            window = app.open_file(UI / "index.html", title="UI4ME — Desktop workspace",
                                   width=1080, height=760)
            doc = window.document
            assert doc is not None
            record("local_html_loaded", window_id=window.id, document_id=doc.id)
            state = {"rows": 2, "dark": True, "details": None}

            def set_status(message):
                doc.set_text("status", message)

            def add_row(_):
                if args.hold_callback:
                    print("HOLD: application callback blocked; native editing and Ctrl+Shift+Q must remain usable", flush=True)
                    threading.Event().wait()
                state["rows"] += 1
                name = doc.get_value("name").strip() or "Untitled item"
                priority = doc.get_value("priority")
                checked = doc.get_checked("notify")
                number = state["rows"]
                markup = (f'<tr id="row-{number}"><td>{number:02d}</td><td>{html.escape(name)}</td>'
                          f'<td>{html.escape(priority)}</td><td>{"Yes" if checked else "No"}</td></tr>')
                doc.batch([
                    {"op": "insert_html", "element": "rows", "position": "beforeend", "html": markup},
                    {"op": "set_text", "element": "row-count", "text": str(number)},
                    {"op": "set_text", "element": "status", "text": f"Added {name}. Updated in one document transaction."},
                ])
                doc.scroll_into_view(f"row-{number}")

            def change_theme(_):
                state["dark"] = not state["dark"]
                (doc.add_class if state["dark"] else doc.remove_class)("workspace", "dark")
                set_status("Dark palette" if state["dark"] else "Light palette")

            def open_details(_):
                existing = state["details"]
                if existing is not None and not existing.closed:
                    existing.set(focus=True)
                    return
                second = app.open_file(UI / "details.html", title="UI4ME — Second document",
                                       width=540, height=420)
                state["details"] = second
                assert second.document is not None
                second.document.on("click", lambda _: second.request_close(), element="close-details")

            def choose_file(_):
                result = app.rpc.call("dialog.open_files", {
                    "window_id": window.id, "title": "Choose a local document",
                    "filters": [{"name": "Text files", "extensions": ["txt", "md", "html"]},
                                {"name": "All files", "extensions": ["*"]}],
                })
                set_status("Selection cancelled" if result["cancelled"] else "Selected: " + ", ".join(result["paths"]))

            doc.on("click", add_row, element="add-row")
            doc.on("click", change_theme, element="theme")
            doc.on("click", open_details, element="second-window")
            doc.on("click", choose_file, element="choose-file")
            doc.on("click", lambda _: doc.command("show_dialog", element="about-dialog"), element="show-dialog")
            doc.on("click", lambda _: doc.command("close_dialog", element="about-dialog", return_value="closed"), element="close-dialog")
            doc.on("click", lambda _: doc.command("toggle_popover", element="help-popover"), element="show-popover")
            doc.on("click", lambda _: app.request_quit(), element="quit")
            doc.on("input", lambda event: doc.set_text("name-preview", event.get("value", "")), element="name")
            doc.on("change", lambda _: set_status("File input: " + ", ".join(doc.command("get_files", element="attachment")["files"])),
                   element="attachment")
            # Submission stays local and is reported to the backend. Prevention
            # is registered before dispatch, never attempted in a late callback.
            doc.prevent_default("entry-form", "submit")
            doc.on("submit", add_row, element="entry-form")

            if args.headless_verify or args.native_verify:
                query = doc.query("input", limit=2)
                assert len(query["ids"]) == 2 and query["total"] >= 4 and query["truncated"]
                remaining = doc.query("input", offset=2, limit=4096)
                assert len(set(query["ids"] + remaining["ids"])) == query["total"]
                record("document_queries", identified_inputs=query["total"])

                text = "Příliš žluťoučký kůň — 日本語 😀"
                doc.set_value("name", text)
                assert doc.get_value("name") == text
                doc.set_checked("notify", False)
                assert not doc.get_checked("notify")
                doc.set_checked("notify", True)
                record("unicode_values")

                doc.batch([
                    {"op": "set_text", "element": "status", "text": "Atomic checkpoint"},
                    {"op": "add_class", "element": "status", "name": "verified"},
                    {"op": "set_attribute", "element": "name", "name": "placeholder", "value": "A new item"},
                ])
                assert doc.get_text("status") == "Atomic checkpoint"
                assert doc.command("get_attribute", element="name", name="placeholder")["value"] == "A new item"
                record("atomic_updates")

                try:
                    doc.batch([
                        {"op": "set_text", "element": "status", "text": "This must roll back"},
                        {"op": "remove_element", "element": "does-not-exist"},
                    ])
                except RpcError:
                    assert doc.get_text("status") == "Atomic checkpoint"
                else:
                    raise AssertionError("Invalid atomic transaction was accepted")
                record("atomic_failure_rollback")

                doc.insert_html("rows", '<tr id="dynamic-test"><td>03</td><td>Inserted</td><td>Normal</td><td>Yes</td></tr>')
                assert doc.command("has_element", element="dynamic-test")["exists"]
                doc.remove("dynamic-test")
                assert not doc.command("has_element", element="dynamic-test")["exists"]
                record("dynamic_fragments")

                try:
                    window.load_html('<div contenteditable="true">Rich editing is outside v0.2</div>')
                except RpcError:
                    assert window.document is doc
                    assert doc.command("has_element", element="name")["exists"]
                else:
                    raise AssertionError("Excluded rich contenteditable was admitted")
                record("document_load_rollback")

                doc.set_text("status", text)
                offset, serialized = 0, ""
                while True:
                    page = doc.command("get_html", element="status", offset=offset, limit=7)
                    serialized += page["html"]
                    if page["next_offset"] is None:
                        break
                    assert page["next_offset"] > offset
                    offset = page["next_offset"]
                assert serialized == text
                node = doc.command("node_of", element="status")["node"]
                assert doc.command("get_node", node=node)["id"] == "status"
                assert doc.command("node_children", node=node)["total"] == 1
                assert isinstance(doc.command("css_profile")["features"], list)
                assert "tree" in doc.command("get_accessibility_tree")
                record("document_inspection")

                doc.command("show_dialog", element="about-dialog")
                assert doc.command("get_dialog", element="about-dialog")["open"]
                doc.command("close_dialog", element="about-dialog", return_value="verified")
                assert doc.command("get_dialog", element="about-dialog")["return_value"] == "verified"
                doc.command("show_popover", element="help-popover")
                assert doc.command("get_popover", element="help-popover")["open"]
                doc.command("hide_popover", element="help-popover")
                record("document_overlays")

                sheet = doc.add_stylesheet("#status { color: rgb(120, 210, 170); }")
                sheet_id = sheet.get("sheet", sheet.get("id"))
                assert isinstance(sheet_id, int), sheet
                doc.command("replace_stylesheet", sheet=sheet_id, css="#status { color: rgb(130, 220, 180); }")
                doc.command("remove_stylesheet", sheet=sheet_id)
                record("stylesheet_update")

                nested = threading.Event()
                def focused(_):
                    doc.set_text("status", "Focus callback completed a nested RPC")
                    nested.set()
                unsubscribe = doc.on("focus", focused, element="name")
                doc.focus("notes")
                doc.focus("name")
                assert nested.wait(5), "No focus callback arrived through IPC"
                assert doc.get_text("status") == "Focus callback completed a nested RPC"
                unsubscribe()
                record("focus_callback_nested_rpc")

                second = app.open_html('<html><body><p id="message">Second document</p></body></html>',
                                       title="UI4ME — Verification document", width=400, height=300)
                assert second.document is not None
                previous = second.document
                assert previous.get_text("message") == "Second document"
                record("two_documents")
                current = second.load_html('<html><body><p id="message">Replacement document</p></body></html>')
                assert current.id != previous.id
                try:
                    previous.set_text("message", "Wrong generation")
                except StaleDocumentError:
                    pass
                else:
                    raise AssertionError("Retired Python facade remained writable")
                try:
                    app.rpc.call("document.command", {"window_id": second.id, "document_id": previous.id,
                        "command": {"op": "set_text", "element": "message", "text": "Wrong generation"}})
                except RpcError:
                    pass
                else:
                    raise AssertionError("Host accepted a stale document identity")
                assert current.get_text("message") == "Replacement document"
                record("stale_document_rejected")
                app.ready()
                record("application_ready")
                if args.native_verify:
                    # Let native frames/resources settle. This is explicitly
                    # only smoke evidence, not a substitute for real input tests.
                    time.sleep(1.0)
                    record("native_renderer_smoke")
                app.request_quit()
                assert app.rpc.shutdown_requested.wait(8), "Missing shutdown notification"
                record("shutdown_received")
                assert REQUIRED_CHECKS <= {r["test"] for r in results}
            else:
                if args.updates is not None:
                    def update_status():
                        count = 0
                        while not app.rpc.shutdown_requested.wait(args.updates):
                            if app.rpc.closed:
                                return
                            count += 1
                            try:
                                doc.set_text("status", f"Background document update {count}; native input remains independent.")
                            except (StaleDocumentError, RpcError, DisconnectedError, OSError):
                                return
                            if count % 40 == 0:
                                print(f"UPDATE {count}: document command completed", flush=True)
                    threading.Thread(target=update_status, name="renderer-updates", daemon=True).start()
                if args.crash_after_ready is not None:
                    if args.crash_after_ready <= 0:
                        parser.error("--crash-after-ready must be positive")
                    timer = threading.Timer(args.crash_after_ready, lambda: os._exit(23))
                    timer.daemon = True
                    timer.start()
                app.run()
        return 0
    except Exception as error:
        results.append({"test": "application", "status": "FAIL",
                        "error": f"{type(error).__name__}: {error}"})
        raise
    finally:
        save()


if __name__ == "__main__":
    raise SystemExit(main())
