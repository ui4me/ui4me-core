"""UI4ME Core: native HTML/CSS desktop windows controlled from Python.

Start an application from a normal script with ``Application.start()``; the
packaged Rust host owns the windows, rendering and event loop.
"""
from .application import Application, Window, Document, Node, StaleDocumentError
from .client import RpcClient, RpcError, ProtocolError, DisconnectedError
from ._bootstrap import StartupError
from .live_bitmap import LiveBitmap
from .launcher import host_binary

__version__ = "1.0.1"
__all__ = ["Application", "Window", "Document", "Node", "StaleDocumentError", "StartupError",
           "RpcClient", "RpcError", "ProtocolError", "DisconnectedError", "LiveBitmap", "host_binary"]
