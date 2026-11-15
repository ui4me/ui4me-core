"""How fast does the Python reader drain host notifications? (socketpair mock host)"""
import json, socket, struct, sys, threading, time
sys.path.insert(0, "<repo>/python")
from ui4me_core.client import RpcClient, SocketStream, encode_frame

EVENT = {"jsonrpc": "2.0", "method": "document.event", "params": {"window_id": "w1", "document_id": "d1",
         "event": {"type": "pointermove", "element": "box", "path": ["box", "main", "body"], "x": 101.5, "y": 77.25,
                   "client_x": 101.5, "client_y": 77.25, "button": 0, "buttons": 0, "pointer_id": 1,
                   "pointer_type": "mouse", "shift": False, "ctrl": False, "alt": False, "meta": False,
                   "timestamp": 123456.789}}}
def run(n, hog, slow_cb):
    a, b = socket.socketpair()
    handled = [0]
    client = RpcClient(SocketStream(a), ordered_notifications=True)
    def cb(p):
        handled[0] += 1
        if slow_cb: time.sleep(slow_cb)
    client.on_notification("document.event", cb)
    stop = threading.Event()
    if hog:
        def spin():
            x = 0
            while not stop.is_set():
                x += 1
        threading.Thread(target=spin, daemon=True).start()
    frame = encode_frame(EVENT)
    data = frame * n
    t0 = time.perf_counter()
    b.sendall(data)
    # Wait until reader consumed everything (delivered or dropped)
    while handled[0] + client.dropped_notifications < n and not client._ordered_notifications.qsize() + handled[0] + client.dropped_notifications >= n:
        time.sleep(0.001)
    t_read = time.perf_counter() - t0
    stop.set()
    print(f"n={n} hog={hog} slow_cb={slow_cb}: reader consumed {n} frames ({len(frame)} B each) in {t_read:.3f}s "
          f"=> {n / t_read:,.0f} frames/s; dropped={client.dropped_notifications} handled_so_far={handled[0]}", flush=True)
    client.close(); b.close()
run(20000, False, 0)
run(20000, True, 0)
run(5000, False, 0.01)
