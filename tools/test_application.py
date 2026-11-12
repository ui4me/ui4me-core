"""Document generations and ordered callback IPC, independent of any desktop."""
import socket
import sys
from pathlib import Path
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "examples/python"))
from application import Application, Document, StaleDocumentError, Window
from client import RpcClient, RpcError, SocketStream, encode_frame, read_frame, write_all


class FakeRpc:
    def __init__(self):
        self.notifications, self.requests, self.calls = {}, {}, []
        self.next_window = self.next_document = 0
        self.fail_load = False
        self.closed = False
        self.shutdown_requested = threading.Event()

    def on_notification(self, method, callback):
        self.notifications[method] = callback

    def on_request(self, method, callback):
        self.requests[method] = callback

    def call(self, method, params=None, **options):
        params = params or {}
        self.calls.append((method, params, options))
        if method == "window.create":
            self.next_window += 1
            return {"window_id": f"window-{self.next_window}"}
        if method == "document.load":
            if self.fail_load:
                raise RpcError(-32602, "Fixture load failure")
            self.next_document += 1
            return {"document_id": f"document-{self.next_document}"}
        return {"updated": True}

    def close(self):
        self.closed = True


class FacadeTests(unittest.TestCase):
    def setUp(self):
        self.rpc = FakeRpc()
        self.app = Application(self.rpc)
        self.window = self.app.open_html('<p id="label">Original</p>')
        self.document = self.window.document

    def tearDown(self):
        self.app.close()

    def emit(self, document, event):
        self.rpc.notifications["document.event"]({"window_id": document.window.id,
            "document_id": document.id, "event": event})

    def test_commands_carry_window_and_current_document_identity(self):
        self.document.set_text("label", "Příliš 😀")
        self.assertEqual(self.rpc.calls[-1], ("document.command", {
            "window_id": self.window.id, "document_id": self.document.id,
            "command": {"op": "set_text", "element": "label", "text": "Příliš 😀"}}, {}))

    def test_successful_reload_retires_old_commands_and_notifications(self):
        old_events, new_events = [], []
        self.document.on("click", old_events.append)
        current = self.window.load_html('<p id="new">Replacement</p>')
        current.on("click", new_events.append)
        with self.assertRaises(StaleDocumentError):
            self.document.set_text("label", "wrong generation")
        before = len(self.rpc.calls)
        self.emit(self.document, {"type": "click", "element": "label"})
        self.emit(current, {"type": "click", "element": "new"})
        self.assertEqual(old_events, [])
        self.assertEqual(len(new_events), 1)
        self.assertEqual(len(self.rpc.calls), before)

    def test_failed_reload_keeps_live_document_and_callbacks(self):
        events = []
        self.document.on("click", events.append)
        self.rpc.fail_load = True
        with self.assertRaises(RpcError):
            self.window.load_html("invalid replacement")
        self.assertIs(self.window.document, self.document)
        self.document.set_text("label", "still live")
        self.emit(self.document, {"type": "click", "element": "label"})
        self.assertEqual(len(events), 1)

    def test_unload_prevents_late_callbacks_and_local_writes(self):
        events = []
        self.document.on("click", events.append)
        self.document.unload()
        self.assertIsNone(self.window.document)
        self.emit(self.document, {"type": "click", "element": "label"})
        with self.assertRaises(StaleDocumentError):
            self.document.command("get_text", element="label")
        self.assertEqual(events, [])

    def test_delegation_unsubscribe_and_nonmatching_event(self):
        events = []
        unsubscribe = self.document.on("click", events.append, element="panel")
        self.emit(self.document, {"type": "focus", "element": "child", "path": ["child", "panel"]})
        self.emit(self.document, {"type": "click", "element": "other", "path": ["other"]})
        self.emit(self.document, {"type": "click", "element": "child", "path": ["child", "panel"]})
        unsubscribe()
        unsubscribe()
        self.emit(self.document, {"type": "click", "element": "panel"})
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["document_id"], self.document.id)
        self.assertEqual(events[0]["window_id"], self.window.id)

    def test_closed_window_retires_facade_and_only_that_window(self):
        second = self.app.open_html("<p>Second</p>")
        self.rpc.notifications["window.event"]({"window_id": self.window.id, "type": "closed"})
        with self.assertRaises(StaleDocumentError):
            self.document.set_text("label", "closed")
        self.assertTrue(self.window.closed)
        self.assertFalse(second.closed)

    def test_callback_limit_is_bounded_and_reclaimable(self):
        remove = [self.document.on("click", lambda _: None) for _ in range(Document.MAX_HANDLERS)]
        with self.assertRaises(ValueError):
            self.document.on("click", lambda _: None)
        remove[0]()
        self.document.on("click", lambda _: None)

    def test_document_native_errors_are_visible_and_generation_scoped(self):
        events = []
        self.document.on("error", events.append)
        with patch("application.print") as log:
            self.rpc.notifications["document.error"]({"window_id": self.window.id,
                "document_id": self.document.id, "message": "Native clipboard unavailable"})
            self.assertEqual(events[0]["message"], "Native clipboard unavailable")
            log.assert_called_once()
            self.window.load_html("<p>Replacement</p>")
            self.rpc.notifications["document.error"]({"window_id": self.window.id,
                "document_id": self.document.id, "message": "Retired error"})
            self.assertEqual(len(events), 1)
            log.assert_called_once()

    def test_batch_does_not_expand_into_non_atomic_multiple_calls(self):
        operations = [{"op": "set_text", "element": "label", "text": str(i)} for i in range(3)]
        count = len(self.rpc.calls)
        self.document.batch(iter(operations))
        self.assertEqual(len(self.rpc.calls), count + 1)
        self.assertEqual(self.rpc.calls[-1][1]["command"], {"op": "batch", "ops": operations})

    def test_fatal_error_retires_document_even_if_handler_fails(self):
        def broken(_):
            raise ValueError("Application callback failed")
        self.document.on("error", broken)
        with patch("application.print"), self.assertRaises(ValueError):
            self.rpc.notifications["document.error"]({"window_id": self.window.id,
                "document_id": self.document.id, "message": "Paint failed", "fatal": True})
        self.assertIsNone(self.window.document)
        self.assertFalse(self.window.closed)
        with self.assertRaises(StaleDocumentError):
            self.document.get_text("label")

    def test_fatal_error_handler_can_install_a_replacement_document(self):
        self.document.on("error", lambda event: self.window.load_html("<p>Recovered</p>"))
        with patch("application.print"):
            self.rpc.notifications["document.error"]({"window_id": self.window.id,
                "document_id": self.document.id, "message": "Paint failed", "fatal": True})
        self.assertIsNotNone(self.window.document)
        self.assertIsNot(self.window.document, self.document)
        self.window.document.command("get_text", element="recovered")
        with self.assertRaises(StaleDocumentError):
            self.document.get_text("label")

    def test_environment_constructor_selects_ordered_notifications(self):
        with patch("application.RpcClient.from_environment", return_value=FakeRpc()) as factory:
            with Application.from_environment():
                factory.assert_called_once_with(ordered_notifications=True)


