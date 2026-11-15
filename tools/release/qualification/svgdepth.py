"""F-03: an image SVG with DEPTH nested <g> groups must not abort the host.

Usage: python svgdepth.py [DEPTH=700]
"""
import base64
import time

from _probe import int_argument, run_probe, settle_seconds

DEPTH = int_argument(1, 700)
SVG = ('<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">' + '<g>' * DEPTH
       + '<rect width="10" height="10" fill="red"/>' + '</g>' * DEPTH + '</svg>')
URL = 'data:image/svg+xml;base64,' + base64.b64encode(SVG.encode()).decode()


def body(app, window):
    document = window.document
    time.sleep(settle_seconds(0.5))
    document.insert_html('box', f'<img id="im" src="{URL}" width="40" height="40">')
    time.sleep(settle_seconds(2.0))
    text = document.get_text('s')
    if text != 'start':
        raise AssertionError(f'unexpected text {text!r}')
    return f'depth={DEPTH}'


if __name__ == '__main__':
    run_probe('svgdepth', '<p id="s">start</p><div id="box"></div>', body, title=f'UI4ME SVG depth {DEPTH}')
