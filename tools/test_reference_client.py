"""Protocol tests that run on Linux and Windows without a desktop."""
import concurrent.futures
import json
import os
from pathlib import Path
import socket
import struct
import sys
import threading
import unittest
from unittest.mock import patch

EXAMPLES = Path(__file__).resolve().parents[1] / "examples" / "python"
sys.path.insert(0, str(EXAMPLES))
from client import (DisconnectedError, MAX_FRAME_BYTES, MAX_PENDING, ProtocolError,
                    RpcClient, RpcError, SocketStream, NamedPipeStream, encode_frame, read_frame, write_all)


class FragmentedStream:
    def __init__(self, content=b"", fragment=1):
        self.content, self.offset, self.fragment = content, 0, fragment
        self.written = bytearray()
        self.read_calls = []
    def read(self, length):
        self.read_calls.append(length)
        result = self.content[self.offset:self.offset + min(length, self.fragment)]
        self.offset += len(result)
        return result
    def write(self, data):
        part = data[:self.fragment]
        self.written.extend(part)
        return len(part)


class FramingTests(unittest.TestCase):
    def test_fragmented_unicode_uses_byte_count(self):
        value = {"jsonrpc": "2.0", "id": "α", "result": "Příliš 😀 日本語"}
        frame = encode_frame(value)
        self.assertEqual(struct.unpack(">I", frame[:4])[0], len(frame[4:]))
        self.assertEqual(read_frame(FragmentedStream(frame)), value)

    def test_partial_writes(self):
        stream = FragmentedStream(fragment=3)
        frame = encode_frame({"jsonrpc": "2.0", "method": "app.ready"})
        write_all(stream, frame)
        self.assertEqual(bytes(stream.written), frame)

    def test_clean_eof(self):
        self.assertIsNone(read_frame(FragmentedStream()))

    def test_truncated_header(self):
        with self.assertRaises(DisconnectedError):
            read_frame(FragmentedStream(b"\x00\x00"))

    def test_truncated_body(self):
        with self.assertRaises(DisconnectedError):
            read_frame(FragmentedStream(struct.pack(">I", 12) + b"{}"))

    def test_oversize_rejected_before_reading_body(self):
        stream = FragmentedStream(struct.pack(">I", MAX_FRAME_BYTES + 1), fragment=4)
        with self.assertRaises(ProtocolError):
            read_frame(stream)
        self.assertEqual(stream.read_calls, [4])

    def test_duplicate_members_rejected(self):
        body = b'{"jsonrpc":"2.0","id":"one","id":"two"}'
        with self.assertRaises(ProtocolError):
            read_frame(FragmentedStream(struct.pack(">I", len(body)) + body))

    def test_bad_utf8_and_batch_rejected(self):
        for body in (b'\xff', b'[]', b'{"jsonrpc":"1.0"}', b'{"jsonrpc":"2.0","result":NaN}'):
            with self.subTest(body=body), self.assertRaises(ProtocolError):
                read_frame(FragmentedStream(struct.pack(">I", len(body)) + body))


class RpcTests(unittest.TestCase):
    def setUp(self):
        a, b = socket.socketpair()
        b.settimeout(5)
        self.client, self.server = RpcClient(SocketStream(a)), SocketStream(b)
    def tearDown(self):
        self.client.close()
        self.server.close()
    def reply(self, request, value):
        write_all(self.server, encode_frame({"jsonrpc": "2.0", "id": request["id"], "result": value}))

    def test_responses_can_arrive_out_of_order(self):
        first = self.client.call_async("first")
        second = self.client.call_async("second")
        a, b = read_frame(self.server), read_frame(self.server)
        self.reply(b, "second")
        self.reply(a, "first")
        self.assertEqual(first.result(3), "first")
        self.assertEqual(second.result(3), "second")

    def test_callback_can_await_nested_host_call(self):
        self.client.on_request("window.before_close", lambda p: {
            "allow": self.client.call("host.check", p, timeout=3)["allowed"]})
        write_all(self.server, encode_frame({"jsonrpc": "2.0", "id": "host-1",
            "method": "window.before_close", "params": {"window_id": "w1"}}))
        nested = read_frame(self.server)
        self.assertEqual(nested["method"], "host.check")
        self.reply(nested, {"allowed": True})
        answer = read_frame(self.server)
        self.assertEqual(answer, {"jsonrpc": "2.0", "id": "host-1", "result": {"allow": True}})

    def test_connection_failure_fails_outstanding_requests(self):
        future = self.client.call_async("will.not.complete")
        read_frame(self.server)
        self.server.close()
        with self.assertRaises(DisconnectedError):
            future.result(3)

    def test_pending_limit_is_enforced(self):
        futures = [self.client.call_async("pending") for _ in range(MAX_PENDING)]
        with self.assertRaises(RpcError) as error:
            self.client.call_async("overflow")
        self.assertEqual(error.exception.code, -32005)
        self.client.close()
        self.assertTrue(all(f.done() for f in futures))

    def test_cancel_and_late_response_do_not_break_connection(self):
        cancelled = self.client.call_async("slow")
        request = read_frame(self.server)
        self.assertTrue(cancelled.cancel())
        cancel = read_frame(self.server)
        self.assertEqual(cancel["method"], "request.cancel")
        self.reply(request, "too late")
        following = self.client.call_async("next")
        self.reply(read_frame(self.server), "ok")
        self.assertEqual(following.result(3), "ok")
        self.assertTrue(cancelled.cancelled())

    def test_unknown_host_method_receives_error(self):
        write_all(self.server, encode_frame({"jsonrpc": "2.0", "id": "host-2",
            "method": "unknown", "params": {}}))
        self.assertEqual(read_frame(self.server)["error"]["code"], -32601)

    def test_rpc_error_remains_distinct_from_disconnect(self):
        future = self.client.call_async("denied")
        request = read_frame(self.server)
        write_all(self.server, encode_frame({"jsonrpc": "2.0", "id": request["id"],
            "error": {"code": -32003, "message": "Denied", "data": {"reason": "policy"}}}))
        with self.assertRaises(RpcError) as error:
            future.result(3)
        self.assertEqual(error.exception.data, {"reason": "policy"})
        self.assertFalse(self.client.closed)

    def test_user_dialogs_have_no_implicit_deadline(self):
        deadlines = []
        class Completed:
            def result(self, timeout):
                deadlines.append(timeout)
                return {"ok": True}
        with patch.object(self.client, "call_async", return_value=Completed()):
            self.client.call("dialog.open_files")
            self.client.call("host.capabilities")
            self.client.call("dialog.save_file", timeout=0.25)
            self.client.call("dialog.choose_folder", timeout=None)
        self.assertEqual(deadlines, [None, 15.0, 0.25, None])


