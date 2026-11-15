"""UI4ME host IPC client, using only the Python standard library.

Most applications use ``ui4me_core.Application``; this module is the transport
underneath it. The reader and writer never run application callbacks. Callbacks may therefore
await host RPCs. Both directions use the same length-prefixed JSON-RPC profile.
"""

from __future__ import annotations

import collections
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
import traceback
from typing import Any, Callable

CLIENT_VERSION = "1.0.1"  # Kept equal to ui4me_core.__version__ (checked by tests).

MAX_FRAME_BYTES = 2 * 1024 * 1024
MAX_PENDING = 128
MAX_QUEUED_BYTES = 8 * 1024 * 1024
MAX_QUEUED_CALLBACKS = 128
_DEFAULT_TIMEOUT = object()

# Delivery classes for host messages waiting for a callback worker (see
# classify_message and docs/reference/protocol.md, "Writing a binding").
CONTROL = "control"          # never dropped; bypasses the queue bound
DISCRETE = "discrete"        # dropped only when nothing coalescible is queued
COALESCIBLE = "coalescible"  # dropped first; a later one supersedes it

_CONTROL_METHODS = frozenset({
    "app.shutdown", "app.activated", "request.cancelled", "document.error",
    "desktop.preferences_changed",
})
_LIFECYCLE_REQUESTS = frozenset({"window.before_close", "app.before_quit", "app.shutdown"})
_CONTROL_WINDOW_EVENTS = frozenset({"closed", "close_requested"})
_COALESCIBLE_WINDOW_EVENTS = frozenset({"resized", "scale_changed"})
_COALESCIBLE_DOCUMENT_EVENTS = frozenset({
    "pointermove", "pointerover", "pointerout", "mousemove", "mouseover", "mouseout",
    "wheel", "scroll", "selectionchange", "dragover", "drag_feedback",
})


def classify_message(method: str, params: dict | None, *, request: bool = False) -> str:
    """Return how a host message may be treated when callbacks fall behind.

    Lifecycle requests and control notifications (window ``closed`` and
    ``close_requested``, ``document.error``, ``app.shutdown``,
    ``app.activated``, ``request.cancelled``, ``desktop.preferences_changed``)
    are ``CONTROL``. High-frequency, non-semantic events (pointer and mouse
    moves, ``wheel``, ``scroll``, ``selectionchange``, drag-over, CSS
    ``transition*``/``animation*``, window ``resized``/``scale_changed``) are
    ``COALESCIBLE``. Everything else, such as ``click`` or ``input``, is
    ``DISCRETE``.
    """
    if request:
        return CONTROL if method in _LIFECYCLE_REQUESTS else DISCRETE
    if method in _CONTROL_METHODS:
        return CONTROL
    params = params if isinstance(params, dict) else {}
    if method == "window.event":
        kind = params.get("type")
        if kind in _CONTROL_WINDOW_EVENTS:
            return CONTROL
        return COALESCIBLE if kind in _COALESCIBLE_WINDOW_EVENTS else DISCRETE
    if method == "document.event":
        event = params.get("event")
        kind = event.get("type") if isinstance(event, dict) else None
        if isinstance(kind, str) and (kind in _COALESCIBLE_DOCUMENT_EVENTS
                                      or kind.startswith(("transition", "animation"))):
            return COALESCIBLE
    return DISCRETE


