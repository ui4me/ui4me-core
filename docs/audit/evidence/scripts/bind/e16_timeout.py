import socket, sys
sys.path.insert(0, "<repo>/python")
from ui4me_core.client import RpcClient, SocketStream
a, b = socket.socketpair()
client = RpcClient(SocketStream(a))
try:
    client.call("document.command", {}, timeout=0.2)
except TimeoutError:
    print(sys.version.split()[0], "caught by `except TimeoutError` (as documented)")
except Exception as e:
    print(sys.version.split()[0], "NOT a TimeoutError:", type(e).__module__ + "." + type(e).__qualname__, "mro:", [c.__name__ for c in type(e).__mro__])
