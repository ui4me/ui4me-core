"""Issue #1 probe: N controls with colour transitions, then toggle the colour scheme.
Usage: python burst.py N ROUNDS [mode]   mode = env (set_environment) | class (toggle class on root)"""
import sys, time, threading
from ui4me_core import Application
N = int(sys.argv[1]); ROUNDS = int(sys.argv[2]); MODE = sys.argv[3] if len(sys.argv) > 3 else "env"
css = """
button { transition: background-color 120ms, border-color 120ms, color 120ms; background:#eee; border:1px solid #999; color:#111 }
@media (prefers-color-scheme: dark) { button { background:#333; border-color:#777; color:#eee } }
.dark button { background:#333; border-color:#777; color:#eee }
"""
html = "<style>%s</style><div id=root>%s</div><p id=s>x</p>" % (css, "".join(f'<button id="b{i}">B{i}</button>' for i in range(N)))
def main():
    with Application.start(name="Burst probe", app_id="org.ui4me.audit.Burst", instance="multiple") as app:
        w = app.open_html(html, title="burst", width=800, height=600)
        d = w.document
        cnt = {"n":0}
        import os
        SLOW=float(os.environ.get("SLOW","0"))
        def onev(e):
            cnt["n"]+=1
            if SLOW: time.sleep(SLOW)
        for t in ("transitionrun","transitionstart","transitionend"): d.on(t, onev)
        def body():
            try:
                time.sleep(1.0)
                for r in range(ROUNDS):
                    if MODE == "env":
                        d.set_environment(color_scheme="dark" if r % 2 == 0 else "light")
                    else:
                        (d.add_class if r % 2 == 0 else d.remove_class)("root", "dark")
                    time.sleep(0.4)
                    d.get_text("s")  # round trip proves the connection still lives
                    print("ROUND_OK", r, "events", cnt["n"], flush=True)
                print("PROBE_PASS", flush=True)
            except Exception as e:
                print("PROBE_FAIL", type(e).__name__, e, flush=True)
            finally:
                try: app.request_quit()
                except Exception: pass
        threading.Thread(target=body, daemon=True).start()
        app.run()
if __name__ == "__main__":
    main()
