"""The UI4ME Python API: applications, windows, documents and nodes.

The Rust host owns native windows, input, editing and rendering. Python owns
application logic and talks to the host over the private IPC protocol.
Application notifications are serialized on one event worker; independent
transport and lifecycle-request workers continue resolving RPC and close/quit
policy, so a notification callback can issue document commands or wait for a
native dialog without blocking lifecycle control.
"""
from __future__ import annotations

from collections.abc import Callable, Iterable
import base64
import os
from pathlib import Path
import sys
import threading
import traceback
from typing import Any

from .client import DisconnectedError, RpcClient, RpcError

CLOSED = -32004
INVALID_STATE = -32008


class StaleDocumentError(RuntimeError):
    """The document was replaced, unloaded, or its window was closed."""


class HostStaleError(StaleDocumentError, RpcError):
    """The host answered ``-32004``: the window, document or bitmap no longer
    exists (for example its ``closed`` event had not been processed yet).

    It is both a ``StaleDocumentError`` (the documented contract for stale
    handles) and an ``RpcError`` with ``code == -32004`` (what 1.0.1 raised).
    The facade is retired when this is raised.
    """

    def __init__(self, code: int, message: str, data: Any = None):
        RpcError.__init__(self, code, message, data)


class Application:
    """A UI4ME application: native windows owned by the Rust host, logic here.

    ``Application.start()`` is the normal entry point for ``python app.py``.
    ``Application.from_environment()`` only connects to a host that already
    launched this process from a manifest. ``run()`` announces readiness and
    waits for native shutdown. No native event loop runs in this process. The
    exposed ``rpc`` remains available for desktop services.
    """

    def __init__(self, rpc: RpcClient):
        self.rpc = rpc
        self._lock = threading.RLock()
        self._windows: dict[str, Window] = {}
        self._ready = False
        self._window_listeners: list[Callable[[dict], Any]] = []
        self._shutdown_listeners: list[Callable[[], Any]] = []
        self._activated_listeners: list[Callable[[dict], Any]] = []
        # Windows/documents retired on the transport's reader thread whose
        # full cleanup and listener calls are still queued for the event worker.
        self._closing: dict[str, Window] = {}
        self._failed_documents: dict[str, Document] = {}
        # Windows whose first load failed before app.ready (see open_html).
        self._abandoned: list[Window] = []
        self._warned_policies: set[int] = set()
        self._on_document_event = self._document_event
        self.rpc.on_notification("document.event", self._on_document_event)
        self.rpc.on_notification("document.error", self._document_error)
        self.rpc.on_notification("window.event", self._window_event)
        self.rpc.on_notification("app.activated", self._activated)
        received = getattr(self.rpc, "on_notification_received", None)
        if received is not None:
            # Control bookkeeping is applied as the message arrives, so a slow
            # or blocked event callback cannot delay or lose it (F-29).
            received("window.event", self._window_event_received)
            received("document.error", self._document_error_received)
            self.rpc.notification_filter = self._wants_notification

    @classmethod
    def start(cls, *, name: str | None = None, app_id: str | None = None,
              version: str | None = None, icon: str | Path | None = None,
              instance: str = "single", startup_timeout_ms: int = 30_000,
              shutdown_timeout_ms: int = 5_000, max_windows: int = 16) -> Application:
        """Start a native UI4ME application from a normal Python script.

        Run directly (``python app.py``, ``uv run app.py`` or ``python -m
        package.app``), this starts the packaged native host, which runs the
        same command again as its supervised backend; this first process
        then exits with the host's status and never returns from here. In
        that second run, the call connects to the host and returns the
        ``Application``. Code before ``start()`` therefore runs twice: keep
        side effects after it.

        ``name`` is the window/application name (default: the script name).
        ``app_id`` is a reverse-DNS identity; without it, a stable ID is derived
        from the script path. ``instance="single"`` forwards a second launch
        of the same app to the running one; ``"multiple"`` allows several.
        Already inside a host-launched backend (``ui4me-host run --app``),
        the options are validated and the call simply connects. Call it once
        per process: a second call raises ``StartupError``.
        """
        from . import _bootstrap

        _bootstrap.validate_options(name=name, app_id=app_id, version=version, instance=instance,
                                    startup_timeout_ms=startup_timeout_ms,
                                    shutdown_timeout_ms=shutdown_timeout_ms, max_windows=max_windows)
        _bootstrap.claim_start()
        if _bootstrap.managed():
            app = cls.from_environment()
            _bootstrap.forget_host_environment()
            return app
        raise SystemExit(_bootstrap.launch(
            name=name, app_id=app_id, version=version, icon=icon, instance=instance,
            startup_timeout_ms=startup_timeout_ms, shutdown_timeout_ms=shutdown_timeout_ms,
            max_windows=max_windows))

    @classmethod
    def from_environment(cls) -> Application:
        """Connect to the native host that launched this process from a manifest."""
        from ._bootstrap import ENDPOINT, TOKEN, StartupError

        if not (os.environ.get(ENDPOINT) and os.environ.get(TOKEN)):
            raise StartupError(
                "No UI4ME host connection: UI4ME_ENDPOINT and UI4ME_TOKEN are set only for a backend "
                "that the native host started. Use Application.start() to run a script directly, "
                "or launch it with: ui4me-host run --app app.toml")
        # Document/window events are ordered, while the existing four-worker
        # host request pool remains able to answer close/quit independently.
        return cls(RpcClient.from_environment(ordered_notifications=True))

    def create_window(self, *, title: str = "UI4ME", width: int = 900,
                      height: int = 600, visible: bool = True, decorations: bool = True,
                      min_width: int | None = None, min_height: int | None = None,
                      max_width: int | None = None, max_height: int | None = None) -> Window:
        options = {
            "title": title, "width": width, "height": height, "visible": visible,
            "decorations": decorations,
        }
        for key, value in (("min_width", min_width), ("min_height", min_height),
                           ("max_width", max_width), ("max_height", max_height)):
            if value is not None:
                options[key] = value
        state = self.rpc.call("window.create", options)
        window = Window(self, state["window_id"])
        with self._lock:
            self._windows[window.id] = window
        return window

    def open_file(self, path: str | Path, **window_options) -> Window:
        """Create a window and load an HTML file into it.

        If loading fails, the exception is raised with the new window as
        ``error.window``. After ``run()``/``ready()`` that window is closed.
        Before it, closing a window would cancel the whole application, so
        the empty window is kept: load a fallback into ``error.window``, or
        open another window, in which case the unused one is closed as soon
        as the application is ready.
        """
        window = self.create_window(**window_options)
        try:
            window.load_file(path)
        except BaseException as error:
            self._abandon(window, error)
            raise
        return window

    def open_html(self, html: str, *, asset_root: str | Path | None = None,
                  **window_options) -> Window:
        """Create a window and load ``html`` into it (see ``open_file`` for failures)."""
        window = self.create_window(**window_options)
        try:
            window.load_html(html, asset_root=asset_root)
        except BaseException as error:
            self._abandon(window, error)
            raise
        return window

    def _abandon(self, window: Window, error: BaseException) -> None:
        try:
            error.window = window  # type: ignore[attr-defined]
        except Exception:
            pass
        if not self._ready:
            # The host cancels startup when a window closes before app.ready.
            self._abandoned.append(window)
            return
        try:
            window.request_close()
        except Exception:  # Never hide the load error behind a cleanup error.
            pass

    def ready(self) -> None:
        if not self._ready:
            self.rpc.call("app.ready")
            self._ready = True
            abandoned, self._abandoned = self._abandoned, []
            for window in abandoned:
                # A first load failed before readiness and the window was never
                # reused; close it now that closing no longer cancels startup.
                with self._lock:
                    others = any(w is not window and not w.closed for w in self._windows.values())
                if others and not window.closed and window.document is None:
                    try:
                        window.request_close()
                    except Exception:
                        pass

    def run(self) -> None:
        """Announce readiness and wait for an orderly native shutdown.

        Before returning, events the host sent ahead of the shutdown (such as
        the last window's ``closed`` event) are delivered (for up to two
        seconds), then ``on_shutdown`` callbacks run in this thread. A quit
        requested before ``run()`` (``request_quit()`` during startup) is an
        orderly shutdown, not an error. If a callback raised ``SystemExit``
        (``sys.exit(n)``) or ``KeyboardInterrupt``, the application was asked
        to quit, and ``run()`` finally raises ``SystemExit`` with that status.
        """
        if not self.rpc.shutdown_requested.is_set():
            try:
                self.ready()
            except RpcError as error:
                # Quitting during startup makes app.ready fail with -32008;
                # app.shutdown follows at once in that case.
                if error.code != INVALID_STATE or not self.rpc.shutdown_requested.wait(2.0):
                    raise
        while not self.rpc.shutdown_requested.wait(0.1):
            if self.rpc.closed:
                raise DisconnectedError("The host disconnected before orderly shutdown")
        drain = getattr(self.rpc, "wait_for_notifications", None)
        if drain is not None:
            drain(2.0)
        for callback in list(self._shutdown_listeners):
            try:
                callback()
            except Exception as error:  # One failing hook must not skip the others.
                self._report(error, "on_shutdown callback")
        status = getattr(self.rpc, "exit_status", None)
        if status:
            raise SystemExit(status)

    def on_window_event(self, callback: Callable[[dict], Any]) -> Callable[[], None]:
        """Call ``callback(params)`` for every native window event.

        ``params`` holds ``window_id``, ``type`` (for example ``"closed"``,
        ``"resized"``, ``"focused"``) and the event's fields. Callbacks run in
        event order, after the window bookkeeping of this API (a ``closed``
        window is already retired). Returns a function that unsubscribes.
        """
        with self._lock:
            self._window_listeners.append(callback)

        def unsubscribe() -> None:
            with self._lock:
                if callback in self._window_listeners:
                    self._window_listeners.remove(callback)
        return unsubscribe

    def on_shutdown(self, callback: Callable[[], Any]) -> None:
        """Run ``callback()`` once the host has shut the application down, in the
        thread that called ``run()``, after the final events were delivered."""
        with self._lock:
            self._shutdown_listeners.append(callback)

    def on_activated(self, callback: Callable[[dict], Any]) -> Callable[[], None]:
        """Call ``callback(params)`` when a second launch of a single-instance
        app reached this one (``app.activated``: ``args``, ``cwd``,
        ``activation_token``). ``args`` is the second host's command line,
        not the script's arguments. Returns a function that unsubscribes."""
        with self._lock:
            self._activated_listeners.append(callback)

        def unsubscribe() -> None:
            with self._lock:
                if callback in self._activated_listeners:
                    self._activated_listeners.remove(callback)
        return unsubscribe

    def request_quit(self) -> None:
        self.rpc.call("app.request_quit")

    def on_before_quit(self, callback: Callable[[], bool]) -> None:
        """Decide whether the application may quit: return ``True`` to allow.

        Anything else keeps it running: ``False``, an exception (printed with
        its traceback) or a missing ``return`` (``None``; warned once). Other
        values are tested for truth, with a warning. ``sys.exit()`` inside it
        allows the quit."""
        self.rpc.on_request("app.before_quit", lambda _: {
            "allow": self._decide(callback, "on_before_quit", "quitting", quit_policy=True)})

    def _before_close(self, params: dict) -> dict:
        with self._lock:
            window = self._windows.get(params.get("window_id"))
            callback = getattr(window, "_before_close", None)
        if callback is None:
            return {"allow": True}
        return {"allow": self._decide(callback, f"on_before_close for window {window.id}", "closing")}

    def _decide(self, callback: Callable[[], Any], name: str, action: str, *,
                quit_policy: bool = False) -> bool:
        """Run a close/quit policy. Fail safe (keep open) but never silently."""
        try:
            result = callback()
        except (SystemExit, KeyboardInterrupt) as error:
            # sys.exit() in a policy means "quit": a quit policy allows, a
            # close policy refuses this close and requests the quit instead.
            self._report(error, f"the {name} callback", request_quit=not quit_policy)
            return quit_policy
        except Exception as error:
            self._report(error, f"the {name} callback ({action} was refused)")
            return False
        if isinstance(result, bool):
            return result
        key = id(callback)
        if key not in self._warned_policies:
            self._warned_policies.add(key)
            meaning = "allow" if result else "keep open"
            print(f'UI4ME: {name} callback returned {result!r}; treating it as "{meaning}". '
                  f"Return True to allow {action}, False to refuse.", file=sys.stderr, flush=True)
        return bool(result)

    def _report(self, error: BaseException, context: str, *, request_quit: bool = True) -> None:
        report = getattr(self.rpc, "report_callback_error", None)
        if report is not None:
            report(error, context, request_quit=request_quit)
            return
        try:
            print(f"UI4ME: {context} failed:", file=sys.stderr)
            traceback.print_exception(type(error), error, error.__traceback__, file=sys.stderr)
        except Exception:
            pass

    def clipboard_text(self) -> str:
        return self.rpc.call("clipboard.read_text")["text"]

    def set_clipboard_text(self, text: str) -> None:
        self.rpc.call("clipboard.write_text", {"text": text})

    def capabilities(self) -> dict:
        return self.rpc.call("host.capabilities")

    def desktop_preferences(self) -> dict:
        """Observed preferences, including unknown values and their source."""
        return self.rpc.call("desktop.preferences")

    def on_desktop_preferences(self, callback: Callable[[dict], Any]) -> None:
        self.rpc.on_notification("desktop.preferences_changed", callback)

    def open_external(self, target: str | Path, *, window: Window | None = None) -> dict:
        params: dict[str, Any] = {"target": str(target)}
        if window is not None:
            window._check()
            if window.app is not self:
                raise ValueError("Window belongs to another application")
            params["window_id"] = window.id
        return self.rpc.call("desktop.open_external", params, timeout=60)

    def reveal_file(self, path: str | Path, *, window: Window | None = None) -> dict:
        params: dict[str, Any] = {"path": str(Path(path).expanduser().resolve())}
        if window is not None:
            window._check()
            if window.app is not self:
                raise ValueError("Window belongs to another application")
            params["window_id"] = window.id
        return self.rpc.call("desktop.reveal_file", params, timeout=60)

    def _wants_notification(self, method: str, params: dict) -> bool:
        """Reader thread: drop document events no handler is waiting for, so
        they never occupy a callback queue slot. Lock-free reads only."""
        if method != "document.event" or self.rpc._notifications.get(method) != self._on_document_event:
            return True
        window = self._windows.get(params.get("window_id"))
        document = window.document if window is not None and not window.closed else None
        if document is None or document._retired or document.id != params.get("document_id"):
            return False  # It belongs to a retired document and would be ignored.
        event = params.get("event")
        kind = event.get("type") if isinstance(event, dict) else None
        return any(entry[0] == kind for entry in list(document._handlers))

    def _window_event_received(self, params: dict) -> None:
        """Reader thread: a closed window is stale at once (lock-free)."""
        if params.get("type") != "closed":
            return
        window = self._windows.pop(params.get("window_id"), None)
        if window is not None:
            self._closing[window.id] = window
            window.closed = True
            if window.document is not None:
                window.document._retired = True

    def _document_error_received(self, params: dict) -> None:
        """Reader thread: a fatally failed document is stale at once."""
        if not params.get("fatal"):
            return
        window = self._windows.get(params.get("window_id"))
        document = window.document if window is not None else None
        if document is not None and document.id == params.get("document_id"):
            self._failed_documents[document.id] = document
            document._retired = True

    def _document_event(self, params: dict) -> None:
        with self._lock:
            window = self._windows.get(params.get("window_id"))
            document = window.document if window and not window.closed else None
            if document is None or document.id != params.get("document_id"):
                return  # A queued event belongs to a retired document.
        event = params.get("event")
        if isinstance(event, dict):
            document._dispatch(event)

    def _window_event(self, params: dict) -> None:
        if params.get("type") == "closed":
            window_id = params.get("window_id")
            with self._lock:
                window = self._closing.pop(window_id, None) or self._windows.pop(window_id, None)
                if window:
                    window.closed = True
                    if window.document:
                        window.document._retire()
        with self._lock:
            listeners = list(self._window_listeners)
        for callback in listeners:
            try:
                callback(params)
            except BaseException as error:  # One failing listener must not skip the others.
                self._report(error, f"window event listener ({params.get('type')})")

    def _activated(self, params: dict) -> None:
        with self._lock:
            listeners = list(self._activated_listeners)
        for callback in listeners:
            try:
                callback(params)
            except BaseException as error:
                self._report(error, "on_activated callback")

    def _document_error(self, params: dict) -> None:
        with self._lock:
            document = self._failed_documents.pop(params.get("document_id"), None)
            if document is not None:
                window = document.window
                if window.closed:
                    document._retire()
                    return
            else:
                window = self._windows.get(params.get("window_id"))
                document = window.document if window and not window.closed else None
                if document is None or document.id != params.get("document_id"):
                    return
        message = str(params.get("message", "Document operation failed"))[:4096]
        fatal = bool(params.get("fatal", False))
        print(f"UI4ME document error ({window.id}, {document.id}): {message}", file=sys.stderr, flush=True)
        try:
            document._dispatch({"type": "error", "message": message, "fatal": fatal}, force=fatal)
        finally:
            if fatal:
                # The host has already unloaded the failed renderer. An error
                # handler may have loaded a replacement into the still-live
                # window; never retire that replacement in this cleanup.
                with self._lock:
                    document._retire()
                    if window.document is document:
                        window.document = None

    def close(self) -> None:
        with self._lock:
            for window in list(self._windows.values()) + list(self._closing.values()):
                window.closed = True
                if window.document:
                    window.document._retire()
            self._windows.clear()
            self._closing.clear()
        self.rpc.close()

    def __enter__(self) -> Application:
        return self

    def __exit__(self, exc_type, exc, traceback):
        if exc is not None and not isinstance(exc, (SystemExit, KeyboardInterrupt)):
            # Closing the connection makes the host end this backend at once,
            # which would lose the traceback Python prints only afterwards.
            _report_before_disconnect(exc)
        self.close()


