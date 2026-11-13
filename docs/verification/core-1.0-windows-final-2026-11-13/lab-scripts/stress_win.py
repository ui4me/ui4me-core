# executed inside the lab backend via ctl: defines bitmap stress helpers
import time as _t
def frames_for(w, h, n=4):
    out = []
    for k in range(n):
        row = bytes(((x * 255 // max(w-1,1) + k*60) & 255, (k*80) & 255, 255 - (x*255//max(w-1,1)), 255)[c] for x in range(w) for c in range(4))
        out.append(row * h)
    return out

def stress(bid, w, h, count, fps=0, slots=2, mode="present"):
    for b in list(doc._bitmaps):
        if b.id == bid: b.close()
    bm = doc.create_bitmap(bid, w, h, slots=slots)
    fr = frames_for(w, h)
    lat, applied, dropped = [], 0, 0
    t0 = _t.perf_counter()
    for i in range(count):
        s = _t.perf_counter()
        r = bm.present(fr[i % len(fr)]) if mode == "present" else bm.try_present(fr[i % len(fr)])
        lat.append(_t.perf_counter() - s)
        if r is None: dropped += 1
        elif r.get("presented_to_engine"): applied += 1
        if fps: 
            nxt = t0 + (i + 1) / fps
            d = nxt - _t.perf_counter()
            if d > 0: _t.sleep(d)
    total = _t.perf_counter() - t0
    lat.sort()
    q = lambda p: round(lat[min(len(lat)-1, int(p*len(lat)))]*1000, 3)
    return {"bitmap": bid, "size": [w, h], "frames": count, "applied": applied, "dropped": dropped,
            "seconds": round(total, 3), "fps": round(count/total, 1),
            "ack_ms": {"p50": q(.5), "p95": q(.95), "p99": q(.99), "max": round(lat[-1]*1000, 3)},
            "last_sequence": bm._sequence}
state["stress"]=stress; state["frames_for"]=frames_for

import zlib as _z, struct as _s
def png(w, h, rgba):
    raw = b"".join(b"\0" + bytes(rgba) * w for _ in range(h))
    def chunk(t, d): return _s.pack(">I", len(d)) + t + d + _s.pack(">I", _z.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", _s.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", _z.compress(raw)) + chunk(b"IEND", b"")
state["png"] = png
def bundle_loop(n, w=200, h=100, delay=0.02, start=0):
    cols = [(255,0,0,255),(0,0,255,255),(0,160,0,255),(255,200,0,255)]
    imgs = [png(w, h, c) for c in cols]
    t0 = _t.perf_counter(); lat = []
    for i in range(start, start + n):
        s = _t.perf_counter()
        doc.register_resources("lab", {"pic.png": (imgs[i % 4], "image/png")})
        lat.append(_t.perf_counter() - s)
        _t.sleep(delay)
    lat.sort()
    return {"n": n, "seconds": round(_t.perf_counter()-t0, 2), "register_ms_p50": round(lat[len(lat)//2]*1000, 2), "register_ms_max": round(lat[-1]*1000, 2)}
state["bundle_loop"] = bundle_loop
