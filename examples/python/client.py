"""Reference UI4ME host client, using only the Python standard library.

This is executable protocol documentation and test material, not a packaged SDK.
The reader and writer never run application callbacks. Callbacks may therefore
await host RPCs. Both directions use the same length-prefixed JSON-RPC profile.
"""

from __future__ import annotations

import concurrent.futures
import ctypes
import json
import os
import queue
import socket
import struct
import sys
import threading
import time
from typing import Any, Callable

MAX_FRAME_BYTES = 2 * 1024 * 1024
MAX_PENDING = 128
MAX_QUEUED_BYTES = 8 * 1024 * 1024
_DEFAULT_TIMEOUT = object()


class ProtocolError(Exception):
    pass


class DisconnectedError(ConnectionError):
    pass


class RpcError(Exception):
    def __init__(self, code: int, message: str, data: Any = None):
        super().__init__(f"{code}: {message}")
        self.code, self.message, self.data = code, message, data


class SocketStream:
    def __init__(self, sock: socket.socket):
        self.sock = sock

    def read(self, size: int) -> bytes:
        return self.sock.recv(size)

    def write(self, data: bytes | memoryview) -> int:
        return self.sock.send(data)

    def close(self) -> None:
        try:
            self.sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self.sock.close()


class NamedPipeStream:
    """Overlapped Windows pipe handle shared by dedicated reader/writer threads.

    Windows serializes all I/O on a synchronous file object, so a blocked read
    would stall every write on the same pipe. Each operation therefore uses its
    own OVERLAPPED/event, and close cancels in-flight operations with CancelIoEx
    before closing the handle once no operation still uses it.
    """

    _ABORTED = (6, 109, 232, 233, 995)

    def __init__(self, name: str, timeout: float = 15.0):
        if os.name != "nt":
            raise OSError("Windows named pipes require Windows")
        if not name.startswith("\\\\.\\pipe\\"):
            raise ValueError("A local named-pipe path is required")
        from ctypes import wintypes

        class OVERLAPPED(ctypes.Structure):
            _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t),
                        ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD),
                        ("hEvent", wintypes.HANDLE)]

        self._overlapped = OVERLAPPED
        self._wintypes = wintypes
        self._dll = ctypes.WinDLL("kernel32", use_last_error=True)
        self._lock = threading.Lock()
        self._idle = threading.Condition(self._lock)
        self._in_flight = 0
        self._closed = False
        k = self._dll
        k.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                  wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD,
                                  wintypes.HANDLE]
        k.CreateFileW.restype = wintypes.HANDLE
        k.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
        k.WaitNamedPipeW.restype = wintypes.BOOL
        for name_ in ("ReadFile", "WriteFile"):
            f = getattr(k, name_)
            f.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                          ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(OVERLAPPED)]
            f.restype = wintypes.BOOL
        k.GetOverlappedResult.argtypes = [wintypes.HANDLE, ctypes.POINTER(OVERLAPPED),
                                          ctypes.POINTER(wintypes.DWORD), wintypes.BOOL]
        k.GetOverlappedResult.restype = wintypes.BOOL
        k.CreateEventW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR]
        k.CreateEventW.restype = wintypes.HANDLE
        k.CancelIoEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
        k.CancelIoEx.restype = wintypes.BOOL
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        k.CloseHandle.restype = wintypes.BOOL
        deadline = time.monotonic() + timeout
        invalid = ctypes.c_void_p(-1).value
        while True:
            handle = k.CreateFileW(name, 0x80000000 | 0x40000000, 0, None, 3,
                                   0x40000000, None)  # FILE_FLAG_OVERLAPPED
            if handle != invalid:
                self._handle = handle
                return
            error = ctypes.get_last_error()
            if error not in (2, 231) or time.monotonic() >= deadline:
                raise ctypes.WinError(error)
            k.WaitNamedPipeW(name, 100)
            if error == 2:
                time.sleep(0.025)

    def _transfer(self, function, buffer, length: int) -> tuple[int, int]:
        """Run one overlapped operation; return (error, transferred)."""
        with self._lock:
            if self._closed:
                raise DisconnectedError("Named pipe closed")
            self._in_flight += 1
        k = self._dll
        event = k.CreateEventW(None, True, False, None)
        try:
            if not event:
                raise ctypes.WinError(ctypes.get_last_error())
            overlapped = self._overlapped(hEvent=event)
            count = self._wintypes.DWORD()
            error = 0
            if not function(self._handle, buffer, length, None, ctypes.byref(overlapped)):
                error = ctypes.get_last_error()
                if error != 997:  # ERROR_IO_PENDING
                    return error, 0
            # Waiting here keeps `buffer` and `overlapped` alive until the
            # kernel has finished with them, including after cancellation.
            if not k.GetOverlappedResult(self._handle, ctypes.byref(overlapped),
                                         ctypes.byref(count), True):
                return ctypes.get_last_error(), count.value
            return 0, count.value
        finally:
            if event:
                k.CloseHandle(event)
            with self._lock:
                self._in_flight -= 1
                self._idle.notify_all()

    def read(self, size: int) -> bytes:
        buffer = ctypes.create_string_buffer(min(size, 65536))
        try:
            error, count = self._transfer(self._dll.ReadFile, buffer, len(buffer))
        except DisconnectedError:
            return b""
        if error in self._ABORTED:
            return b""
        if error and error != 234:  # ERROR_MORE_DATA still delivered `count`
            raise ctypes.WinError(error)
        return buffer.raw[:count]

    def write(self, data: bytes | memoryview) -> int:
        part = bytes(data[:65536])
        buffer = ctypes.create_string_buffer(part, len(part))
        error, count = self._transfer(self._dll.WriteFile, buffer, len(part))
        if error in self._ABORTED:
            raise DisconnectedError("Named pipe closed")
        if error:
            raise ctypes.WinError(error)
        return count

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        if not self._close_when_idle(time.monotonic() + 1):
            # Never close/reuse a handle while an operation still owns it.
            threading.Thread(target=self._close_when_idle, args=(None,),
                             name="ui4me-pipe-close", daemon=True).start()

    def _close_when_idle(self, deadline):
        while True:
            with self._lock:
                if not self._in_flight:
                    self._dll.CloseHandle(self._handle)
                    return True
            # Repeat: an operation may be issued just after a cancellation.
            self._dll.CancelIoEx(self._handle, None)
            with self._idle:
                if self._in_flight:
                    self._idle.wait(0.01)
            if deadline is not None and time.monotonic() >= deadline:
                return False


