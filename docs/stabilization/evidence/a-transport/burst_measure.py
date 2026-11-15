"""Run docs/audit/evidence/scripts/burst2.py unchanged, counting the frames and
bytes the backend's reader receives (backend role only).
Usage: python3 burst_measure.py BURST2_PATH N ROUNDS [mode]"""
import atexit, os, runpy, sys, time
import ui4me_core.client as client

script = sys.argv.pop(1)
stats = {"frames": 0, "bytes": 0, "events": 0, "first": None, "last": None, "clients": []}
_read_exact = client.read_exact
def read_exact(stream, length, *, clean_eof=False):
    data = _read_exact(stream, length, clean_eof=clean_eof)
    if data is not None:
        stats["bytes"] += len(data)
        if length == 4:
            stats["frames"] += 1
            now = time.monotonic()
            stats["first"] = stats["first"] or now
            stats["last"] = now
    return data
client.read_exact = read_exact
_init = client.RpcClient.__init__
def init(self, *a, **k):
    _init(self, *a, **k)
    stats["clients"].append(self)
client.RpcClient.__init__ = init

@atexit.register
def report():
    if stats["frames"]:
        dropped = sum(getattr(c, "dropped_notifications", 0) for c in stats["clients"])
        print(f"MEASURE frames={stats['frames']} bytes={stats['bytes']} "
              f"client_dropped_notifications={dropped}", flush=True)

runpy.run_path(script, run_name="__main__")