class OrderedTransportTests(unittest.TestCase):
    def test_single_worker_preserves_notifications_during_nested_rpc(self):
        local, remote = socket.socketpair()
        remote.settimeout(4)
        rpc, server = RpcClient(SocketStream(local), ordered_notifications=True), SocketStream(remote)
        app = Application(rpc)
        window = Window(app, "w1")
        document = Document(window, "d1")
        window.document = document
        app._windows[window.id] = window
        events, finished = [], threading.Event()

        def callback(event):
            events.append(("start", event["sequence"]))
            document.set_text("status", str(event["sequence"]))
            events.append(("end", event["sequence"]))
            if event["sequence"] == 2:
                finished.set()

        document.on("click", callback)
        try:
            for sequence in (1, 2):
                write_all(server, encode_frame({"jsonrpc": "2.0", "method": "document.event",
                    "params": {"window_id": "w1", "document_id": "d1",
                        "event": {"type": "click", "element": "button", "sequence": sequence}}}))
            for sequence in (1, 2):
                nested = read_frame(server)
                self.assertEqual(nested["method"], "document.command")
                self.assertEqual(nested["params"]["command"]["text"], str(sequence))
                # The next notification must not overtake an unfinished callback.
                self.assertEqual(events[-1], ("start", sequence))
                write_all(server, encode_frame({"jsonrpc": "2.0", "id": nested["id"], "result": {"updated": True}}))
            self.assertTrue(finished.wait(3))
            self.assertEqual(events, [("start", 1), ("end", 1), ("start", 2), ("end", 2)])
        finally:
            app.close()
            server.close()

    def test_blocked_full_event_queue_does_not_block_lifecycle_or_shutdown(self):
        local, remote = socket.socketpair()
        remote.settimeout(4)
        rpc = RpcClient(SocketStream(local), workers=2, ordered_notifications=True)
        server = SocketStream(remote)
        entered, release = threading.Event(), threading.Event()

        def blocked(_):
            entered.set()
            release.wait(10)

        rpc.on_notification("document.event", blocked)
        notification = encode_frame({"jsonrpc": "2.0", "method": "document.event", "params": {"fixture": True}})
        try:
            write_all(server, notification)
            self.assertTrue(entered.wait(2), "Application event worker did not start")
            # A bounded overloaded event queue must not swallow the unrelated
            # native lifecycle path or stop the independent transport reader.
            for _ in range(140):
                write_all(server, notification)
            for method in ("window.before_close", "app.before_quit"):
                write_all(server, encode_frame({"jsonrpc": "2.0", "id": method, "method": method, "params": {}}))
                answer = read_frame(server)
                self.assertEqual(answer["id"], method)
                self.assertEqual(answer["result"], {"allow": True})
            self.assertGreater(rpc.dropped_notifications, 0)
            write_all(server, encode_frame({"jsonrpc": "2.0", "method": "app.shutdown", "params": {}}))
            self.assertTrue(rpc.shutdown_requested.wait(2), "Shutdown was queued behind the blocked document callback")
            started = time.monotonic()
            rpc.close()
            self.assertLess(time.monotonic() - started, 0.75, "Transport close waited for blocked application code")
            self.assertFalse(release.is_set())
        finally:
            release.set()
            rpc.close()
            server.close()


if __name__ == "__main__":
    unittest.main()
