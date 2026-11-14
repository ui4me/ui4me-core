"""Native live bitmap example: streaming pixels, no encoded image or browser.

Run it directly (python live_bitmap_demo.py) or through live-bitmap.toml.
This example exercises two-slot ownership,
coalescing, document epochs, ordinary button events, and graceful shutdown.
"""
from __future__ import annotations
from pathlib import Path
from threading import Event, Thread
import time
from ui4me_core import Application

UI = Path(__file__).resolve().parent / 'ui' / 'live_bitmap.html'


def main() -> None:
    stopping = Event()
    with Application.start(name="UI4ME Live Graphics", app_id="org.ui4me.LiveBitmapDemo") as app:
        window = app.open_file(UI, title='UI4ME — Live bitmap & custom title bar',
                               width=700, height=510, decorations=False)
        document = window.document
        assert document is not None
        bitmap = document.create_bitmap('preview', 320, 200, slots=2)
        document.on('click',lambda _:window.set(minimized=True),element='minimize')
        # `window.get` separates requested from observed state; toggle the
        # compositor's actual maximize state, not a missing top-level key.
        document.on('click',lambda _:window.set(maximized=not bool((window.state().get('observed') or {}).get('maximized'))),element='maximize')
        document.on('click',lambda _:window.request_close(),element='close')
        window.on_before_close(lambda: (stopping.set() or True))

        def produce() -> None:
            frame = bytearray(320 * 200 * 4)
            count = 0
            while not stopping.wait(1/24):
                count += 1
                phase = count & 255
                for y in range(200):
                    for x in range(320):
                        i = (y * 320 + x) * 4
                        frame[i:i+4] = bytes(((x+phase)&255, (y+phase)&255, (x^y^phase)&255, 255))
                try:
                    bitmap.try_present(frame)
                except Exception:
                    stopping.set()
                    break
        worker = Thread(target=produce,daemon=True,name='ui4me-frame-producer')
        worker.start()
        try:
            app.run()
        finally:
            stopping.set()
            worker.join(timeout=2)
            bitmap.close()

if __name__ == '__main__':main()
