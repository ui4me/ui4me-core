import os, sys, time, threading
from ui4me_core import Application
def rss(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("VmRSS:"): return int(l.split()[1])
CYC = int(sys.argv[1])
def main():
    with Application.start(name="Churn", app_id="org.ui4me.audit.Churn", instance="multiple") as app:
        base = app.open_html("<p id=s>base</p>", title="base", width=300, height=200)
        def body():
            host = os.getppid(); errs = 0; t0 = time.monotonic()
            try:
                print("START", rss(host), flush=True)
                for i in range(CYC):
                    w = app.open_html(f"<h1>w{i}</h1><input id=i value='{i}'><b>bold</b> <code>mono</code>", title=f"w{i}", width=320, height=200)
                    d = w.document
                    assert d.get_value("i") == str(i)
                    for k in range(3):
                        d = w.load_html(f"<p id=p>gen {k}</p><table><tr><td>{i}</td></tr></table>")
                    assert d.get_text("p") == "gen 2"
                    w.request_close()
                    if i % 50 == 49: print("CYCLE", i+1, rss(host), round(time.monotonic()-t0,1), flush=True)
                time.sleep(2); print("END", rss(host), flush=True); print("CHURN_PASS", flush=True)
            except Exception as e:
                print("CHURN_FAIL", i, type(e).__name__, e, flush=True)
            finally:
                app.request_quit()
        threading.Thread(target=body, daemon=True).start()
        app.run()
if __name__ == "__main__": main()
