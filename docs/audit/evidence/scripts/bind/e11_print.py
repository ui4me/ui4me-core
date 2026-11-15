import sys, time, threading
from ui4me_core import Application
app = Application.start(name="e11")
win = app.open_html("<p id=p>x</p>", width=200, height=100)
app.ready()
for i in range(20000):
    print(f"line {i}")
sys.stdout.flush()
print("progress: ", end="", flush=True)
for i in range(3):
    print(".", end="", flush=True); time.sleep(0.3)
print(" done", flush=True)
try:
    input("prompt> ")
except EOFError as e:
    print("input() -> EOFError", flush=True)
threading.Thread(target=lambda: (time.sleep(0.5), app.request_quit()), daemon=True).start()
app.run()