def connect_stream(endpoint: str, timeout: float = 15.0):
    if os.name == "nt":
        return NamedPipeStream(endpoint, timeout)
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(timeout)
    try:
        sock.connect(endpoint)
        sock.settimeout(None)
        return SocketStream(sock)
    except BaseException:
        sock.close()
        raise


def read_exact(stream, length: int, *, clean_eof: bool = False) -> bytes | None:
    chunks = bytearray()
    while len(chunks) < length:
        piece = stream.read(length - len(chunks))
        if not piece:
            if clean_eof and not chunks:
                return None
            raise DisconnectedError("Connection closed inside a frame")
        chunks.extend(piece)
    return bytes(chunks)


def _unique_object(items):
    result = {}
    for key, value in items:
        if key in result:
            raise ProtocolError(f"Duplicate JSON member: {key}")
        result[key] = value
    return result


def encode_frame(value: dict) -> bytes:
    body = json.dumps(value, ensure_ascii=False, allow_nan=False,
                      separators=(",", ":")).encode("utf-8")
    if not body or len(body) > MAX_FRAME_BYTES:
        raise ProtocolError("Message exceeds the frame limit")
    return struct.pack(">I", len(body)) + body


def read_frame(stream) -> dict | None:
    header = read_exact(stream, 4, clean_eof=True)
    if header is None:
        return None
    length, = struct.unpack(">I", header)
    if length == 0 or length > MAX_FRAME_BYTES:
        raise ProtocolError("Invalid frame size")
    body = read_exact(stream, length)
    try:
        value = json.loads(body.decode("utf-8"), object_pairs_hook=_unique_object,
                           parse_constant=lambda _: (_ for _ in ()).throw(ProtocolError("Non-finite number")))
    except (UnicodeError, ValueError, RecursionError) as error:
        raise ProtocolError("Invalid UTF-8 JSON") from error
    if not isinstance(value, dict) or value.get("jsonrpc") != "2.0":
        raise ProtocolError("Expected a JSON-RPC 2.0 object")
    return value


def write_all(stream, frame: bytes) -> None:
    remaining = memoryview(frame)
    while remaining:
        size = stream.write(remaining)
        if not size:
            raise DisconnectedError("Connection closed while writing")
        remaining = remaining[size:]


