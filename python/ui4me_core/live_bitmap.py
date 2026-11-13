"""Document-local, bounded raw RGBA8 frames over the existing private IPC.

This is not an image encoder or a new network protocol. The host allocates a
private file in the session's runtime directory and the producer maps it for
writes. A synchronous present acknowledges ownership of the slot; no pending
frame queue is built in Python. Image style/layout is preserved across frames.
"""
from __future__ import annotations

import mmap
from pathlib import Path
import threading
from typing import TYPE_CHECKING

from .client import DisconnectedError, RpcError

CLOSED = -32004
INVALID_STATE = -32008

if TYPE_CHECKING:
    from .application import Document


class LiveBitmap:
    def __init__(self, document: "Document", bitmap_id: str, width: int, height: int, *, slots: int = 2):
        document._check()
        if not (1 <= len(bitmap_id) <= 64) or not all(c.isascii() and (c.isalnum() or c in "_-") for c in bitmap_id):
            raise ValueError("bitmap_id must contain 1..64 ASCII letters/digits/_/-")
        if not (1 <= width <= 4096 and 1 <= height <= 4096 and slots in (2, 3)):
            raise ValueError("invalid bitmap geometry or slot count")
        self._document = document
        self.id = bitmap_id
        self.width = width
        self.height = height
        self._lock = threading.Lock()
        self._closed = False
        self._sequence = 0
        response = document.window.app.rpc.call("bitmap.create", {
            "window_id": document.window.id, "document_id": document.id, "bitmap_id": bitmap_id,
            "width": width, "height": height, "slots": slots, "pixel_format": "RGBA8"
        })
        self.stride = int(response["stride"])
        self.slots = int(response["slots"])
        self.slot_bytes = int(response["slot_bytes"])
        if (self.stride, self.slot_bytes) != (width * 4, width * height * 4):
            raise RuntimeError("Host returned invalid bitmap geometry")
        path = Path(response["path"])
        try:
            self._file = path.open("r+b", buffering=0)
            try:
                self._map = mmap.mmap(self._file.fileno(), self.slot_bytes * self.slots,
                                      access=mmap.ACCESS_WRITE)
            except BaseException:
                self._file.close()
                raise
        except BaseException:
            # A successful bitmap.create owns native resources even if the
            # subsequent Python file mapping fails. Release it explicitly.
            try:
                document.window.app.rpc.call("bitmap.close", {
                    "window_id": document.window.id, "document_id": document.id,
                    "bitmap_id": bitmap_id,
                })
            except Exception:
                pass
            raise
        document._bitmaps.append(self)

    def present(self, pixels: bytes | bytearray | memoryview) -> dict:
        """Write one complete frame; return after the host snapshots that slot.

        Frame data is straight-alpha RGBA8 in row-major order with a tight
        stride. The host premultiplies before raster composition. A producer
        must not modify the supplied buffer concurrently with this call.
        """
        view = memoryview(pixels).cast("B")
        if len(view) != self.slot_bytes:
            raise ValueError(f"Expected {self.slot_bytes} RGBA bytes, received {len(view)}")
        with self._lock:
            if self._closed:
                raise RuntimeError("Bitmap is closed")
            self._document._check()
            self._sequence += 1
            slot = self._sequence % self.slots
            start = slot * self.slot_bytes
            self._map[start:start + self.slot_bytes] = view
            return self._document.window.app.rpc.call("bitmap.present", {
                "window_id": self._document.window.id,
                "document_id": self._document.id,
                "bitmap_id": self.id,
                "sequence": self._sequence,
                "slot": slot,
            })

    def try_present(self, pixels: bytes | bytearray | memoryview) -> dict | None:
        """Nonblocking admission: return None if a previous present is in progress."""
        if not self._lock.acquire(blocking=False):
            return None
        try:
            # Inline the locked portion instead of nesting a nonreentrant lock.
            view = memoryview(pixels).cast("B")
            if len(view) != self.slot_bytes:
                raise ValueError(f"Expected {self.slot_bytes} RGBA bytes")
            if self._closed:
                raise RuntimeError("Bitmap is closed")
            self._document._check()
            self._sequence += 1
            slot = self._sequence % self.slots
            start = slot * self.slot_bytes
            self._map[start:start + self.slot_bytes] = view
            return self._document.window.app.rpc.call("bitmap.present", {
                "window_id": self._document.window.id, "document_id": self._document.id,
                "bitmap_id": self.id, "sequence": self._sequence, "slot": slot,
            })
        finally:
            self._lock.release()

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
            self._map.close()
            self._file.close()
            try:
                self._document._bitmaps.remove(self)
            except ValueError:
                pass
            # An already-unloaded document has automatically released its host
            # resources. Do not send a stale close request in that case.
            if not self._document._retired and not self._document.window.closed:
                try:
                    self._document.window.app.rpc.call("bitmap.close", {
                        "window_id": self._document.window.id, "document_id": self._document.id,
                        "bitmap_id": self.id,
                    })
                except DisconnectedError:
                    pass  # The host is gone; its private file went with it.
                except RpcError as error:
                    # The window closed or the host is shutting down: closing a
                    # window or document releases its bitmaps in the host.
                    if error.code not in (CLOSED, INVALID_STATE):
                        raise

    def __enter__(self) -> "LiveBitmap":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()
