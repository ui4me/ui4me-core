"""Same drain test with a buffered reader (one recv per 64 KiB instead of two per frame)."""
import socket, sys, threading, time
sys.path.insert(0, "<repo>/python")
from ui4me_core.client import RpcClient, SocketStream, encode_frame
exec(open("e9_drain.py").read().split("def run")[0].split("from ui4me_core.client")[1].split("\n",1)[1])
class BufferedSocketStream(SocketStream):
    def __init__(self, sock):
        super().__init__(sock)
        self._r = sock.makefile("rb", buffering=65536)
    def read(self, size):
        return self._r.read1(size)
def run(n, hog, buffered):
    a, b = socket.socketpair()
    handled = [0]
    client = RpcClient((BufferedSocketStream if buffered else SocketStream)(a), ordered_notifications=True)
    client.on_notification("document.event", lambda p: handled.__setitem__(0, handled[0] + 1))
    stop = threading.Event()
    if hog:
        threading.Thread(target=lambda: [None for _ in iter(lambda: stop.is_set(), True)], daemon=True).start()
    frame = encode_frame(EVENT)
    t0 = time.perf_counter()
    threading.Thread(target=b.sendall, args=(frame * n,), daemon=True).start()
    while client._ordered_notifications.qsize() + handled[0] + client.dropped_notifications < n:
        time.sleep(0.001)
    dt = time.perf_counter() - t0
    stop.set()
    print(f"buffered={buffered} hog={hog}: {n / dt:,.0f} frames/s, dropped={client.dropped_notifications}", flush=True)
    client.close(); b.close()
for buffered in (False, True):
    run(20000, True, buffered)