class _CallbackQueue:
    """A bounded FIFO for callback workers that knows which messages matter.

    ``CONTROL`` messages are always admitted and never evicted (their volume
    is bounded by the host: one per window, document, request or lifecycle
    step). When ``capacity`` other messages are queued, a new message first
    evicts the oldest queued ``COALESCIBLE`` one; if there is none, the new
    message is refused. Kept messages stay in arrival order.
    """

    def __init__(self, capacity: int):
        self._items: collections.deque = collections.deque()
        self._capacity = capacity
        self._bounded = 0
        self._ready = threading.Condition(threading.Lock())
        self.unfinished_tasks = 0
        self.drained = threading.Event()  # set whenever the queue becomes empty
        self.drained.set()

    def put(self, message: dict, kind: str) -> tuple[bool, list]:
        """Queue ``message``; return (admitted, evicted [(message, kind)])."""
        evicted = []
        with self._ready:
            if kind != CONTROL and self._bounded >= self._capacity:
                for index, (queued, queued_kind) in enumerate(self._items):
                    if queued_kind == COALESCIBLE:
                        del self._items[index]
                        self._bounded -= 1
                        self.unfinished_tasks -= 1
                        evicted.append((queued, queued_kind))
                        break
                else:
                    return False, evicted
            self._items.append((message, kind))
            if kind != CONTROL:
                self._bounded += 1
            self.unfinished_tasks += 1
            self.drained.clear()
            self._ready.notify()
        return True, evicted

    def get(self, timeout: float) -> dict:
        with self._ready:
            if not self._items:
                self._ready.wait(timeout)
            if not self._items:
                raise queue.Empty
            message, kind = self._items.popleft()
            if kind != CONTROL:
                self._bounded -= 1
            if not self._items:
                self.drained.set()
            return message

    def task_done(self) -> None:
        with self._ready:
            self.unfinished_tasks -= 1

    def qsize(self) -> int:
        with self._ready:
            return len(self._items)


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


def _log(text: str) -> None:
    try:
        print(f"UI4ME: {text}", file=sys.stderr, flush=True)
    except Exception:  # Logging must never fail the transport.
        pass


def _exit_status(error: BaseException) -> int:
    """The process exit status Python would use for this exception."""
    if isinstance(error, KeyboardInterrupt):
        return 130
    code = getattr(error, "code", None)
    if code is None:
        return 0
    if isinstance(code, int) and not isinstance(code, bool):
        return code
    _log(str(code))
    return 1


if issubclass(concurrent.futures.TimeoutError, TimeoutError):  # Python 3.11+
    _CallTimeout = TimeoutError