def use_utf8_stdio() -> None:
    """Write UTF-8 to stdout/stderr when they are the host's log pipes.

    On Windows, Python encodes a piped stdout with the ANSI code page, so
    printing text such as "→" or "😀" raised UnicodeEncodeError. The host
    decodes backend output as UTF-8.
    """
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            reconfigure(encoding="utf-8", errors="backslashreplace")


class RpcClient:
    def __init__(self, stream, *, workers: int = 4):
        self._stream = stream
        self._lock = threading.RLock()
        self._pending: dict[str, concurrent.futures.Future] = {}
        self._next_id = 0
        self._closed = threading.Event()
        self.shutdown_requested = threading.Event()
        self._handlers: dict[str, Callable[[dict], Any]] = {}
        self._notifications: dict[str, Callable[[dict], None]] = {}
        self._outgoing = queue.Queue(maxsize=256)
        self._callbacks = queue.Queue(maxsize=128)
        self._queued_bytes = 0
        self.dropped_notifications = 0
        self._failure: BaseException | None = None
        self.on_request("window.before_close", lambda _: {"allow": True})
        self.on_request("app.before_quit", lambda _: {"allow": True})
        self.on_request("app.shutdown", self._shutdown)
        self.on_notification("app.shutdown", self._shutdown)
        self._threads = [
            threading.Thread(target=self._reader, name="ui4me-receiver", daemon=True),
            threading.Thread(target=self._writer, name="ui4me-writer", daemon=True),
        ] + [threading.Thread(target=self._callback_worker, name=f"ui4me-callback-{i}", daemon=True)
             for i in range(workers)]
        for thread in self._threads:
            thread.start()

    @classmethod
    def from_environment(cls):
        use_utf8_stdio()
        client = cls(connect_stream(os.environ["UI4ME_ENDPOINT"]))
        try:
            client.hello = client.call("host.hello", {
                "token": os.environ["UI4ME_TOKEN"], "protocol_version": 1,
                "client": {"name": "ui4me-python-reference", "version": "0.1.0"},
            })
            return client
        except BaseException:
            client.close()
            raise

    def _shutdown(self, _):
        return {"ok": True}

    def on_request(self, method: str, callback: Callable[[dict], Any]) -> None:
        self._handlers[method] = callback

    def on_notification(self, method: str, callback: Callable[[dict], None]) -> None:
        self._notifications[method] = callback

    def _enqueue(self, message: dict) -> None:
        frame = encode_frame(message)
        with self._lock:
            if self._closed.is_set():
                raise DisconnectedError("Connection is closed") from self._failure
            if self._queued_bytes + len(frame) > MAX_QUEUED_BYTES:
                raise RpcError(-32005, "Outgoing buffer is full")
            try:
                self._outgoing.put_nowait(frame)
            except queue.Full:
                raise RpcError(-32005, "Outgoing queue is full") from None
            self._queued_bytes += len(frame)

    def call_async(self, method: str, params: dict | None = None):
        future = concurrent.futures.Future()
        with self._lock:
            if self._closed.is_set():
                raise DisconnectedError("Connection is closed")
            if len(self._pending) >= MAX_PENDING:
                raise RpcError(-32005, "Too many outstanding requests")
            self._next_id += 1
            request_id = f"py-{self._next_id}"
            self._pending[request_id] = future
        future.request_id = request_id
        try:
            self._enqueue({"jsonrpc": "2.0", "id": request_id,
                           "method": method, "params": params or {}})
        except BaseException:
            with self._lock:
                self._pending.pop(request_id, None)
            raise
        def cancelled(done):
            if done.cancelled():
                with self._lock:
                    self._pending.pop(request_id, None)
                try:
                    self.notify("request.cancel", {"id": request_id})
                except (DisconnectedError, RpcError):
                    pass
        future.add_done_callback(cancelled)
        return future

    def call(self, method: str, params: dict | None = None, *, timeout=_DEFAULT_TIMEOUT):
        # User-operated native dialogs have no implicit deadline. An explicit
        # caller timeout still requests cancellation and leaves outcome advisory.
        if timeout is _DEFAULT_TIMEOUT:
            timeout = None if method.startswith("dialog.") else 15.0
        future = self.call_async(method, params)
        try:
            return future.result(timeout)
        except concurrent.futures.TimeoutError:
            future.cancel()  # Outcome may be unknown; this is not a rollback.
            raise

    def notify(self, method: str, params: dict | None = None) -> None:
        self._enqueue({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def _writer(self):
        try:
            while not self._closed.is_set():
                try:
                    frame = self._outgoing.get(timeout=0.2)
                except queue.Empty:
                    continue
                try:
                    write_all(self._stream, frame)
                finally:
                    with self._lock:
                        self._queued_bytes -= len(frame)
        except BaseException as error:
            self._fail(error)

    def _reader(self):
        try:
            while not self._closed.is_set():
                message = read_frame(self._stream)
                if message is None:
                    raise DisconnectedError("Host disconnected")
                if "method" in message:
                    if not isinstance(message["method"], str) or not isinstance(message.get("params", {}), dict):
                        raise ProtocolError("Invalid host request")
                    if "id" in message and not isinstance(message["id"], str):
                        raise ProtocolError("Request IDs must be strings")
                    try:
                        self._callbacks.put_nowait(message)
                    except queue.Full:
                        if "id" in message:
                            self._enqueue({"jsonrpc": "2.0", "id": message["id"],
                                           "error": {"code": -32005, "message": "Callback queue is full"}})
                        else:
                            self.dropped_notifications += 1
                else:
                    request_id = message.get("id")
                    if not isinstance(request_id, str) or (("result" in message) == ("error" in message)):
                        raise ProtocolError("Invalid response")
                    with self._lock:
                        future = self._pending.pop(request_id, None)
                    if future is None or future.done():
                        continue  # Late response to a cancelled request.
                    if "error" in message:
                        e = message["error"]
                        if not isinstance(e, dict) or not isinstance(e.get("code"), int) or not isinstance(e.get("message"), str):
                            if not future.done():
                                future.set_exception(ProtocolError("Invalid RPC error"))
                            raise ProtocolError("Invalid RPC error")
                        try:
                            future.set_exception(RpcError(e["code"], e["message"], e.get("data")))
                        except concurrent.futures.InvalidStateError:
                            pass
                    else:
                        try:
                            future.set_result(message["result"])
                        except concurrent.futures.InvalidStateError:
                            pass
        except BaseException as error:
            self._fail(error)

    def _callback_worker(self):
        while not self._closed.is_set():
            try:
                message = self._callbacks.get(timeout=0.2)
            except queue.Empty:
                continue
            method, params = message["method"], message.get("params", {})
            if "id" not in message:
                callback = self._notifications.get(method)
                if callback:
                    try:
                        callback(params)
                    except Exception as error:
                        print(f"Notification handler failed: {type(error).__name__}: {error}", flush=True)
                if method == "app.shutdown":
                    self.shutdown_requested.set()
                continue
            try:
                callback = self._handlers.get(method)
                if callback is None:
                    raise RpcError(-32601, f"Unknown method: {method}")
                result = callback(params)
                response = {"jsonrpc": "2.0", "id": message["id"], "result": result}
            except RpcError as error:
                response = {"jsonrpc": "2.0", "id": message["id"], "error": {
                    "code": error.code, "message": error.message, "data": error.data}}
            except Exception as error:
                response = {"jsonrpc": "2.0", "id": message["id"], "error": {
                    "code": -32603, "message": f"Callback failed: {type(error).__name__}"}}
            try:
                self._enqueue(response)
                if method == "app.shutdown":
                    self.shutdown_requested.set()
            except (DisconnectedError, RpcError):
                return

    def _fail(self, error: BaseException):
        with self._lock:
            if self._closed.is_set():
                return
            self._failure = error
            self._closed.set()
            pending, self._pending = self._pending, {}
        self._stream.close()
        for future in pending.values():
            if not future.done():
                try:
                    future.set_exception(DisconnectedError(str(error)))
                except concurrent.futures.InvalidStateError:
                    pass

    @property
    def closed(self) -> bool:
        return self._closed.is_set()

    def wait(self, timeout: float | None = None) -> bool:
        return self._closed.wait(timeout)

    def flush(self, timeout: float = 2.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                if self._queued_bytes == 0:
                    return
            if self._closed.wait(0.01):
                return
        raise TimeoutError("Outgoing messages did not drain")

    def close(self):
        self._fail(DisconnectedError("Client closed"))
        for thread in self._threads:
            if thread is not threading.current_thread():
                thread.join(timeout=1.0)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
