import sys, time, threading
from ui4me_core import Application
app = Application.start(name="Notes")
app.on_window_event(lambda p: None)
app.rpc.on_notification("app.activated", lambda p: print("primary got app.activated", p.get("args"), flush=True))
app.open_html("<p>x</p>", width=200, height=100)
threading.Timer(6, app.request_quit).start()
print("primary running", flush=True)
app.run()