class Utf8StdioTests(unittest.TestCase):
    def test_backend_output_is_utf8_even_with_an_ansi_pipe_encoding(self):
        import subprocess
        code = ("import sys; sys.path.insert(0, sys.argv[1]); import client; "
                "client.use_utf8_stdio(); print('Příliš → 😀')")
        env = dict(os.environ, PYTHONIOENCODING="cp1252")
        result = subprocess.run([sys.executable, "-c", code, str(EXAMPLES)], env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.decode("utf-8").strip(), "Příliš → 😀")


@unittest.skipUnless(os.name == "nt", "Real named-pipe transport requires Windows")
class WindowsNamedPipeTests(unittest.TestCase):
    def test_real_named_pipe_unicode_and_cancellation_of_blocked_read(self):
        import ctypes
        from ctypes import wintypes
        import time
        import uuid
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.CreateNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
            wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID]
        k.CreateNamedPipeW.restype = wintypes.HANDLE
        k.ConnectNamedPipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
        k.ConnectNamedPipe.restype = wintypes.BOOL
        k.ReadFile.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
            ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
        k.ReadFile.restype = wintypes.BOOL
        k.WriteFile.argtypes = k.ReadFile.argtypes
        k.WriteFile.restype = wintypes.BOOL
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        k.CloseHandle.restype = wintypes.BOOL
        name = r"\\.\pipe\ui4me-python-test-" + uuid.uuid4().hex
        handle = k.CreateNamedPipeW(name, 3 | 0x00080000, 0x8, 1, 65536, 65536, 1000, None)
        self.assertNotEqual(handle, ctypes.c_void_p(-1).value)
        errors, stop = [], threading.Event()
        class Peer:
            def read(self, length):
                buf, count = ctypes.create_string_buffer(length), wintypes.DWORD()
                if not k.ReadFile(handle, buf, length, ctypes.byref(count), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                return buf.raw[:count.value]
            def write(self, data):
                part = bytes(data[:3])  # Force multiple writes and partial reads.
                buf, count = ctypes.create_string_buffer(part), wintypes.DWORD()
                if not k.WriteFile(handle, buf, len(part), ctypes.byref(count), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                return count.value
        def server():
            try:
                if not k.ConnectNamedPipe(handle, None) and ctypes.get_last_error() != 535:
                    raise ctypes.WinError(ctypes.get_last_error())
                request = read_frame(Peer())
                write_all(Peer(), encode_frame({"jsonrpc": "2.0", "id": request["id"], "result": "Český text 😀"}))
                stop.wait(10)
            except BaseException as error:
                errors.append(error)
        worker = threading.Thread(target=server, daemon=True)
        worker.start()
        client = None
        try:
            client = RpcClient(NamedPipeStream(name, 5))
            self.assertEqual(client.call("test", timeout=5), "Český text 😀")
            started = time.monotonic()
            client.close()
            self.assertLess(time.monotonic() - started, 3)
            self.assertTrue(all(not thread.is_alive() for thread in client._threads))
        finally:
            if client:
                client.close()
            stop.set()
            worker.join(3)
            k.CloseHandle(handle)
        self.assertFalse(errors, errors)


if __name__ == "__main__":
    unittest.main(verbosity=2)
