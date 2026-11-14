"""Live bitmap: Python writes raw RGBA8 frames into a host-created shared buffer.

Run it directly:
  python app.py          (or: uv run app.py)
Or through the manifest:  ui4me-host run --app app.toml

Frames are assembled by copying rows out of a precomputed colour wave; a
per-pixel Python loop is too slow for smooth animation. A producer thread
paces itself to about 60 frames/s while app.run() handles events.
"""
import math
import threading
import time
from pathlib import Path

from ui4me_core import Application, RpcError

HERE = Path(__file__).resolve().parent
WIDTH, HEIGHT = 320, 200
FRAME_SECONDS = 1 / 60  # pace to ~60 frames/s; send faster only if you need it


def make_wave():
    """One 512-pixel colour wave, repeated so any 320-pixel window can be copied."""
    wave = bytearray(1024 * 4)
    for x in range(1024):
        t = (x % 512) / 512.0 * 2 * math.pi
        wave[x * 4] = round(127 + 127 * math.sin(t))
        wave[x * 4 + 1] = round(127 + 127 * math.sin(t + 2.094))
        wave[x * 4 + 2] = round(127 + 127 * math.sin(t + 4.189))
        wave[x * 4 + 3] = 255
    return bytes(wave)


def main():
    with Application.start(name="Python Pixel Preview", app_id="org.ui4me.PythonLiveBitmap") as app:
        window = app.open_file(HERE / "index.html", title="Python Live Pixels", width=780, height=560)
        document = window.document
        bitmap = document.create_bitmap("preview", WIDTH, HEIGHT)
        stopping = threading.Event()
        wave = make_wave()

        def produce():
            frame = bytearray(WIDTH * HEIGHT * 4)
            row_bytes = WIDTH * 4
            started = time.monotonic()
            step = 0
            while not stopping.is_set():
                # Sleep until the next frame is due (or until shutdown).
                delay = started + (step + 1) * FRAME_SECONDS - time.monotonic()
                if delay > 0 and stopping.wait(delay):
                    break
                for y in range(HEIGHT):
                    shift = (step * 3 + round(40 * math.sin((y + step) / 23.0))) % 512
                    frame[y * row_bytes:(y + 1) * row_bytes] = wave[shift * 4:shift * 4 + row_bytes]
                try:
                    # Returns once the host has taken the frame: natural backpressure.
                    bitmap.present(frame)
                    step += 1
                    if step % 30 == 0:
                        fps = step / max(0.001, time.monotonic() - started)
                        document.set_text("fps", f"Frame {step} · {WIDTH} × {HEIGHT} RGBA8 · {fps:.0f} frames/s")
                except Exception as error:
                    # Closing the window or quitting retires the bitmap mid-animation.
                    ending = stopping.is_set() or window.closed or app.rpc.shutdown_requested.is_set()
                    if isinstance(error, RpcError) and error.code == -32008:  # shutting down
                        ending = True
                    if ending:
                        break
                    raise

        producer = threading.Thread(target=produce, daemon=True, name="frame-producer")
        producer.start()
        try:
            app.run()
        finally:
            stopping.set()
            producer.join(timeout=2)
            bitmap.close()


if __name__ == "__main__":
    main()
