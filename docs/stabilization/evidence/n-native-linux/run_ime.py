"""F-07 IME placement on the live KDE Wayland session: fcitx5 (pinyin) started by KWin as the
input method (zwp_input_method_v1), UI4ME host using text-input-v3. For each input inside a
position:fixed box, a translate()-centred fixed modal and a scale(1.5) box (page scrolled by
400px), compose "ni" after character 3 and locate the candidate popup in the screenshot."""
import json, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import nlab
import pyatspi

OUT = Path.home() / "lab/results/n-ime"
POPUP = (61, 174, 233)  # Breeze accent: fcitx5 candidate popup border / highlighted candidate
report = {"inputs": {}}


def entry(at, prefix):
    for acc, _ in nlab.walk(at):
        try:
            if acc.getRoleName() == "entry" and acc.queryText().getText(0, -1).startswith(prefix):
                return acc
        except Exception:
            continue


app = nlab.App("geom", OUT)
at = nlab.atspi_app()
s0 = nlab.shot(OUT / "i0.png")
ox, oy = nlab.content_box(s0)[:2]
app.cmd("scroll_to", y=400)
time.sleep(0.6)
for inp, prefix in (("finput", "fixed"), ("tinput", "modal"), ("sinput", "scaled")):
    rec = {}
    acc = entry(at, prefix)
    t = acc.queryText()
    ext = nlab.extents(acc)["window"]
    c3 = t.getCharacterExtents(3, pyatspi.WINDOW_COORDS)
    # click the left part of character 3 -> caret 3
    nlab.click(ox + c3[0] + 2, oy + c3[1] + c3[3] // 2)
    rec["caret_before"] = t.caretOffset
    nlab.sh("ydotool key 29:1 57:1 57:0 29:0")  # Ctrl+Space: fcitx5 trigger
    time.sleep(0.8)
    rec["fcitx5_state"] = nlab.sh("fcitx5-remote", check=False).strip()
    nlab.sh("ydotool type -d 60 ni")
    time.sleep(1.2)
    shot = nlab.shot(OUT / f"ime-{inp}.png")
    caret = t.caretOffset
    ce = t.getCharacterExtents(caret, pyatspi.WINDOW_COORDS) if caret < t.characterCount else None
    rec["value_during_composition"] = t.getText(0, -1)
    rec["caret_during_composition"] = caret
    rec["atspi_entry_window"] = ext
    rec["atspi_char_at_caret_window"] = list(ce) if ce else None
    below = oy + ext[1] + ext[3] + 3
    popup = nlab.colour_box(shot, POPUP, region=(0, below, 1280, min(800, below + 120)))
    rec["popup_screen"] = popup
    if popup and ce:
        rec["popup_minus_caret_px"] = [popup[0] - (ox + ce[0]), popup[1] - (oy + ce[1] + ce[3])]
        rec["popup_minus_entry_bottom_px"] = popup[1] - (oy + ext[1] + ext[3])
    rec["composition_events"] = [e["type"] for e in app.events() if e["type"].startswith("composition")][-4:]
    nlab.keys("esc")                                        # cancel the composition
    nlab.sh("ydotool key 29:1 57:1 57:0 29:0")              # IME off again
    time.sleep(0.5)
    rec["value_after_cancel"] = t.getText(0, -1)
    report["inputs"][inp] = rec
    print(inp, json.dumps(rec, ensure_ascii=False))
app.close()
(OUT / "report.json").write_text(json.dumps(report, indent=1, ensure_ascii=False))