else:  # pragma: no cover - Python 3.10
    class _CallTimeout(TimeoutError, concurrent.futures.TimeoutError):
        """A call() timeout; both the builtin and the futures class catch it."""


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
    def __init__(self, stream, *, workers: int = 4, ordered_notifications: bool = False):
        self._stream = stream
        self._lock = threading.RLock()
        self._pending: dict[str, concurrent.futures.Future] = {}
        self._next_id = 0
        self._closed = threading.Event()
        self.shutdown_requested = threading.Event()
        self._handlers: dict[str, Callable[[dict], Any]] = {}
        self._notifications: dict[str, Callable[[dict], None]] = {}
        self._outgoing = queue.Queue(maxsize=256)
        self._callbacks = _CallbackQueue(MAX_QUEUED_CALLBACKS)
        # Optional application-event ordering must not serialize lifecycle
        # policy requests behind a user callback waiting for a native dialog.
        # The default client retains its qualified callback-pool behavior.
        self._ordered_notifications = _CallbackQueue(MAX_QUEUED_CALLBACKS) if ordered_notifications else None
        self._queued_bytes = 0
        # Notifications discarded because callbacks fell behind, in total and
        # per delivery class. CONTROL is never dropped, so it stays 0.
        self.dropped_notifications = 0
        self.dropped_by_kind = {CONTROL: 0, DISCRETE: 0, COALESCIBLE: 0}
        # Notifications nobody subscribed to (see notification_filter); these
        # never occupy a queue slot and are not drops.
        self.filtered_notifications = 0
        # Optional reader-thread predicate (method, params) -> keep? It must
        # be fast and must not block or run application code.
        self.notification_filter: Callable[[str, dict], bool] | None = None
        self._received_hooks: dict[str, Callable[[dict], None]] = {}
        self._stats_lock = threading.Lock()
        self._burst_drops = 0
        # Exit status requested by SystemExit/KeyboardInterrupt in a callback.
        self.exit_status: int | None = None
        self._failure: BaseException | None = None
        self.on_request("window.before_close", lambda _: {"allow": True})
        self.on_request("app.before_quit", lambda _: {"allow": True})
        self.on_request("app.shutdown", self._shutdown)
        self.on_notification("app.shutdown", self._shutdown)
        self._io_threads = [
            threading.Thread(target=self._reader, name="ui4me-receiver", daemon=True),
            threading.Thread(target=self._writer, name="ui4me-writer", daemon=True),
        ]
        self._threads = self._io_threads + [threading.Thread(target=self._callback_worker, name=f"ui4me-callback-{i}", daemon=True)
             for i in range(workers)]
        if self._ordered_notifications is not None:
            self._threads.append(threading.Thread(target=self._callback_worker,
                args=(self._ordered_notifications,), name="ui4me-application-events", daemon=True))
        for thread in self._threads:
            thread.start()

    @classmethod
    def from_environment(cls, *, workers: int = 4, ordered_notifications: bool = False):
        use_utf8_stdio()
        client = cls(connect_stream(os.environ["UI4ME_ENDPOINT"]), workers=workers,
                     ordered_notifications=ordered_notifications)
        try:
            client.hello = client.call("host.hello", {
                "token": os.environ["UI4ME_TOKEN"], "protocol_version": 1,
                "client": {"name": "ui4me-python", "version": CLIENT_VERSION},
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

    def on_notification_received(self, method: str, hook: Callable[[dict], None] | None) -> None:
        """Run ``hook(params)`` on the reader thread as each such notification
        arrives, before it is queued for its callback. For bookkeeping only
        (marking a window closed): it must not block, take locks that user
        code can hold across RPCs, or run application callbacks."""
        if hook is None:
            self._received_hooks.pop(method, None)
        else:
            self._received_hooks[method] = hook

    def _count_drop(self, message: dict, kind: str) -> None:
        with self._stats_lock:
            self.dropped_notifications += 1
            self.dropped_by_kind[kind] += 1
            self._burst_drops += 1
            first = self._burst_drops == 1
        if first:
            event = message.get("params", {}).get("event") if isinstance(message.get("params"), dict) else None
            what = event.get("type") if isinstance(event, dict) else message.get("method")
            _log("event callbacks are behind (a callback is slow or blocked); dropping queued "
                 f"events, pointer/scroll/animation first (first dropped: {what}). Window "
                 "lifecycle, errors and shutdown are never dropped.")

    def _end_drop_burst(self) -> None:
        with self._stats_lock:
            dropped, self._burst_drops = self._burst_drops, 0
        if dropped:
            _log(f"event callbacks caught up; {dropped} event(s) were dropped "
                 f"({self.dropped_notifications} in total)")

    def _report_callback_error(self, error: BaseException, context: str, *, request_quit: bool = True) -> None:
        """Make a failed callback visible. SystemExit and KeyboardInterrupt
        raised in a callback request an orderly application quit instead of
        silently ending the worker thread; run() then exits with that status."""
        if isinstance(error, (SystemExit, KeyboardInterrupt)):
            status = _exit_status(error)
            if self.exit_status is None or (status and not self.exit_status):
                self.exit_status = status
            action = "requesting application quit" if request_quit else "allowing the quit"
            _log(f"{type(error).__name__}({status}) raised in {context}; {action} "
                 f"(exit status {status})")
            if request_quit and not self.closed:
                try:
                    future = self.call_async("app.request_quit")
                    future.add_done_callback(lambda done: done.cancelled() or done.exception())
                except (DisconnectedError, RpcError, ProtocolError):
                    pass
            return
        try:
            print(f"UI4ME: {context} failed:", file=sys.stderr)
            traceback.print_exception(type(error), error, error.__traceback__, file=sys.stderr)
            sys.stderr.flush()
        except Exception:  # Reporting must never kill the worker.
            pass

    report_callback_error = _report_callback_error

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
        except concurrent.futures.TimeoutError as error:
            future.cancel()  # Outcome may be unknown; this is not a rollback.
            if isinstance(error, TimeoutError):
                raise  # Python 3.11+: the same class as the builtin.
            # Python 3.10: concurrent.futures.TimeoutError is a separate class;
            # raise one that `except TimeoutError` (documented) also catches.
            raise _CallTimeout(f"{method} timed out after {timeout} s; its outcome is unknown") from None

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
                    self._admit(message)
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

    def _admit(self, message: dict) -> None:
        """Reader thread: classify, apply bookkeeping, queue. Never blocks."""
        method, params, request = message["method"], message.get("params", {}), "id" in message
        if not request:
            if self.notification_filter is not None:
                try:
                    keep = self.notification_filter(method, params)
                except Exception:
                    keep = True
                if not keep:
                    self.filtered_notifications += 1
                    return
            hook = self._received_hooks.get(method)
            if hook is not None:
                try:
                    hook(params)
                except Exception as error:  # Bookkeeping must never stop the reader.
                    self._report_callback_error(error, f"bookkeeping for {method}")
            if method == "app.shutdown":
                self.shutdown_requested.set()
        kind = classify_message(method, params, request=request)
        callbacks = self._callbacks
        if self._ordered_notifications is not None and not request and method != "app.shutdown":
            callbacks = self._ordered_notifications
        admitted, evicted = callbacks.put(message, kind)
        for dropped, dropped_kind in evicted:
            self._count_drop(dropped, dropped_kind)
        if admitted:
            return
        if request:
            self._enqueue({"jsonrpc": "2.0", "id": message["id"],
                           "error": {"code": -32005, "message": "Callback queue is full"}})
        else:
            self._count_drop(message, kind)

    def _callback_worker(self, callbacks=None):
        callbacks = self._callbacks if callbacks is None else callbacks
        while not self._closed.is_set():
            try:
                message = callbacks.get(timeout=0.2)
            except queue.Empty:
                if self._burst_drops and self._all_drained():
                    self._end_drop_burst()
                continue
            try:
                if not self._dispatch(message):
                    return
            except BaseException as error:  # Never let one message end the worker.
                self._report_callback_error(error, f"handling of {message.get('method')}")
            finally:
                callbacks.task_done()
            if self._burst_drops and self._all_drained():
                self._end_drop_burst()

    def _all_drained(self) -> bool:
        return self._callbacks.drained.is_set() and (
            self._ordered_notifications is None or self._ordered_notifications.drained.is_set())

    def wait_for_notifications(self, timeout: float) -> bool:
        """Wait until ordered notifications already received have been handled.

        The host sends a window's final ``closed`` event before ``app.shutdown``,
        but shutdown is handled outside the ordered queue. Returns False on
        timeout (for example a callback still waiting for a native dialog).
        """
        if self._ordered_notifications is None:
            return True
        deadline = time.monotonic() + timeout
        while self._ordered_notifications.unfinished_tasks:
            if self._closed.is_set() or time.monotonic() >= deadline:
                return False
            time.sleep(0.01)
        return True

    def _dispatch(self, message) -> bool:
        """Handle one host message; False when the connection is gone."""
        method, params = message["method"], message.get("params", {})
        if "id" not in message:
            callback = self._notifications.get(method)
            if callback:
                try:
                    callback(params)
                except BaseException as error:
                    self._report_callback_error(error, f"{method} notification handler")
            if method == "app.shutdown":
                self.shutdown_requested.set()
            return True
        try:
            callback = self._handlers.get(method)
            if callback is None:
                raise RpcError(-32601, f"Unknown method: {method}")
            result = callback(params)
            response = {"jsonrpc": "2.0", "id": message["id"], "result": result}
        except RpcError as error:
            response = {"jsonrpc": "2.0", "id": message["id"], "error": {
                "code": error.code, "message": error.message, "data": error.data}}
        except BaseException as error:
            # The host treats an error answer to a policy request as "deny".
            self._report_callback_error(error, f"{method} request handler")
            response = {"jsonrpc": "2.0", "id": message["id"], "error": {
                "code": -32603, "message": f"Callback failed: {type(error).__name__}"}}
        try:
            try:
                self._enqueue(response)
            except (TypeError, ValueError, RecursionError, ProtocolError) as error:
                # json.dumps refused the result (NaN, an object, a cycle) or the
                # frame is too large: answer visibly instead of never answering.
                self._report_callback_error(error, f"encoding the result of {method}")
                self._enqueue({"jsonrpc": "2.0", "id": message["id"], "error": {
                    "code": -32603,
                    "message": f"Callback result is not JSON-serializable: {type(error).__name__}: {error}"[:1024]}})
            if method == "app.shutdown":
                self.shutdown_requested.set()
        except (DisconnectedError, RpcError):
            return False
        return True

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
            if self._ordered_notifications is not None and thread not in self._io_threads:
                # User code may intentionally be blocked. These daemon workers
                # do not own transport resources; shutdown must not await them.
                continue
            if thread is not threading.current_thread():
                thread.join(timeout=1.0)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
