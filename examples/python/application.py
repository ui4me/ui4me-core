"""Small reference API for HTML/CSS applications over the private host protocol.

This module is supplied source, not a separately published SDK. The Rust host
owns native windows, input, editing and rendering. Python owns application logic.
Application notifications are serialized on one event worker; independent
transport and lifecycle-request workers continue resolving RPC and close/quit
policy, so a notification callback can issue document commands or wait for a
native dialog without blocking lifecycle control.
"""
from __future__ import annotations

from collections.abc import Callable, Iterable
from pathlib import Path
import sys
import threading
from typing import Any

from client import DisconnectedError, RpcClient


class StaleDocumentError(RuntimeError):
    """The document was replaced, unloaded, or its window was closed."""


class Application:
    """Connect to the host that launched this backend, then create documents.

    Use ``Application.from_environment()`` from a managed backend. ``run()``
    announces readiness and waits for native shutdown. No native event loop runs
    in this process. The exposed ``rpc`` remains available for desktop services.
    """

    def __init__(self, rpc: RpcClient):
        self.rpc = rpc
        self._lock = threading.RLock()
        self._windows: dict[str, Window] = {}
        self._ready = False
        self.rpc.on_notification("document.event", self._document_event)
        self.rpc.on_notification("document.error", self._document_error)
        self.rpc.on_notification("window.event", self._window_event)

    @classmethod
    def from_environment(cls) -> Application:
        # Document/window events are ordered, while the existing four-worker
        # host request pool remains able to answer close/quit independently.
        return cls(RpcClient.from_environment(ordered_notifications=True))

    def create_window(self, *, title: str = "UI4ME", width: int = 900,
                      height: int = 600, visible: bool = True) -> Window:
        state = self.rpc.call("window.create", {
            "title": title, "width": width, "height": height, "visible": visible,
        })
        window = Window(self, state["window_id"])
        with self._lock:
            self._windows[window.id] = window
        return window

    def open_file(self, path: str | Path, **window_options) -> Window:
        window = self.create_window(**window_options)
        try:
            window.load_file(path)
        except BaseException:
            window.request_close()
            raise
        return window

    def open_html(self, html: str, *, asset_root: str | Path | None = None,
                  **window_options) -> Window:
        window = self.create_window(**window_options)
        try:
            window.load_html(html, asset_root=asset_root)
        except BaseException:
            window.request_close()
            raise
        return window

    def ready(self) -> None:
        if not self._ready:
            self.rpc.call("app.ready")
            self._ready = True

    def run(self) -> None:
        self.ready()
        while not self.rpc.shutdown_requested.wait(0.1):
            if self.rpc.closed:
                raise DisconnectedError("The host disconnected before orderly shutdown")

    def request_quit(self) -> None:
        self.rpc.call("app.request_quit")

    def on_before_quit(self, callback: Callable[[], bool]) -> None:
        self.rpc.on_request("app.before_quit", lambda _: {"allow": bool(callback())})

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
        if params.get("type") != "closed":
            return
        with self._lock:
            window = self._windows.pop(params.get("window_id"), None)
            if window:
                window.closed = True
                if window.document:
                    window.document._retire()

    def _document_error(self, params: dict) -> None:
        with self._lock:
            window = self._windows.get(params.get("window_id"))
            document = window.document if window and not window.closed else None
            if document is None or document.id != params.get("document_id"):
                return
        message = str(params.get("message", "Document operation failed"))[:4096]
        fatal = bool(params.get("fatal", False))
        print(f"UI4ME document error ({window.id}, {document.id}): {message}", file=sys.stderr, flush=True)
        try:
            document._dispatch({"type": "error", "message": message, "fatal": fatal})
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
            for window in self._windows.values():
                window.closed = True
                if window.document:
                    window.document._retire()
            self._windows.clear()
        self.rpc.close()

    def __enter__(self) -> Application:
        return self

    def __exit__(self, *_):
        self.close()


class Window:
    def __init__(self, app: Application, window_id: str):
        self.app, self.id = app, window_id
        self.document: Document | None = None
        self.closed = False

    def _load(self, **source) -> Document:
        if self.closed:
            raise StaleDocumentError("Window is closed")
        result = self.app.rpc.call("document.load", {"window_id": self.id, **source}, timeout=30)
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
        return self.app.rpc.call("window.set", {"window_id": self.id, **properties})

    def state(self) -> dict:
        return self.app.rpc.call("window.get", {"window_id": self.id})

    def request_close(self) -> None:
        self.app.rpc.call("window.request_close", {"window_id": self.id})


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
        self._lock = threading.RLock()

    def _check(self) -> None:
        if self._retired or self.window.closed or self.window.document is not self:
            raise StaleDocumentError("This document was replaced, unloaded, or closed")

    def _retire(self) -> None:
        with self._lock:
            self._retired = True
            self._handlers.clear()

    def command(self, op: str, **fields) -> Any:
        self._check()
        if "op" in fields:
            raise ValueError("op is the operation argument")
        return self.window.app.rpc.call("document.command", {
            "window_id": self.window.id, "document_id": self.id,
            "command": {"op": op, **fields},
        })

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

    def _dispatch(self, event: dict) -> None:
        with self._lock:
            if self._retired:
                return
            handlers = list(self._handlers)
        kind, target = event.get("type"), event.get("element")
        route = event.get("path", [])
        for event_type, element, callback in handlers:
            if self._retired:
                break
            if kind == event_type and (element is None or element == target or element in route):
                callback(dict(event, window_id=self.window.id, document_id=self.id))

    def unload(self) -> None:
        self._check()
        self.window.app.rpc.call("document.unload", {
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

    def focus(self, element: str) -> None:
        self.command("focus", element=element)

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
