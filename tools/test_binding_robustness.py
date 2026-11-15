"""Python binding robustness under bursts and failing callbacks (T16, T9).

These tests drive the real ``RpcClient`` and ``Application`` over a socketpair
whose other end is a scripted peer. They prove client-side behaviour only (no
native host, no rendering): which notifications survive an overloaded event
worker, that window/document bookkeeping is applied without waiting for that
worker, and that user callback failures are visible and never kill a worker.
"""
import io
import math
import queue
import socket
import sys
import threading
import time
import unittest
from contextlib import redirect_stderr
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
from ui4me_core.application import Application, Document, HostStaleError, StaleDocumentError, Window
from ui4me_core.client import RpcClient, RpcError, SocketStream, encode_frame, read_frame, write_all


class Peer:
    """The host end of a socketpair: records frames and auto-answers requests."""

    def __init__(self, test, *, workers=4):
        local, remote = socket.socketpair()
        remote.settimeout(10)
        self.stream = SocketStream(remote)
        self.rpc = RpcClient(SocketStream(local), workers=workers, ordered_notifications=True)
        self.app = Application(self.rpc)
        self.frames = queue.Queue()
        self.seen = []
        self.answers = {"window.request_close": {"requested": True}, "app.request_quit": {"requested": True},
                        "document.command": {"updated": True}}
        self._lock = threading.Lock()
        self._thread = threading.Thread(target=self._read, daemon=True)
        self._thread.start()
        test.addCleanup(self.close)

    def _read(self):
        try:
            while True:
                message = read_frame(self.stream)
                if message is None:
                    return
                self.seen.append(message)
                self.frames.put(message)
                method = message.get("method")
                if "id" in message and method in self.answers:
                    answer = self.answers[method]
                    if isinstance(answer, dict) and "error" in answer:
                        self.send({"jsonrpc": "2.0", "id": message["id"], "error": answer["error"]})
                    else:
                        self.respond(message["id"], answer)
        except Exception:  # The client side closed.
            return

    def send(self, message):
        with self._lock:
            write_all(self.stream, encode_frame(message))

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def respond(self, request_id, result):
        self.send({"jsonrpc": "2.0", "id": request_id, "result": result})

    def event(self, document, event):
        self.notify("document.event", {"window_id": document.window.id, "document_id": document.id,
                                       "event": event})

    def expect(self, predicate, timeout=5.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                message = self.frames.get(timeout=max(0.01, deadline - time.monotonic()))
            except queue.Empty:
                break
            if predicate(message):
                return message
        raise AssertionError("expected frame was not sent by the client")

    def sent(self, method):
        return wait_for(lambda: any(m.get("method") == method for m in list(self.seen)))

    def window(self, window_id, document_id):
        window = Window(self.app, window_id)
        window.document = Document(window, document_id)
        self.app._windows[window_id] = window
        return window

    def close(self):
        self.rpc.close()
        self.stream.close()


def wait_for(condition, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return True
        time.sleep(0.005)
    return bool(condition())


class ControlNotificationTests(unittest.TestCase):
    """T16 (Python): control notifications are never dropped under bursts."""

    def test_request_close_during_1000_event_burst_retires_window_and_drops_no_control(self):
        peer = Peer(self)
        busy = peer.window("w1", "d1")
        closing = peer.window("w2", "d2")
        entered, release = threading.Event(), threading.Event()
        moves, lifecycle = [], []

        def slow(event):
            moves.append(event["seq"])
            if not entered.is_set():
                entered.set()
                release.wait(20)

        busy.document.on("pointermove", slow)
        peer.app.on_window_event(lambda p: lifecycle.append((p["window_id"], p["type"])))
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            peer.event(busy.document, {"type": "pointermove", "seq": -1})
            self.assertTrue(entered.wait(3), "event worker did not start")
            closer = threading.Thread(target=closing.request_close)
            closer.start()
            peer.expect(lambda m: m.get("method") == "window.request_close")
            for seq in range(500):
                peer.event(busy.document, {"type": "pointermove", "seq": seq})
            peer.notify("window.event", {"window_id": "w2", "type": "close_requested",
                                         "data": {"reason": "backend_request"}})
            peer.send({"jsonrpc": "2.0", "id": "host:decision:1", "method": "window.before_close",
                       "params": {"window_id": "w2", "reason": "backend_request"}})
            answer = peer.expect(lambda m: m.get("id") == "host:decision:1")
            self.assertEqual(answer["result"], {"allow": True})
            closer.join(5)
            for seq in range(500, 1000):
                peer.event(busy.document, {"type": "pointermove", "seq": seq})
            peer.notify("window.event", {"window_id": "w2", "type": "closed", "data": {}})
            # The event worker is still blocked: the bookkeeping must not wait for it.
            self.assertTrue(wait_for(lambda: closing.closed), "closed window not retired while the worker was busy")
            with self.assertRaises(StaleDocumentError):
                closing.document.set_text("p", "after close")
            self.assertNotIn("w2", peer.app._windows)
            self.assertFalse(busy.closed)
            release.set()
            self.assertTrue(wait_for(lambda: ("w2", "closed") in lifecycle))
        self.assertEqual(lifecycle, [("w2", "close_requested"), ("w2", "closed")])
        counts = peer.rpc.dropped_by_kind
        self.assertEqual(counts["control"], 0)
        self.assertGreater(peer.rpc.dropped_notifications, 0)
        self.assertEqual(peer.rpc.dropped_notifications, counts["coalescible"] + counts["discrete"])
        self.assertLessEqual(len(moves), 1 + 128 + 1)
        self.assertIn("dropping", stderr.getvalue())
        self.assertEqual(stderr.getvalue().count("UI4ME: event callbacks are behind"), 1)

    def test_click_after_300_event_burst_is_delivered(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        entered, release = threading.Event(), threading.Event()
        clicks, transitions = [], []

        def slow(event):
            transitions.append(event["seq"])
            if not entered.is_set():
                entered.set()
                release.wait(20)

        window.document.on("transitionrun", slow)
        window.document.on("click", lambda event: clicks.append(event["element"]))
        with redirect_stderr(io.StringIO()):
            for seq in range(301):
                peer.event(window.document, {"type": "transitionrun", "seq": seq})
                if seq == 0:
                    self.assertTrue(entered.wait(3))
            peer.event(window.document, {"type": "click", "element": "save"})
            self.assertTrue(wait_for(lambda: peer.rpc.dropped_notifications >= 300 - 128))
            release.set()
            self.assertTrue(wait_for(lambda: clicks == ["save"]), "click after the burst was dropped")
        self.assertEqual(peer.rpc.dropped_by_kind["discrete"], 0)
        # Order of what was kept is preserved.
        self.assertEqual(transitions, sorted(transitions))

    def test_unsubscribed_burst_never_occupies_the_queue(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        entered, release = threading.Event(), threading.Event()
        clicks = []

        def first_click(event):
            clicks.append(event["element"])
            if not entered.is_set():
                entered.set()
                release.wait(20)

        window.document.on("click", first_click)
        peer.event(window.document, {"type": "click", "element": "a"})
        self.assertTrue(entered.wait(3))
        for seq in range(1000):
            peer.event(window.document, {"type": "pointermove", "seq": seq})
        peer.event(window.document, {"type": "click", "element": "b"})
        self.assertTrue(wait_for(lambda: peer.rpc.filtered_notifications >= 1000))
        release.set()
        self.assertTrue(wait_for(lambda: clicks == ["a", "b"]))
        self.assertEqual(peer.rpc.dropped_notifications, 0)

    def test_fatal_document_error_retires_eagerly_and_still_reaches_handlers(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        entered, release = threading.Event(), threading.Event()
        errors = []
        window.document.on("input", lambda e: (entered.set(), release.wait(20)))
        window.document.on("error", lambda e: errors.append(e["fatal"]))
        document = window.document
        peer.event(document, {"type": "input"})
        self.assertTrue(entered.wait(3))
        for seq in range(200):
            peer.event(document, {"type": "input", "seq": seq})
        peer.notify("document.error", {"window_id": "w1", "document_id": "d1", "message": "Renderer failed",
                                       "fatal": True})
        with redirect_stderr(io.StringIO()):
            self.assertTrue(wait_for(lambda: document._retired), "fatal error not applied eagerly")
            with self.assertRaises(StaleDocumentError):
                document.set_text("x", "y")
            release.set()
            self.assertTrue(wait_for(lambda: errors == [True]), "fatal error handler was skipped")
        self.assertIsNone(window.document)

    def test_shutdown_and_lifecycle_requests_bypass_a_full_pool(self):
        peer = Peer(self, workers=1)
        gate = threading.Event()
        peer.rpc.on_request("test.slow", lambda _: gate.wait(20) and {"ok": True})
        for index in range(200):
            peer.send({"jsonrpc": "2.0", "id": f"slow-{index}", "method": "test.slow", "params": {}})
        peer.send({"jsonrpc": "2.0", "id": "quit-1", "method": "app.before_quit", "params": {}})
        peer.notify("app.shutdown", {"deadline_ms": 5000})
        self.assertTrue(peer.rpc.shutdown_requested.wait(3), "app.shutdown was queued behind blocked callbacks")
        rejected = peer.expect(lambda m: str(m.get("id", "")).startswith("slow-") and "error" in m)
        self.assertEqual(rejected["error"]["code"], -32005)
        gate.set()
        answer = peer.expect(lambda m: m.get("id") == "quit-1", timeout=10)
        self.assertEqual(answer["result"], {"allow": True})


class CallbackRobustnessTests(unittest.TestCase):
    """T9: a failing callback never kills a worker and is always visible."""

    def test_sys_exit_in_event_callback_requests_quit_and_keeps_delivering(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        seen = []

        def quit_button(event):
            seen.append(event["n"])
            if event["n"] == 1:
                sys.exit(3)

        window.document.on("click", quit_button)
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            peer.event(window.document, {"type": "click", "n": 1})
            peer.expect(lambda m: m.get("method") == "app.request_quit")
            peer.event(window.document, {"type": "click", "n": 2})
            self.assertTrue(wait_for(lambda: seen == [1, 2]), "the event worker died")
        self.assertEqual(peer.rpc.exit_status, 3)
        self.assertIn("SystemExit", stderr.getvalue())
        self.assertIn("requesting application quit", stderr.getvalue())

    def test_run_exits_with_the_callback_exit_status_after_shutdown(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        peer.app._ready = True
        window.document.on("click", lambda e: sys.exit(4))
        with redirect_stderr(io.StringIO()):
            peer.event(window.document, {"type": "click"})
            peer.expect(lambda m: m.get("method") == "app.request_quit")
            peer.notify("app.shutdown", {"deadline_ms": 5000})
            with self.assertRaises(SystemExit) as raised:
                peer.app.run()
        self.assertEqual(raised.exception.code, 4)

    def test_keyboard_interrupt_in_callback_requests_quit_with_130(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        window.document.on("click", lambda e: (_ for _ in ()).throw(KeyboardInterrupt()))
        with redirect_stderr(io.StringIO()):
            peer.event(window.document, {"type": "click"})
            peer.expect(lambda m: m.get("method") == "app.request_quit")
        self.assertEqual(peer.rpc.exit_status, 130)

    def test_sys_exit_in_close_policy_denies_close_and_quits(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        window.on_before_close(lambda: sys.exit(0))
        with redirect_stderr(io.StringIO()):
            peer.send({"jsonrpc": "2.0", "id": "host:decision:1", "method": "window.before_close",
                       "params": {"window_id": "w1"}})
            answer = peer.expect(lambda m: m.get("id") == "host:decision:1")
            self.assertTrue(peer.sent("app.request_quit"))
        self.assertEqual(answer["result"], {"allow": False})
        # The pool worker survived: a second decision is answered.
        window.on_before_close(lambda: True)
        peer.send({"jsonrpc": "2.0", "id": "host:decision:2", "method": "window.before_close",
                   "params": {"window_id": "w1"}})
        self.assertEqual(peer.expect(lambda m: m.get("id") == "host:decision:2")["result"], {"allow": True})

    def test_sys_exit_in_quit_policy_allows_the_quit(self):
        peer = Peer(self)
        peer.app.on_before_quit(lambda: sys.exit(5))
        with redirect_stderr(io.StringIO()):
            peer.send({"jsonrpc": "2.0", "id": "q1", "method": "app.before_quit", "params": {}})
            answer = peer.expect(lambda m: m.get("id") == "q1")
        self.assertEqual(answer["result"], {"allow": True})
        self.assertEqual(peer.rpc.exit_status, 5)

    def test_unencodable_result_gets_an_error_response_and_workers_survive(self):
        peer = Peer(self)
        peer.rpc.on_request("test.nan", lambda _: {"value": math.nan})
        peer.rpc.on_request("test.object", lambda _: {"value": object()})
        peer.rpc.on_request("test.ok", lambda _: {"ok": True})
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            for name in ("nan", "object"):
                peer.send({"jsonrpc": "2.0", "id": name, "method": f"test.{name}", "params": {}})
                answer = peer.expect(lambda m, name=name: m.get("id") == name)
                self.assertEqual(answer["error"]["code"], -32603)
                self.assertIn("not JSON-serializable", answer["error"]["message"])
            for index in range(8):
                peer.send({"jsonrpc": "2.0", "id": f"ok-{index}", "method": "test.ok", "params": {}})
                self.assertEqual(peer.expect(lambda m, i=index: m.get("id") == f"ok-{i}")["result"], {"ok": True})
        alive = [t for t in threading.enumerate() if t.name.startswith("ui4me-callback-") and t.is_alive()]
        self.assertGreaterEqual(len(alive), 4)
        self.assertIn("Traceback", stderr.getvalue())

    def test_failing_handler_does_not_skip_the_next_and_prints_a_traceback(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        ran = []

        def broken(event):
            return {}["missing_key"]

        window.document.on("click", broken)
        window.document.on("click", lambda e: ran.append("second"))
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            peer.event(window.document, {"type": "click"})
            self.assertTrue(wait_for(lambda: ran == ["second"]), "one failing handler skipped the next")
        text = stderr.getvalue()
        self.assertIn("Traceback", text)
        self.assertIn("test_binding_robustness.py", text)
        self.assertIn("KeyError", text)

    def test_window_listener_failures_are_isolated(self):
        peer = Peer(self)
        peer.window("w1", "d1")
        ran = []
        peer.app.on_window_event(lambda p: sys.exit(1))
        peer.app.on_window_event(lambda p: ran.append(p["type"]))
        with redirect_stderr(io.StringIO()):
            peer.notify("window.event", {"window_id": "w1", "type": "focused", "data": {"focused": True}})
            self.assertTrue(wait_for(lambda: ran == ["focused"]))

    def test_none_returning_or_raising_policy_vetoes_loudly(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        window.on_before_close(lambda: None)
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            for index in range(2):
                peer.send({"jsonrpc": "2.0", "id": f"n{index}", "method": "window.before_close",
                           "params": {"window_id": "w1"}})
                self.assertEqual(peer.expect(lambda m, i=index: m.get("id") == f"n{i}")["result"], {"allow": False})
        self.assertEqual(stderr.getvalue().count("returned None"), 1, stderr.getvalue())
        self.assertIn("Return True to allow", stderr.getvalue())

        def raising():
            raise ValueError("unsaved")

        window.on_before_close(raising)
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            peer.send({"jsonrpc": "2.0", "id": "r", "method": "window.before_close", "params": {"window_id": "w1"}})
            self.assertEqual(peer.expect(lambda m: m.get("id") == "r")["result"], {"allow": False})
        self.assertIn("ValueError: unsaved", stderr.getvalue())
        self.assertIn("refused", stderr.getvalue())


class FacadeContractTests(unittest.TestCase):
    def test_host_closed_error_is_a_stale_document_error_and_retires_the_document(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        document = window.document
        peer.answers["document.command"] = {"error": {"code": -32004, "message": "Window does not exist"}}
        with self.assertRaises(StaleDocumentError) as raised:
            document.set_text("p", "x")
        self.assertIsInstance(raised.exception, RpcError)
        self.assertIsInstance(raised.exception, HostStaleError)
        self.assertEqual(raised.exception.code, -32004)
        self.assertTrue(document._retired)
        sent = len(peer.seen)
        with self.assertRaises(StaleDocumentError):
            document.get_text("p")  # now rejected locally, without a request
        time.sleep(0.05)
        self.assertEqual(len(peer.seen), sent)

    def test_host_closed_error_on_a_window_call_retires_the_window(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        peer.answers["window.set"] = {"error": {"code": -32004, "message": "Window is closing"}}
        with self.assertRaises(HostStaleError):
            window.set(title="x")
        self.assertTrue(window.closed)
        with self.assertRaises(StaleDocumentError):
            window.document.set_text("p", "x")
        # The later closed event still reaches listeners exactly once.
        seen = []
        peer.app.on_window_event(lambda p: seen.append(p["type"]))
        peer.notify("window.event", {"window_id": "w1", "type": "closed", "data": {}})
        self.assertTrue(wait_for(lambda: seen == ["closed"]))
        self.assertNotIn("w1", peer.app._closing)

    def test_closed_event_racing_a_host_closed_error_is_still_host_stale(self):
        """binding review #3: the reader thread pops the window (closed event)
        between the caller's lookup and its removal; never a KeyError."""
        peer = Peer(self)
        window = peer.window("w1", "d1")
        app = peer.app

        class RacingWindows(dict):
            fired = False

            def _closed_arrives(self):
                if not self.fired:
                    self.fired = True
                    app._window_event_received({"window_id": "w1", "type": "closed"})

            def get(self, key, default=None):
                value = dict.get(self, key, default)
                self._closed_arrives()
                return value

            def pop(self, key, *default):
                self._closed_arrives()
                return dict.pop(self, key, *default)

        app._windows = RacingWindows(app._windows)
        peer.answers["window.set"] = {"error": {"code": -32004, "message": "Window is closing"}}
        with self.assertRaises(HostStaleError):
            window.set(title="x")
        self.assertTrue(app._windows.fired)
        self.assertTrue(window.closed)
        self.assertNotIn("w1", app._windows)
        self.assertIs(app._closing.get("w1"), window)

    def test_ready_survives_a_closed_event_while_it_scans_windows(self):
        """binding review #4: ready() iterated app._windows while the reader
        thread can pop it (RuntimeError: dictionary changed size)."""
        peer = Peer(self)
        peer.answers["app.ready"] = {}
        app = peer.app
        abandoned = Window(app, "w0")
        app._windows["w0"] = abandoned
        app._abandoned.append(abandoned)

        class ClosingWhileScanned(Window):
            """Reading .closed delivers another window's closed event."""

            @property
            def closed(self):
                app._window_event_received({"window_id": "w2", "type": "closed"})
                return True

            @closed.setter
            def closed(self, value):
                pass

        app._windows["w1"] = ClosingWhileScanned(app, "w1")
        app._windows["w2"] = Window(app, "w2")
        app.ready()
        self.assertTrue(app._ready)
        self.assertNotIn("w2", app._windows)

    def test_other_rpc_errors_are_unchanged(self):
        peer = Peer(self)
        window = peer.window("w1", "d1")
        peer.answers["document.command"] = {"error": {"code": -32602, "message": "Unknown element"}}
        with self.assertRaises(RpcError) as raised:
            window.document.set_text("missing", "x")
        self.assertNotIsInstance(raised.exception, StaleDocumentError)
        self.assertFalse(window.document._retired)

    def test_on_activated_receives_second_launches_in_order(self):
        peer = Peer(self)
        seen = []
        unsubscribe = peer.app.on_activated(lambda p: seen.append(p["cwd"]))
        peer.notify("app.activated", {"args": ["ui4me-host"], "cwd": "/a", "activation_token": None})
        self.assertTrue(wait_for(lambda: seen == ["/a"]))
        unsubscribe()
        peer.notify("app.activated", {"args": [], "cwd": "/b", "activation_token": None})
        time.sleep(0.1)
        self.assertEqual(seen, ["/a"])

    def test_call_timeout_is_the_builtin_timeout_error(self):
        peer = Peer(self)
        with self.assertRaises(TimeoutError):
            peer.rpc.call("test.never", timeout=0.05)
        cancel = peer.expect(lambda m: m.get("method") == "request.cancel")
        self.assertTrue(cancel["params"]["id"].startswith("py-"))


if __name__ == "__main__":
    unittest.main()
