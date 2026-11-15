"""A request handler whose result cannot be encoded: is a response sent, does the worker survive?"""
import socket, sys, threading, time
sys.path.insert(0, "<repo>/python")
from ui4me_core.client import RpcClient, SocketStream, encode_frame, read_frame
a, b = socket.socketpair()
client = RpcClient(SocketStream(a), workers=2)
client.on_request("test.nan", lambda p: {"value": float("nan")})
client.on_request("test.obj", lambda p: {"value": object()})
client.on_request("test.ok", lambda p: {"ok": True})
b.settimeout(2)
def ask(method, i):
    b.sendall(encode_frame({"jsonrpc": "2.0", "id": f"h{i}", "method": method, "params": {}}))
    try:
        print(method, "->", read_frame(SocketStream(b)), flush=True)
    except socket.timeout:
        print(method, "-> NO RESPONSE within 2 s", flush=True)
time.sleep(0.1)
ask("test.nan", 1); ask("test.obj", 2)
print("live callback workers:", [t.name for t in threading.enumerate() if t.name.startswith("ui4me-callback")], flush=True)
ask("test.ok", 3)
