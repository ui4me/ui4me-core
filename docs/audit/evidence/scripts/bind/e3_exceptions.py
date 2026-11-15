"""Exceptions in event/policy callbacks: what does the developer see? Also None-returning policy."""
import sys, time, threading
from ui4me_core import Application
app = Application.start(name="e3")
win = app.open_html("<input id=a><input id=b>", width=300, height=200)
doc = win.document
def helper(x):
    return x["missing_key"]
def h1(e):
    helper(e)
def h2(e):
    print("second handler for same event ran", flush=True)
doc.on("focus", h1)
doc.on("focus", h2)
def bad_policy():
    raise ValueError("policy bug")
win.on_before_close(bad_policy)
app.ready()
doc.focus("a")
time.sleep(0.5)
print("--- now request_close with raising policy", flush=True)
win.request_close()
time.sleep(0.5)
print("window closed?", win.closed, flush=True)
def none_policy():
    pass   # forgot to return True
win.on_before_close(none_policy)
win.request_close()
time.sleep(0.5)
print("window closed after None-returning policy?", win.closed, flush=True)
win.on_before_close(lambda: True)
win.request_close()
t0 = time.time()
app.run()
print("run returned", round(time.time() - t0, 2), flush=True)
