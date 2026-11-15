"""Application.start() called a second time inside the backend."""
import os, sys, time
from ui4me_core import Application
open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "twice.count"), "a").write(f"{os.getpid()}\n")
instance = sys.argv[1]
app = Application.start(name="e10", instance=instance)
app2 = Application.start(name="e10", instance=instance)   # e.g. a helper that also calls start()
print("second start returned", app2, flush=True)
