"""Raw Core 1.0.1 replay of UI4ME's tick+region op stream, with CSS variants; prints host RSS."""
import os, re, threading, time
from ui4me_core import Application
TAG = os.environ.get("LEAK_TAG", "r"); CSS = os.environ.get("LEAK_CSS", "full")
P = os.path.dirname(os.path.abspath(__file__))
doc = open(os.path.join(P, "doc.html"), encoding="utf-8").read()
region = open(os.path.join(P, "region.html"), encoding="utf-8").read()
if CSS == "nofunctional":
    doc = re.sub(r"<style>.*?</style>", "", doc, flags=re.S)
app = Application.start(name="Core replay " + TAG, app_id="org.ui4me.test.CoreReplay" + TAG)
w = app.open_html(doc, title="Core replay " + TAG, width=500, height=400)
d = w.document
theme = ''
if CSS == "nohas":
    theme = "\n".join(l for l in theme.splitlines() if ":has(" not in l)
def blocks(css):
    out, depth, cur = [], 0, ""
    for ch in css:
        cur += ch
        if ch == "{": depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0: out.append(cur); cur = ""
    return out
if CSS.startswith("slice:"):
    _, a, b = CSS.split(":"); bl = blocks(theme); n = len(bl)
    theme = "".join(bl[int(float(a) * n):int(float(b) * n)])
    print("BLOCKS", n, int(float(a) * n), int(float(b) * n), flush=True)
    open(os.path.join(P, f"css-{TAG}.css"), "w").write(theme)
if CSS == "custom":
    theme = os.environ["LEAK_CSSTEXT"]
if CSS in ("full", "nohas", "custom") or CSS.startswith("slice:"):
    d.add_stylesheet(theme)
def rss(pid):
    for line in open(f"/proc/{pid}/status"):
        if line.startswith("VmRSS:"): return int(line.split()[1])
def body():
    t0 = time.monotonic(); n = 0; gen = 0; current = None; last = 0
    while time.monotonic() - t0 < 150:
        n += 1
        d.batch([{"op": "set_text", "element": "u4-6", "text": str(n)}] +
                ([{"op": "set_text", "element": f"g{gen}-u4-11", "text": f"region tick {n}"}] if current else []))
        if time.monotonic() - last > 1.0:
            last = time.monotonic()
            if current:
                d.batch([{"op": "remove_element", "element": current}]); current = None
            else:
                gen += 1
                html = re.sub(r'id="(u4-\d+)"', lambda m: f'id="g{gen}-{m.group(1)}"', region)
                d.batch([{"op": "insert_html", "element": "u4-8", "position": "beforeend", "html": html}])
                current = f"g{gen}-u4-9"
        if n % 1500 == 0: print("SAMPLE", TAG, round(time.monotonic() - t0, 1), n, rss(os.getppid()), flush=True)
        time.sleep(0.02)
    app.request_quit()
threading.Thread(target=body, daemon=True).start()
app.run()

