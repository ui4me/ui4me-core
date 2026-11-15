import sys, time
from ui4me_core import Application
with Application.start(name="e12") as app:
    app.open_html("<p>x</p>", width=200, height=100)
    app.on_shutdown(lambda: print("on_shutdown ran", flush=True))
    app.run()