_REPORTED = "_ui4me_traceback_reported"


def _report_before_disconnect(error: BaseException) -> None:
    import traceback

    try:
        traceback.print_exception(type(error), error, error.__traceback__)
        sys.stderr.flush()
        setattr(error, _REPORTED, True)
    except Exception:  # Reporting must never replace the application's error.
        return
    previous = sys.excepthook
    if getattr(previous, "_ui4me_hook", False):
        return

    def hook(kind, value, trace):
        # Already shown above; anything else is reported normally.
        if not getattr(value, _REPORTED, False):
            previous(kind, value, trace)

    hook._ui4me_hook = True
    sys.excepthook = hook


class Window:
    def __init__(self, app: Application, window_id: str):
        self.app, self.id = app, window_id
        self.document: Document | None = None
        self.closed = False

    def _check(self) -> None:
        if self.closed:
            raise StaleDocumentError("Window is closed")

    def _call(self, method: str, params: dict, **options) -> Any:
        """A window request; host -32004 retires this window (HostStaleError)."""
        try:
            return self.app.rpc.call(method, params, **options)
        except RpcError as error:
            if error.code != CLOSED or isinstance(error, HostStaleError):
                raise
            with self.app._lock:
                self.closed = True
                if self.app._windows.get(self.id) is self:
                    del self.app._windows[self.id]
                    self.app._closing[self.id] = self  # Its closed event finishes the cleanup.
                if self.document is not None:
                    self.document._retired = True
            raise HostStaleError(error.code, error.message, error.data) from None

    def _load(self, **source) -> Document:
        if self.closed:
            raise StaleDocumentError("Window is closed")
        result = self._call("document.load", {"window_id": self.id, **source}, timeout=30)
        document = Document(self, result["document_id"])
        # Retire the old facade only after a successful atomic replacement.
        with self.app._lock:
            if self.document:
                self.document._retire()
            self.document = document
        return document

    def load_file(self, path: str | Path, *, asset_root: str | Path | None = None) -> Document:
        values: dict[str, Any] = {"path": str(Path(path).expanduser().resolve())}
        if asset_root is not None:
            values["asset_root"] = str(Path(asset_root).expanduser().resolve())
        return self._load(**values)

    def load_html(self, html: str, *, asset_root: str | Path | None = None) -> Document:
        values: dict[str, Any] = {"html": html}
        if asset_root is not None:
            values["asset_root"] = str(Path(asset_root).expanduser().resolve())
        return self._load(**values)

    def set(self, **properties) -> dict:
        self._check()
        return self._call("window.set", {"window_id": self.id, **properties})

    def state(self) -> dict:
        self._check()
        return self._call("window.get", {"window_id": self.id})

    def request_attention(self, kind: str | None = "informational") -> dict:
        self._check()
        return self._call("window.request_attention", {"window_id": self.id, "kind": kind})

    def on_before_close(self, callback: Callable[[], bool]) -> None:
        """Decide whether this window may close: return ``True`` to allow.

        Anything else keeps it open: ``False``, an exception (printed with its
        traceback) or a missing ``return`` (``None``; warned once). Other
        values are tested for truth, with a warning. ``sys.exit()`` inside it
        keeps the window open and requests an application quit."""
        self._check()
        # A request callback is installed once per Application; route by window
        # rather than replacing another window's policy when this is called.
        self._before_close = callback
        self.app.rpc.on_request("window.before_close", self.app._before_close)

    def request_close(self) -> None:
        self._check()
        self._call("window.request_close", {"window_id": self.id})

    def _file_dialog(self, kind: str, *, wait: bool, **options):
        self._check()
        params = {"window_id": self.id, **{k: v for k, v in options.items() if v is not None}}
        # Returning the transport future preserves its cancellation semantics:
        # future.cancel() asks the host to close the actual native dialog.
        if wait:
            return self._call("dialog." + kind, params, timeout=None)
        return self.app.rpc.call_async("dialog." + kind, params)

    def open_files(self, *, multiple: bool = False, title: str | None = None,
                   directory: str | Path | None = None, filters: Iterable[dict] = (),
                   wait: bool = True):
        return self._file_dialog("open_files", wait=wait, multiple=multiple, title=title,
            directory=str(directory) if directory is not None else None, filters=list(filters))

    def save_file(self, *, file_name: str | None = None, title: str | None = None,
                  directory: str | Path | None = None, filters: Iterable[dict] = (),
                  wait: bool = True):
        return self._file_dialog("save_file", wait=wait, file_name=file_name, title=title,
            directory=str(directory) if directory is not None else None, filters=list(filters))

    def choose_folder(self, *, title: str | None = None, directory: str | Path | None = None,
                      wait: bool = True):
        return self._file_dialog("choose_folder", wait=wait, title=title,
            directory=str(directory) if directory is not None else None)


