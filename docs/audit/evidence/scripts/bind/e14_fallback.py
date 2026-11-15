"""A failed open_html closes its window; does that cancel startup before a fallback can be shown?"""
import threading
from ui4me_core import Application, RpcError
app = Application.start(name="e14")
try:
    app.open_html("<p onclick='x'>active content is rejected</p>")
except RpcError as error:
    print("first load failed as expected:", error, flush=True)
    try:
        app.open_html("<p>fallback page</p>")
        print("fallback window opened", flush=True)
    except Exception as e2:
        print("fallback failed:", type(e2).__name__, e2, flush=True)
threading.Timer(0.5, app.request_quit).start()
try:
    app.run()
    print("run returned normally", flush=True)
except Exception as e:
    print("run failed:", type(e).__name__, e, flush=True)
