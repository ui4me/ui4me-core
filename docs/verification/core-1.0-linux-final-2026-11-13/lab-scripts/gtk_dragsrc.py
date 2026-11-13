#!/usr/bin/env python3
"""GTK3 Wayland drag source with controlled offers (test helper).

Usage: gtk_dragsrc.py MODE   (window title "DragSrc MODE")
MODE: mixed | https | bigtext | files1024 | files1025 | delayed | text
"""
import sys, os, time, gi, urllib.parse
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Gdk

mode = sys.argv[1]
base = os.path.expanduser("~/ui4me-dnd/many")
os.makedirs(base, exist_ok=True)


def files(n):
    out = []
    for i in range(n):
        p = os.path.join(base, f"soubor-ž-{i:04d}.txt")
        if not os.path.exists(p):
            open(p, "w").write("x")
        out.append(urllib.parse.urlunsplit(("file", "", urllib.parse.quote(p), "", "")))
    return out


URI, TEXT = 1, 2
targets = {
    "mixed": [("text/uri-list", URI), ("text/plain;charset=utf-8", TEXT)],
    "https": [("text/uri-list", URI)],
    "bigtext": [("text/plain;charset=utf-8", TEXT)],
    "files1024": [("text/uri-list", URI)],
    "files1025": [("text/uri-list", URI)],
    "delayed": [("text/plain;charset=utf-8", TEXT)],
    "text": [("text/plain;charset=utf-8", TEXT)],
    "rawuri": [("text/uri-list", URI)],
}[mode]
log = open(os.path.expanduser(f"~/ui4me-dnd/dragsrc-{mode}.log"), "a")


def data_get(widget, ctx, sel, info, t):
    log.write(f"{time.time():.3f} data-get target={sel.get_target().name()} info={info}\n"); log.flush()
    if mode == "delayed":
        time.sleep(3)
    if info == URI:
        if mode == "https":
            uris = ["https://example.org/ui4me"]
        elif mode == "mixed":
            uris = files(1)
        else:
            uris = files(1024 if mode == "files1024" else 1025)
        if mode == "rawuri":
            uris = ["file:///home/toor/ui4me-dnd/raw ž not encoded.txt", "::not a uri::"]
        ok = sel.set(sel.get_target(), 8, ("\r\n".join(uris) + "\r\n").encode())
        log.write(f"  set {len(uris)} uris ok={ok} first={uris[0]}\n"); log.flush()
    else:
        text = {"bigtext": "y" * (1100 * 1024), "mixed": "mixed offer text",
                "delayed": "delayed payload", "text": "plain helper text"}[mode]
        sel.set_text(text, -1)


win = Gtk.Window(title=f"DragSrc {mode}")
win.set_default_size(260, 160)
box = Gtk.EventBox()
box.add(Gtk.Label(label=f"drag me: {mode}"))
box.drag_source_set(Gdk.ModifierType.BUTTON1_MASK,
                    [Gtk.TargetEntry.new(n, 0, i) for n, i in targets], Gdk.DragAction.COPY)
box.connect("drag-data-get", data_get)
box.connect("drag-end", lambda *a: (log.write(f"{time.time():.3f} drag-end\n"), log.flush()))
win.add(box)
win.connect("destroy", Gtk.main_quit)
win.show_all()
Gtk.main()
