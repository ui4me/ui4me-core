"""Locate and drive controls through AT-SPI extents + KWin client geometry."""
import gi, time
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
import lab


def app(name):
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a and a.get_name() == name:
            return a
    return None


def wait_app(name, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        a = app(name)
        if a and a.get_child_count():
            return a
        time.sleep(0.4)
    raise TimeoutError(name)


def find(root, role=None, name=None, pred=None):
    def rec(a):
        try:
            ok = (role is None or a.get_role_name() == role) and (name is None or a.get_name() == name) and (pred is None or pred(a))
        except Exception:
            ok = False
        if ok:
            return a
        for i in range(a.get_child_count()):
            c = a.get_child_at_index(i)
            r = rec(c) if c else None
            if r:
                return r
    return rec(root)


def all_nodes(root):
    out = []
    def rec(a, d=0):
        out.append((d, a))
        for i in range(a.get_child_count()):
            c = a.get_child_at_index(i)
            if c: rec(c, d + 1)
    rec(root)
    return out


def center(acc, caption):
    e = Atspi.Component.get_extents(acc, Atspi.CoordType.WINDOW)
    w = lab.window(caption)
    return w['cx'] + e.x + e.width / 2, w['cy'] + e.y + e.height / 2


def click(acc, caption, button='left'):
    x, y = center(acc, caption)
    lab.click(x, y, name=button)


def text(acc):
    return Atspi.Text.get_text(acc, 0, -1)


def focused(root):
    for _, a in all_nodes(root):
        try:
            if a.get_state_set().contains(Atspi.StateType.FOCUSED):
                return (a.get_role_name(), a.get_name())
        except Exception:
            pass
    return None
