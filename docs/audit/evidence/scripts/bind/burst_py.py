"""Issue #1 threshold probe (Python). N transitioned buttons (3 props), ROUNDS colour-scheme flips.
SUB=1 subscribes trivial transition callbacks; SUB=0 subscribes nothing (host sends events anyway).
Reader instrumentation: per document.event frame arrival time on the reader thread."""
import os, sys, time, threading
import ui4me_core.client as C
arrivals = []
_orig = C.read_frame
def timed_read_frame(stream):
    m = _orig(stream)
    if m is not None and m.get("method") == "document.event":
        arrivals.append(time.perf_counter())
    return m
C.read_frame = timed_read_frame
from ui4me_core import Application
N = int(sys.argv[1]); ROUNDS = int(sys.argv[2]); SUB = os.environ.get("SUB", "1") == "1"
css = """button { transition: background-color 120ms, border-color 120ms, color 120ms; background:#eee; border:1px solid #999; color:#111 }
@media (prefers-color-scheme: dark) { button { background:#333; border-color:#777; color:#eee } }"""
html = "<style>%s</style><div id=root>%s</div><p id=s>x</p>" % (css, "".join(f'<button id="b{i}">B{i}</button>' for i in range(N)))
def stats():
    if not arrivals: return "frames=0"
    gaps = [b - a for a, b in zip(arrivals, arrivals[1:])] or [0]
    t0 = arrivals[0]
    c1 = [t for t in arrivals if t - t0 < 0.05]
    rate = (len(c1) - 1) / (c1[-1] - t0) if len(c1) > 1 and c1[-1] > t0 else float("nan")
    return (f"frames={len(arrivals)} span={1000*(arrivals[-1]-arrivals[0]):.1f}ms max_gap={1000*max(gaps):.2f}ms "
            f"cluster1={len(c1)} in {1000*(c1[-1]-t0):.2f}ms => reader {rate:,.0f} frames/s")
with Application.start(name="Burst BIND", app_id="org.ui4me.audit.BindBurst", instance="multiple") as app:
    w = app.open_html(html, title="burst", width=800, height=600)
    d = w.document
    cnt = {"n": 0}
    if SUB:
        for t in ("transitionrun", "transitionstart", "transitionend"):
            d.on(t, lambda e: cnt.__setitem__("n", cnt["n"] + 1))
    def body():
        try:
            time.sleep(1.0)
            for r in range(ROUNDS):
                arrivals.clear()
                d.set_environment(color_scheme="dark" if r % 2 == 0 else "light")
                time.sleep(0.5)
                d.get_text("s")
                print(f"ROUND_OK {r} callbacks={cnt['n']} dropped={app.rpc.dropped_notifications} {stats()}", flush=True)
            print("PROBE_PASS", flush=True)
        except Exception as e:
            print(f"PROBE_FAIL {type(e).__name__} {e} | last round reader {stats()} dropped={app.rpc.dropped_notifications}", flush=True)
        finally:
            try: app.request_quit()
            except Exception: pass
    threading.Thread(target=body, daemon=True).start()
    try:
        app.run()
    except Exception as e:
        print("RUN_EXC", type(e).__name__, e, flush=True)
