"""F-17b on the live KDE Wayland session: shortcuts under a Russian (JCUKEN) layout and
click press/release targets. Restores the keyboard configuration afterwards."""
import json, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import nlab

OUT = Path.home() / "lab/results/n-keys"
KXKB = Path.home() / ".config/kxkbrc"
report = {}


def layouts():
    return nlab.sh("qdbus6 --literal org.kde.keyboard /Layouts getLayoutsList").strip(), \
        nlab.sh("qdbus6 org.kde.keyboard /Layouts getLayout").strip()


def set_layouts(layout_list):
    """KWin 6.6 reloads kxkbrc through KConfigWatcher (kwriteconfig6 --notify); the legacy
    org.kde.keyboard.reloadConfig signal had no effect here. None = restore 'no kxkbrc'."""
    if layout_list is None:
        nlab.sh("kwriteconfig6 --notify --file kxkbrc --group Layout --key LayoutList us")
        time.sleep(4)  # KWin rewrites kxkbrc shortly after a layout change; delete after that
        KXKB.unlink(missing_ok=True)
        return
    nlab.sh("kwriteconfig6 --notify --file kxkbrc --group Layout --key Use true")
    nlab.sh(f"kwriteconfig6 --notify --file kxkbrc --group Layout --key LayoutList {layout_list}")
    time.sleep(1.5)


def new_events(app, n):
    return app.events()[n:]


def shortcut(app, combo, label, elements=("editor", "target")):
    n = len(app.events())
    nlab.keys(combo)
    time.sleep(0.5)
    st = app.cmd("state", selections=list(elements))
    rec = {"combo": combo, "events": new_events(app, n),
           **{k: v for k, v in st.items() if k.startswith(("sel:", "value:"))}}
    report.setdefault(label, []).append(rec)
    print(label, json.dumps(rec, ensure_ascii=False))
    return rec


original = KXKB.read_text() if KXKB.exists() else None
report["layouts_before"] = layouts()
app = nlab.App("keys", OUT)
try:
    s0 = nlab.shot(OUT / "k0.png")
    ox, oy = nlab.content_box(s0)[:2]
    rect = {e: app.cmd("command", name="get_rect", element=e)["rect"] for e in ("editor", "target", "pbtn", "pspan", "other")}
    report["rects"] = rect
    c = lambda e, dx=None, dy=None: (round(ox + rect[e]["x"] + (rect[e]["width"] / 2 if dx is None else dx)),
                                     round(oy + rect[e]["y"] + (rect[e]["height"] / 2 if dy is None else dy)))

    # --- US layout baseline
    nlab.click(*c("editor"))
    shortcut(app, "ctrl+a", "us")
    shortcut(app, "ctrl+c", "us")
    report["us_clipboard"] = app.cmd("clipboard")

    # --- switch to us,ru and activate ru
    set_layouts("us,ru")
    nlab.sh("qdbus6 org.kde.keyboard /Layouts setLayout 1")
    time.sleep(1)
    report["layouts_ru"] = layouts()
    # prove the layout is active: physical A/S/C typed into the textarea
    nlab.click(*c("target"))
    n = len(app.events())
    nlab.keys("a", "s", "c")
    report["ru_typed_value"] = app.cmd("state", selections=["target"])["value:target"]
    report["ru_typed_keydowns"] = [(e["key"], e["code"]) for e in new_events(app, n) if e["type"] == "keydown"]
    print("RU typed:", report["ru_typed_value"], report["ru_typed_keydowns"])
    app.cmd("set_value", element="target", value="")

    # editing/clipboard shortcuts in a text input on the RU layout
    nlab.click(*c("editor"))
    # a value different from the US-layout copy, so the clipboard proves the RU copy itself
    app.cmd("set_value", element="editor", value="ru copy 42")
    app.cmd("select", element="editor", start=0, end=0)
    shortcut(app, "ctrl+a", "ru")
    app.cmd("command", name="set_value", element="target", value="")
    shortcut(app, "ctrl+c", "ru")
    report["ru_clipboard_after_copy"] = app.cmd("clipboard")
    nlab.click(*c("target"))
    shortcut(app, "ctrl+v", "ru")
    shortcut(app, "ctrl+z", "ru")
    # app-defined shortcut: prevent_default(editor, keydown, key="s", ctrl=True) + keydown handler
    nlab.click(*c("editor"))
    shortcut(app, "ctrl+s", "ru")
    s1 = nlab.shot(OUT / "k1-ru.png")
    # prevent_default(editor, keydown, key="a", ctrl=True) must stop select-all on every layout
    app.cmd("prevent", element="editor", event="keydown", key="a", ctrl=True)
    app.cmd("select", element="editor", start=0, end=0)
    shortcut(app, "ctrl+a", "ru_prevented_select_all")
    nlab.sh("qdbus6 org.kde.keyboard /Layouts setLayout 0")
    time.sleep(1)
    app.cmd("select", element="editor", start=0, end=0)
    shortcut(app, "ctrl+a", "us_prevented_select_all")
    nlab.sh("qdbus6 org.kde.keyboard /Layouts setLayout 1")
    time.sleep(1)

    # --- click press/release (layout-independent; still on ru)
    def pr(label, a, b):
        n = len(app.events())
        nlab.press_release(*a, *b)
        ev = [e for e in new_events(app, n) if e["type"] in ("pointerdown", "pointerup", "click")]
        report.setdefault("press_release", []).append({"case": label, "from": a, "to": b, "events": ev})
        print(label, json.dumps(ev))
    pr("span->button padding", c("pspan"), c("pbtn", dx=12))
    pr("button->other button", c("pbtn", dx=12), c("other"))
    pr("same button", c("pbtn", dx=12), c("pbtn", dx=20))
finally:
    try:
        nlab.sh("qdbus6 org.kde.keyboard /Layouts setLayout 0", check=False)
        set_layouts(None if original is None else "us")  # original is None or the old text
        if original is not None:
            KXKB.write_text(original)
        report["layouts_after_restore"] = layouts()
    finally:
        app.close()
        (OUT / "report.json").write_text(json.dumps(report, indent=1, ensure_ascii=False))
print("restored:", report.get("layouts_after_restore"))
