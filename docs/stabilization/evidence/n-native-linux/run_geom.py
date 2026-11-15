"""F-07 on the live KDE Wayland session: AT-SPI geometry vs pixels for inputs inside a
position:fixed box, a translate()-centred modal and a scale(1.5) box, page scrolled by 400px.
Also: an injected click on a visible character puts the caret there (pointer -> caret mapping)."""
import json, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import nlab

OUT = Path.home() / "lab/results/n-geom"
BOXES = {"finput": ("fixed", (255, 243, 196)), "tinput": ("modal", (217, 255, 217)),
         "sinput": ("scaled", (255, 217, 240))}
report = {"inputs": {}}


def ink_box(path, region):
    """Dark text pixels inside region (the input's white padding box)."""
    from PIL import Image
    im = Image.open(path).convert("L")
    px = im.load()
    x0, y0, x1, y1 = region
    xs, ys = [], []
    for y in range(y0 + 2, y1 - 2):
        for x in range(x0 + 2, x1 - 2):
            if px[x, y] < 110:
                xs.append(x)
                ys.append(y)
    return (min(xs), min(ys), max(xs) + 1, max(ys) + 1) if xs else None


def atspi_entry(at, value):
    import pyatspi
    for acc, _ in nlab.walk(at):
        try:
            if acc.getRoleName() in ("entry", "text") and "Text" in pyatspi.listInterfaces(acc):
                t = acc.queryText()
                if t.getText(0, -1) == value:
                    return acc
        except Exception:
            continue
    return None


app = nlab.App("geom", OUT)
at = nlab.atspi_app()
s0 = nlab.shot(OUT / "g0.png")
ox, oy = nlab.content_box(s0)[:2]
report["content_origin"] = [ox, oy]
report["scroll"] = app.cmd("scroll_to", y=400)
time.sleep(0.8)
s1 = nlab.shot(OUT / "g1-scrolled.png")
import pyatspi
for inp, (box_id, rgb) in BOXES.items():
    rec = {}
    value = app.cmd("state", selections=[inp])[f"value:{inp}"]
    rect = app.cmd("command", name="get_rect", element=inp)["rect"]
    rec["get_rect"] = rect
    colour = nlab.colour_box(s1, rgb, region=(ox, oy, ox + 800, oy + 500))
    white = nlab.colour_box(s1, (255, 255, 255), region=colour[:4]) if colour else None
    rec["pixels_box_window"] = [colour[0] - ox, colour[1] - oy, colour[2] - ox, colour[3] - oy] if colour else None
    rec["pixels_input_white_window"] = [white[0] - ox, white[1] - oy, white[2] - ox, white[3] - oy] if white else None
    ink = ink_box(s1, white[:4]) if white else None
    rec["pixels_text_ink_window"] = [ink[0] - ox, ink[1] - oy, ink[2] - ox, ink[3] - oy] if ink else None
    acc = atspi_entry(at, value)
    if acc is None:
        rec["atspi"] = "entry not found"
    else:
        rec["atspi_extents"] = nlab.extents(acc)["window"]
        t = acc.queryText()
        ce = []
        for i in range(len(value)):
            x, y, w, h = t.getCharacterExtents(i, pyatspi.WINDOW_COORDS)
            ce.append([x, y, w, h])
        rec["atspi_char_extents"] = ce
        rec["atspi_text_left_right"] = [ce[0][0], ce[-1][0] + ce[-1][2]]
    # pointer -> caret: click on the left quarter of visual character 5 (from pixels only)
    if ink:
        adv = (ink[2] - ink[0]) / (len(value) - 0.15)
        cx = round(ink[0] + 5 * adv + 0.25 * adv)
        cy = round((ink[1] + ink[3]) / 2)
        nlab.click(cx, cy)
        time.sleep(0.4)
        sel = app.cmd("state", selections=[inp])[f"sel:{inp}"]
        rec["click_char5_screen"] = [cx, cy]
        rec["caret_after_click"] = [sel["start"], sel["end"]]
        if acc is not None:
            rec["atspi_caret_offset"] = acc.queryText().caretOffset
            rec["atspi_focused"] = acc.getState().contains(pyatspi.STATE_FOCUSED)
    rec["scroll_after"] = app.cmd("state")["scroll"]["y"]
    report["inputs"][inp] = rec
    print(inp, json.dumps(rec))
nlab.shot(OUT / "g2-after-clicks.png")
app.close()
(OUT / "report.json").write_text(json.dumps(report, indent=1))
