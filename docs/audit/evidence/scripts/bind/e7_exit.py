import sys, time, os, threading
from ui4me_core import Application
mode = sys.argv[1]
app = Application.start(name="e7")
if mode == "nowindow":
    app.run()
win = app.open_html("<p>x</p>", width=200, height=100)
if mode == "fall_off":
    app.ready()
    sys.exit(0)
if mode == "exit3":
    threading.Thread(target=lambda: (time.sleep(0.2), app.request_quit()), daemon=True).start()
    app.run()
    sys.exit(3)
if mode == "hang":
    app.ready()
    print("host pid", os.getppid(), flush=True)
    time.sleep(60)
