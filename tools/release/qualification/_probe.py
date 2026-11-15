"""Shared harness for the qualification probes (kept dependency-free).

Each probe is a normal ``python probe.py`` UI4ME app: the first run starts the
host, which runs the script again as its backend. The backend prints exactly
one ``RESULT PASS <probe> ...`` or ``RESULT FAIL <probe> ...`` line and quits.
A host abort (for example a stack overflow) shows up as a missing RESULT line
and a non-zero exit status, which the runner reports as a failure.
"""
from __future__ import annotations

import os
import sys
import threading
import time
import traceback


def settle_seconds(default: float) -> float:
    """Time for the host to lay out and paint after a change (windowed runs)."""
    try:
        return float(os.environ.get('UI4ME_PROBE_SETTLE', default))
    except ValueError:
        return default


def run_probe(name: str, html: str, body, *, title: str, width: int = 480, height: int = 360) -> None:
    from ui4me_core import Application

    app_id = 'org.ui4me.qualification.' + ''.join(c for c in name.title() if c.isalnum())
    with Application.start(name=f'UI4ME qualification {name}', app_id=app_id, instance='multiple') as app:
        window = app.open_html(html, title=title, width=width, height=height)

        def worker():
            started = time.monotonic()
            try:
                detail = body(app, window)
                print(f'RESULT PASS {name} {detail} seconds={time.monotonic() - started:.2f}', flush=True)
            except BaseException as error:  # noqa: BLE001 - every failure must be reported
                traceback.print_exc()
                print(f'RESULT FAIL {name} {type(error).__name__}: {str(error)[:200]}', flush=True)
            finally:
                try:
                    app.request_quit()
                except Exception:  # noqa: BLE001 - the host may already be gone
                    pass
        threading.Thread(target=worker, daemon=True).start()
        app.run()


def int_argument(index: int, default: int) -> int:
    return int(sys.argv[index]) if len(sys.argv) > index else default