class Document:
    """A generation-bound document; replacing it makes this facade stale.

    ``command(op, **fields)`` exposes every documented low-level operation.
    Convenience methods below cover common operations without introducing a
    second document model, rendering state, or a component framework.
    """
    MAX_HANDLERS = 256

    def __init__(self, window: Window, document_id: str):
        self.window, self.id = window, document_id
        self._retired = False
        self._handlers: list[tuple[str, str | None, Callable[[dict], Any]]] = []
        self._bitmaps: list[Any] = []
        self._lock = threading.RLock()

    def _check(self) -> None:
        if self._retired or self.window.closed or self.window.document is not self:
            raise StaleDocumentError("This document was replaced, unloaded, or closed")

    def _retire(self) -> None:
        with self._lock:
            self._retired = True
            self._handlers.clear()
            for bitmap in list(self._bitmaps):
                bitmap.close()

    def create_bitmap(self, bitmap_id: str, width: int, height: int, *, slots: int = 2):
        """Create a bounded live image for ``<img src="ui4me-bitmap://ID">``.

        Use the returned object's synchronous ``present`` or nonblocking
        ``try_present``. The host keeps geometry stable across same-sized frames.
        """
        from .live_bitmap import LiveBitmap
        return LiveBitmap(self, bitmap_id, width, height, slots=slots)

    def command(self, op: str, **fields) -> Any:
        self._check()
        if "op" in fields:
            raise ValueError("op is the operation argument")
        return self._call("document.command", {
            "window_id": self.window.id, "document_id": self.id,
            "command": {"op": op, **fields},
        })

    def _call(self, method: str, params: dict) -> Any:
        """A document request; host -32004 retires this document (HostStaleError)."""
        try:
            return self.window.app.rpc.call(method, params)
        except RpcError as error:
            if error.code != CLOSED or isinstance(error, HostStaleError):
                raise
            with self.window.app._lock:
                self._retire()
                if self.window.document is self:
                    self.window.document = None
            raise HostStaleError(error.code, error.message, error.data) from None

    def on(self, event_type: str, callback: Callable[[dict], Any], *,
           element: str | None = None) -> Callable[[], None]:
        """Subscribe to an event, optionally delegated to an identified element.

        These notifications arrive after native dispatch. To prevent a supported
        default action, register ``prevent_default`` in advance. A Python event
        callback cannot retroactively cancel a native edit or activation.
        """
        self._check()
        entry = (event_type, element, callback)
        with self._lock:
            if len(self._handlers) >= self.MAX_HANDLERS:
                raise ValueError("Document callback limit reached")
            self._handlers.append(entry)

        def unsubscribe():
            with self._lock:
                if entry in self._handlers:
                    self._handlers.remove(entry)
        return unsubscribe

    def _dispatch(self, event: dict, *, force: bool = False) -> None:
        # ``force``: deliver a fatal error to the handlers of a document that
        # the transport already marked stale when the error arrived.
        with self._lock:
            if self._retired and not force:
                return
            handlers = list(self._handlers)
        kind, target = event.get("type"), event.get("element")
        route = event.get("path", [])
        for event_type, element, callback in handlers:
            if self._retired and not force:
                break
            if kind == event_type and (element is None or element == target or element in route):
                try:
                    callback(dict(event, window_id=self.window.id, document_id=self.id))
                except BaseException as error:  # One failing handler must not skip the others.
                    self.window.app._report(error, f"'{kind}' event handler")

    def unload(self) -> None:
        self._check()
        self._call("document.unload", {
            "window_id": self.window.id, "document_id": self.id,
        })
        with self.window.app._lock:
            self._retire()
            if self.window.document is self:
                self.window.document = None

    def set_text(self, element: str, text: str) -> None:
        self.command("set_text", element=element, text=text)

    def get_text(self, element: str) -> str:
        return self.command("get_text", element=element)["text"]

    def set_value(self, element: str, value: str) -> None:
        self.command("set_value", element=element, value=value)

    def get_value(self, element: str) -> str:
        return self.command("get_value", element=element)["value"]

    def set_checked(self, element: str, checked: bool) -> None:
        self.command("set_checked", element=element, checked=checked)

    def get_checked(self, element: str) -> bool:
        return self.command("get_checked", element=element)["checked"]

    def set_attribute(self, element: str, name: str, value: str) -> None:
        self.command("set_attribute", element=element, name=name, value=value)

    def remove_attribute(self, element: str, name: str) -> None:
        self.command("remove_attribute", element=element, name=name)

    def add_class(self, element: str, name: str) -> None:
        self.command("add_class", element=element, name=name)

    def remove_class(self, element: str, name: str) -> None:
        self.command("remove_class", element=element, name=name)

    def replace_html(self, element: str, html: str) -> None:
        self.command("replace_children_html", element=element, html=html)

    def insert_html(self, element: str, html: str, *, position: str = "beforeend") -> None:
        self.command("insert_html", element=element, position=position, html=html)

    def remove(self, element: str) -> None:
        self.command("remove_element", element=element)

    def batch(self, operations: Iterable[dict]) -> None:
        # The host validates and commits this as one document transaction.
        self.command("batch", ops=list(operations))

    def query(self, selector: str, *, offset: int = 0, limit: int = 1024) -> dict:
        return self.command("query", selector=selector, offset=offset, limit=limit)

    def focus(self, element: str | Node) -> None:
        self.command("focus", **self._target(element))

    def prevent_default(self, element: str, event: str, *, prevent: bool = True,
                        **key_modifiers) -> None:
        self.command("set_default_action", element=element, event=event,
                     prevent=prevent, **key_modifiers)

    def add_stylesheet(self, css: str) -> dict:
        return self.command("add_stylesheet", css=css)

    def scroll_into_view(self, element: str, *, block: str = "nearest",
                         inline: str = "nearest") -> None:
        self.command("scroll_into_view", element=element, block=block, inline=inline)

    def diagnostics(self) -> dict:
        return self.command("get_diagnostics")

    def _target(self, element: str | Node | None, *, required: bool = True) -> dict:
        self._check()
        if isinstance(element, Node):
            element._check()
            if element.document is not self:
                raise ValueError("Node belongs to another document")
            return {"node": element.id}
        if isinstance(element, str):
            return {"element": element}
        if element is None and not required:
            return {}
        raise TypeError("Expected an element id or a Node from this document")

    def node(self, element: str | None = None, *, reference: str | None = None) -> Node:
        """Resolve an id or wrap a generation-scoped reference; never invent IDs."""
        self._check()
        if reference is not None:
            if element is not None:
                raise ValueError("Provide element or reference, not both")
            return Node(self, reference)
        result = self.command("node_of", **({"element": element} if element is not None else {}))
        return Node(self, result["node"])

    def query_nodes(self, selector: str, *, offset: int = 0, limit: int = 1024) -> dict:
        """Paginated node references, including elements without HTML IDs."""
        return self.command("query_nodes", selector=selector, offset=offset, limit=limit)

    def node_batch(self, operations: Iterable[dict]) -> None:
        self.command("node_batch", ops=list(operations))

    def element_at_point(self, x: float, y: float) -> dict | None:
        return self.command("element_at_point", x=x, y=y)

    def focused_element(self) -> dict | None:
        return self.command("get_focused_element")

    def blur(self, element: str | Node | None = None) -> None:
        self.command("blur", **self._target(element, required=False))

    def control_selection(self, element: str | Node) -> dict:
        """Return control selection offsets in UTF-16 units, not Python indices."""
        return self.command("get_control_selection", **self._target(element))

    def set_control_selection(self, element: str | Node, start: int, end: int,
                              *, direction: str = "none") -> dict:
        return self.command("set_control_selection", **self._target(element),
                            start=start, end=end, direction=direction)

    def replace_control_range(self, element: str | Node, replacement: str, *,
                              start: int | None = None, end: int | None = None,
                              selection_mode: str = "preserve") -> dict:
        values: dict[str, Any] = {**self._target(element), "replacement": replacement,
                                  "selection_mode": selection_mode}
        if start is not None:
            values["start"] = start
        if end is not None:
            values["end"] = end
        return self.command("replace_control_range", **values)

    def edit(self, command: str, element: str | Node | None = None) -> dict:
        """Run an editing command through the native editor's history/IME path.

        Clipboard commands are deferred; observe input/error events for their
        completion. Acceptance does not mean that the OS transfer succeeded.
        """
        return self.command("edit_command", command=command,
                            **self._target(element, required=False))

    def reset_form(self, element: str | Node) -> dict:
        return self.command("reset_form", **self._target(element))

    def _form_target(self, form: str | Node, submitter: str | Node | None) -> dict:
        values = self._target(form)
        if submitter is not None:
            target = self._target(submitter)
            values.update({"submitter_node" if k == "node" else "submitter": v
                           for k, v in target.items()})
        return values

    def request_submit(self, form: str | Node, *, submitter: str | Node | None = None) -> dict:
        return self.command("request_submit", **self._form_target(form, submitter))

    def form_data(self, form: str | Node, *, submitter: str | Node | None = None) -> dict:
        """Ordered entries preserve repeated names and file metadata."""
        return self.command("get_form_data", **self._form_target(form, submitter))

    def set_custom_validity(self, element: str | Node, message: str = "") -> dict:
        return self.command("set_custom_validity", **self._target(element), message=message)

    def validity(self, element: str) -> dict:
        return self.command("get_validity", element=element)

    def check_validity(self, element: str) -> dict:
        return self.command("check_validity", element=element)

    def files(self, element: str) -> list[str]:
        return self.command("get_files", element=element)["files"]

    def options(self, element: str) -> list[dict]:
        return self.command("get_options", element=element)["options"]

    def set_selected_values(self, element: str, values: Iterable[str]) -> None:
        self.command("set_selected_values", element=element, values=list(values))

    def environment(self) -> dict:
        return self.command("get_environment")

    def set_environment(self, *, color_scheme: str | None = None,
                        reduced_motion: str | None = None) -> dict:
        values = {k: v for k, v in (("color_scheme", color_scheme),
                                   ("reduced_motion", reduced_motion)) if v is not None}
        return self.command("set_environment", **values)

    def computed_style(self, properties: Iterable[str], element: str | Node | None = None) -> dict:
        return self.command("get_computed_style", **self._target(element, required=False),
                            properties=list(properties))

    def replace_stylesheet(self, sheet: int, css: str) -> dict:
        return self.command("replace_stylesheet", sheet=sheet, css=css)

    def remove_stylesheet(self, sheet: int) -> None:
        self.command("remove_stylesheet", sheet=sheet)

    def stylesheets(self) -> list[dict]:
        return self.command("list_stylesheets")["sheets"]

    def scroll(self, element: str | None = None) -> dict:
        return self.command("get_scroll", **({"element": element} if element is not None else {}))

    def scroll_to(self, *, x: float | None = None, y: float | None = None,
                  element: str | None = None, behavior: str = "auto") -> dict:
        values = {k: v for k, v in (("x", x), ("y", y), ("element", element)) if v is not None}
        return self.command("scroll_to", behavior=behavior, **values)

    def show_dialog(self, element: str, *, modal: bool = True) -> None:
        self.command("show_dialog", element=element, modal=modal)

    def close_dialog(self, element: str, return_value: str = "") -> None:
        self.command("close_dialog", element=element, return_value=return_value)

    def show_popover(self, element: str) -> None:
        self.command("show_popover", element=element)

    def hide_popover(self, element: str) -> None:
        self.command("hide_popover", element=element)

    def toggle_popover(self, element: str) -> bool:
        return self.command("toggle_popover", element=element)["open"]

    def register_resources(self, name: str, entries: dict[str, bytes | tuple[bytes, str]]) -> dict:
        """Atomically replace a named ui4me-res:// bundle for this document.

        Each mapping value is bytes or (bytes, MIME type). Registration is at
        most 256 entries / 1 MiB decoded; all registered bundles share a 16 MiB
        source limit. Existing live resources survive a rejected registration.
        """
        self._check()
        if len(entries) > 256:
            raise ValueError("Resource registration permits at most 256 entries")
        encoded, size = [], 0
        for path, value in entries.items():
            data, mime = value if isinstance(value, tuple) else (value, "application/octet-stream")
            if not isinstance(data, bytes):
                raise TypeError("Resource values must contain bytes")
            if not isinstance(mime, str) or any(c in mime for c in ",\r\n"):
                raise ValueError("Invalid resource MIME type")
            size += len(data)
            if size > 1024 * 1024:
                raise ValueError("Resource registration permits at most 1 MiB decoded bytes")
            encoded.append({"path": path, "data_url": "data:" + mime + ";base64," +
                            base64.b64encode(data).decode("ascii")})
        return self.command("register_resource_bundle", name=name, entries=encoded)

    def unregister_resources(self, name: str) -> dict:
        return self.command("unregister_resource_bundle", name=name)

    def resource_stats(self) -> dict:
        return self.command("get_resource_stats")

    def purge_unused_resources(self, url: str | None = None) -> dict:
        return self.command("purge_unused_resources", **({"url": url} if url is not None else {}))


