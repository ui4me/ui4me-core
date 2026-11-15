"""F-01: a document nested DEPTH levels deep must render without aborting the host.

Usage: python depth.py [DEPTH=250]   (the documented limit is 256)
"""
import time

from _probe import int_argument, run_probe, settle_seconds

DEPTH = int_argument(1, 250)
DEEP = '<div>' * DEPTH + '<span id="leaf">deep</span>' + '</div>' * DEPTH


def body(app, window):
    time.sleep(settle_seconds(0.5))
    document = window.load_html(DEEP)
    time.sleep(settle_seconds(1.5))
    text = document.get_text('leaf')
    rect = document.command('get_rect', element='leaf')
    if text != 'deep':
        raise AssertionError(f'unexpected leaf text {text!r}')
    return f'depth={DEPTH} rect={rect}'


if __name__ == '__main__':
    run_probe('depth', '<p id="s">start</p>', body, title=f'UI4ME depth {DEPTH}')
