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
import base64
from pathlib import Path
import sys
import threading
from typing import Any

from .client import DisconnectedError, RpcClient


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

    def _before_close(self, params: dict) -> dict:
        with self._lock:
            window = self._windows.get(params.get("window_id"))
            callback = getattr(window, "_before_close", None)
        return {"allow": True if callback is None else bool(callback())}

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

    def _check(self) -> None:
        if self.closed:
            raise StaleDocumentError("Window is closed")

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
        self._check()
        return self.app.rpc.call("window.set", {"window_id": self.id, **properties})

    def state(self) -> dict:
        self._check()
        return self.app.rpc.call("window.get", {"window_id": self.id})

    def request_attention(self, kind: str | None = "informational") -> dict:
        self._check()
        return self.app.rpc.call("window.request_attention", {"window_id": self.id, "kind": kind})

    def on_before_close(self, callback: Callable[[], bool]) -> None:
        self._check()
        # A request callback is installed once per Application; route by window
        # rather than replacing another window's policy when this is called.
        self._before_close = callback
        self.app.rpc.on_request("window.before_close", self.app._before_close)

    def request_close(self) -> None:
        self._check()
        self.app.rpc.call("window.request_close", {"window_id": self.id})

    def _file_dialog(self, kind: str, *, wait: bool, **options):
        self._check()
        params = {"window_id": self.id, **{k: v for k, v in options.items() if v is not None}}
        # Returning the transport future preserves its cancellation semantics:
        # future.cancel() asks the host to close the actual native dialog.
        if wait:
            return self.app.rpc.call("dialog." + kind, params, timeout=None)
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
