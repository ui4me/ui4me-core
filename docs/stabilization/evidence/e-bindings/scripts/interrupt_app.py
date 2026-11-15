import sys
from ui4me_core import Application
app = Application.start(name="sig", instance="multiple")
app.open_html("<p>x</p>")
app.on_shutdown(lambda: print("SHUTDOWN-HOOK", flush=True))
app.ready()
print("READY", flush=True)
app.run()
print("RUN-RETURNED", flush=True)
