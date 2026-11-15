"""Optional: Orca (real screen reader, AT-SPI) announces focused button/link names while Tab is
injected. Orca runs with --debug-file; speech lines are extracted from its debug log."""
import re, subprocess, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import nlab

OUT = Path.home() / "lab/results/n-orca"
OUT.mkdir(parents=True, exist_ok=True)
dbg = OUT / "orca-debug.txt"
orca = subprocess.Popen(["orca", "--replace", f"--debug-file={dbg}"], env=nlab.ENV,
                        stdout=open(OUT / "orca.out", "w"), stderr=subprocess.STDOUT)
time.sleep(6)
app = nlab.App("focus", OUT / "app")
ox, oy = nlab.content_box(nlab.shot(OUT / "o0.png"))[:2]
nlab.click(ox + 90, oy + 90)
time.sleep(1.5)
for _ in range(3):
    nlab.keys("tab")
    time.sleep(1.5)
app.close()
time.sleep(1)
orca.terminate()
try:
    orca.wait(10)
except subprocess.TimeoutExpired:
    orca.kill()
text = dbg.read_text(errors="replace") if dbg.exists() else ""
speech = [l.strip() for l in text.splitlines() if re.search(r"SPEECH OUTPUT|speech.speak|SPEAK", l)]
(OUT / "speech.txt").write_text("\n".join(speech))
print(f"orca debug lines: {len(text.splitlines())}; speech lines: {len(speech)}")
print("\n".join(speech[-40:]))
