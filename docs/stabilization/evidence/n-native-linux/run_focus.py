"""F-08 (focus scroll-into-view) and F-17 (Enter on a link) on the live KDE Wayland session."""
import json, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import nlab

OUT = Path.home() / "lab/results/n-focus"
FOCUS_RGB = (255, 42, 0)  # :focus outline colour
report = {"steps": []}


def step(name, app, extra=None, shot=True):
    time.sleep(0.6)
    st = app.cmd("state")
    rec = {"step": name, "focused": (st["focused"] or {}).get("element"), "scroll_y": st["scroll"]["y"]}
    if extra:
        rec.update(extra)
    acc = nlab.focused(at)
    rec["atspi_focused"] = nlab.describe(acc)
    if acc is not None and "extents" in rec["atspi_focused"]:
        x, y, w, h = rec["atspi_focused"]["extents"]["window"]
        rec["atspi_inside_viewport"] = x >= 0 and y >= 0 and x + w <= 800 and y + h <= 500
    if shot:
        p = nlab.shot(OUT / f"{name}.png")
        box = nlab.content_box(p)
        ring = nlab.colour_box(p, FOCUS_RGB, region=box[:4] if box else None)
        rec["content_box"] = box
        rec["focus_ring_screen"] = ring
        if box and ring:
            rec["focus_ring_in_window"] = [ring[0] - box[0], ring[1] - box[1], ring[2] - box[0], ring[3] - box[1]]
    report["steps"].append(rec)
    print(json.dumps(rec, ensure_ascii=False))
    return rec


app = nlab.App("focus", OUT)
at = nlab.atspi_app()
s0 = nlab.shot(OUT / "s0.png")
box = nlab.content_box(s0)
ox, oy = box[0], box[1]
report["content_origin"] = [ox, oy]

# 1. pointer focus on the top button must not scroll
nlab.click(ox + 90, oy + 90)
step("01-pointer-focus-b1", app)
# 2. injected Tab to the far button (y~1535, viewport 500 high)
nlab.keys("tab")
step("02-tab-far", app)
# 3. Tab to the far link, then Enter (F-17)
nlab.keys("tab")
step("03-tab-farlink", app)
n_before = len(app.events())
nlab.keys("enter")
time.sleep(0.6)
ev = app.events()[n_before:]
report["enter_on_link_events"] = ev
print("ENTER events:", json.dumps(ev, ensure_ascii=False))
# Space on the link must not activate it (browser behaviour)
n_before = len(app.events())
nlab.keys("space")
time.sleep(0.6)
report["space_on_link_events"] = app.events()[n_before:]
print("SPACE events:", json.dumps(report["space_on_link_events"], ensure_ascii=False))
# 4. Tab into the nested overflow:auto box (innermost scrolls first, then the page)
nlab.keys("tab")
r = step("04-tab-inner", app)
report["inner_box_scroll"] = app.cmd("command", name="get_rect", element="inner")
# 5. AT-SPI grabFocus on an off-screen button, page scrolled back to the top first
app.cmd("scroll_to", y=0)
target = nlab.find(at, name="AT-SPI target", role="button")
report["atspi_target_before"] = nlab.describe(target)
ok = target.queryComponent().grabFocus()
report["grabFocus_returned"] = bool(ok)
time.sleep(0.6)
step("05-atspi-grabfocus", app, {"atspi_target_after": nlab.describe(target)})
# 5b. AT-SPI scrollTo-free check: Component.scrollTo is a separate API; not used here.
# 6. backend focus() on an off-screen button
app.cmd("scroll_to", y=0)
res = app.cmd("focus", element="apibtn")
step("06-backend-focus", app, {"focus_reply_scroll_y": res["scroll"]["y"]})
report["events_tail"] = [e for e in app.events() if e["type"] in ("focus", "link_activated", "click")]
app.close()
(OUT / "report.json").write_text(json.dumps(report, indent=1, ensure_ascii=False))
