import json, os, signal, subprocess, sys, time, uuid
from pathlib import Path
HERE = Path(__file__).resolve().parent
sig = getattr(signal, sys.argv[1])
out = HERE / "runs" / f"signal-{sys.argv[1]}-{uuid.uuid4().hex[:6]}"; out.mkdir(parents=True)
ev = out / "evidence.jsonl"
m = out / "app.toml"
m.write_text("\n".join(["[app]", f'id = "org.ui4me.Sig{uuid.uuid4().hex[:6]}"', 'name = "S"', "", "[backend]",
  f'program = "{sys.executable}"', f'args = {json.dumps([str(HERE/"adv_backend.py"), "wait_notifications", str(ev)])}',
  f'cwd = "{HERE}"', ""]))
p = subprocess.Popen([str(HERE/"ui4me-host"), "run", "--app", str(m), "--headless"], stdout=open(out/"host.log","wb"), stderr=subprocess.STDOUT)
for _ in range(100):
    if ev.exists() and "waiting" in ev.read_text(): break
    time.sleep(0.05)
p.send_signal(sig); t=time.monotonic()
code = p.wait(10)
print("host exit", code, "after", round(time.monotonic()-t,3))
time.sleep(0.5)
print(ev.read_text())
rt = Path(os.environ.get("XDG_RUNTIME_DIR")) / "ui4me-host"
print("stale sockets now:", len(list(rt.glob("*.sock"))))
