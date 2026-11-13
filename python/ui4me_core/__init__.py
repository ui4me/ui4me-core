"""UI4ME Core — Python client for the native HTML/CSS desktop host."""
from .application import Application, Window, Document, StaleDocumentError
from .client import RpcClient, RpcError, ProtocolError, DisconnectedError
from .live_bitmap import LiveBitmap
from .launcher import host_binary

__version__ = "1.0.0"
__all__ = ["Application", "Window", "Document", "StaleDocumentError", "RpcClient", "RpcError", "ProtocolError", "DisconnectedError", "LiveBitmap", "host_binary"]
