import time, threading
from ui4me_core import Application
def main():
    with Application.start(name="Idle", app_id="org.ui4me.audit.Idle", instance="multiple") as app:
        app.open_html("<p>idle</p>", title="idle-probe", width=300, height=200)
        threading.Thread(target=lambda: (time.sleep(12), app.request_quit()), daemon=True).start()
        app.run()
if __name__ == "__main__": main()