class Node:
    """A document-generation-bound opaque node reference, including anonymous nodes."""
    def __init__(self, document: Document, reference: str):
        if not isinstance(reference, str) or not reference:
            raise ValueError("A node reference must be a nonempty string")
        self.document, self.id = document, reference

    def _check(self) -> None:
        self.document._check()

    def inspect(self) -> dict:
        self._check()
        return self.document.command("get_node", node=self.id)

    def children(self, *, offset: int = 0, limit: int = 256) -> dict:
        self._check()
        return self.document.command("node_children", node=self.id, offset=offset, limit=limit)

    def _mutate(self, op: str, **fields) -> None:
        self._check()
        self.document.node_batch([{"op": op, "node": self.id, **fields}])

    def set_text(self, text: str) -> None:
        self._mutate("set_text", text=text)

    def set_attribute(self, name: str, value: str) -> None:
        self._mutate("set_attribute", name=name, value=value)

    def remove_attribute(self, name: str) -> None:
        self._mutate("remove_attribute", name=name)

    def add_class(self, name: str) -> None:
        self._mutate("add_class", name=name)

    def remove_class(self, name: str) -> None:
        self._mutate("remove_class", name=name)

    def insert_html(self, html: str, *, position: str = "beforeend") -> None:
        self._mutate("insert_html", position=position, html=html)

    def replace_html(self, html: str) -> None:
        self._mutate("replace_children_html", html=html)

    def append_text(self, text: str) -> None:
        self._mutate("append_text", text=text)

    def trim_children(self, keep: int, *, from_start: bool = True) -> None:
        self._mutate("trim_children", keep=keep, **{"from": "start" if from_start else "end"})

    def move_to(self, target: Node, *, position: str = "beforeend") -> None:
        self._check()
        target._check()
        if target.document is not self.document:
            raise ValueError("Cannot move nodes between documents")
        self._mutate("move_element", target_node=target.id, position=position)

    def remove(self) -> None:
        self._mutate("remove_element")

    def focus(self) -> None:
        self.document.focus(self)

    def blur(self) -> None:
        self.document.blur(self)
