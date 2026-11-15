"""F-02 / issue #1: N transitioned controls, then toggle the colour scheme ROUNDS times.

Each toggle starts transitions on every control and the host emits a burst of
transition events to a backend that subscribed to them. The connection must
survive every round (a document round trip after each round proves it).
Usage: python burst.py [N=100] [ROUNDS=6] [env|class]
"""
import sys
import time

from _probe import int_argument, run_probe, settle_seconds

N = int_argument(1, 100)
ROUNDS = int_argument(2, 6)
MODE = sys.argv[3] if len(sys.argv) > 3 else 'env'
CSS = """
button { transition: background-color 120ms, border-color 120ms, color 120ms;
         background: #eee; border: 1px solid #999; color: #111 }
@media (prefers-color-scheme: dark) { button { background: #333; border-color: #777; color: #eee } }
.dark button { background: #333; border-color: #777; color: #eee }
"""
HTML = '<style>%s</style><div id="root">%s</div><p id="s">x</p>' % (
    CSS, ''.join(f'<button id="b{i}">B{i}</button>' for i in range(N)))


def body(app, window):
    document = window.document
    events = {'n': 0}

    def on_event(event):
        events['n'] += 1
    for kind in ('transitionrun', 'transitionstart', 'transitionend'):
        document.on(kind, on_event)
    time.sleep(settle_seconds(1.0))
    for round_index in range(ROUNDS):
        if MODE == 'env':
            document.set_environment(color_scheme='dark' if round_index % 2 == 0 else 'light')
        else:
            (document.add_class if round_index % 2 == 0 else document.remove_class)('root', 'dark')
        time.sleep(0.4)
        document.get_text('s')   # round trip: the connection is still alive
        print(f'ROUND_OK {round_index} events {events["n"]}', flush=True)
    return f'controls={N} rounds={ROUNDS} mode={MODE} events={events["n"]}'


if __name__ == '__main__':
    run_probe('burst', HTML, body, title=f'UI4ME burst {N}', width=800, height=600)
