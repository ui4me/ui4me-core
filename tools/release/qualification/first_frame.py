"""Open one window and keep it up long enough for the first presented frame.

The runner requires the host's "first frame presented" record (UI4ME_VERBOSE=1)
in a windowed run; the backend checks the document round trip.
Usage: python first_frame.py
"""
import time

from _probe import run_probe, settle_seconds

HTML = '<h1 id="t">first frame</h1><p>UI4ME windowed smoke</p>'


def body(app, window):
    time.sleep(settle_seconds(1.0))
    text = window.document.get_text('t')
    if text != 'first frame':
        raise AssertionError(f'unexpected text {text!r}')
    return 'text-ok'


if __name__ == '__main__':
    run_probe('first-frame', HTML, body, title='UI4ME first frame')
