import json, subprocess, sys, time, uuid
from pathlib import Path
HERE = Path(__file__).resolve().parent
out = HERE / "runs" / f"secondary-{uuid.uuid4().hex[:6]}"; out.mkdir(parents=True)
app = f"org.ui4me.Sec{uuid.uuid4().hex[:6]}"
def manifest(name, scenario):
    ev = out / f"{name}.jsonl"
    m = out / f"{name}.toml"
    m.write_text("\n".join(["[app]", f'id = "{app}"', 'name = "S"', "", "[backend]",
      f'program = "{sys.executable}"', f'args = {json.dumps([str(HERE/"adv_backend.py"), scenario, str(ev)])}',
      f'cwd = "{HERE}"', "", "[host]", "shutdown_timeout_ms = 8000", ""]))
    return m, ev
m1, ev1 = manifest("primary", "shutdown_ignore")
p1 = subprocess.Popen([str(HERE/"ui4me-host"), "run", "--app", str(m1), "--headless"], stdout=open(out/"p1.log","wb"), stderr=subprocess.STDOUT)
while not (ev1.exists() and "app.shutdown" in ev1.read_text()): time.sleep(0.05)
m2, _ = manifest("secondary", "auth")
t = time.monotonic()
r = subprocess.run([str(HERE/"ui4me-host"), "run", "--app", str(m2), "--headless"], capture_output=True, text=True, timeout=60)
print("secondary during primary shutdown: exit", r.returncode, "after", round(time.monotonic()-t,2), "s; stderr:", r.stderr.strip()[-300:])
print("primary exit", p1.wait(30))
