import json, os, socket, subprocess, sys, time, uuid, struct
from pathlib import Path
HERE = Path(__file__).resolve().parent
limit = int(sys.argv[1])
out = HERE / "runs" / f"fd-{uuid.uuid4().hex[:6]}"; out.mkdir(parents=True)
ev = out / "evidence.jsonl"
m = out / "app.toml"
app = f"org.ui4me.Fd{uuid.uuid4().hex[:6]}"
m.write_text("\n".join(["[app]", f'id = "{app}"', 'name = "S"', "", "[backend]",
  f'program = "{sys.executable}"', f'args = {json.dumps([str(HERE/"adv_backend.py"), "wait_notifications", str(ev)])}',
  f'cwd = "{HERE}"', ""]))
p = subprocess.Popen(["prlimit", f"--nofile={limit}", "--", str(HERE/"ui4me-host"), "run", "--app", str(m), "--headless"], stdout=open(out/"host.log","wb"), stderr=subprocess.STDOUT)
for _ in range(200):
    if ev.exists() and "waiting" in ev.read_text(): break
    if p.poll() is not None: break
    time.sleep(0.05)
print("ready" if ev.exists() and "waiting" in ev.read_text() else "not ready", "host alive:", p.poll() is None)
print("host fds:", len(os.listdir(f"/proc/{p.pid}/fd")) if p.poll() is None else None)
rt = Path(os.environ["XDG_RUNTIME_DIR"]) / "ui4me-host"
socks = sorted(rt.glob("*.sock"), key=lambda x: x.stat().st_mtime)[-1:]
# The endpoint: find via /proc/<pid>/net? use newest socket file
ep = str(socks[0])
conns = []
for i in range(40):
    try:
        s = socket.socket(socket.AF_UNIX); s.connect(ep); conns.append(s)
    except OSError as e:
        print("connect", i, "failed:", e); break
time.sleep(1)
print("host fds after flood:", len(os.listdir(f"/proc/{p.pid}/fd")) if p.poll() is None else None)
for s in conns: s.close()
time.sleep(6)
print("socket file still exists:", os.path.exists(ep))
try:
    s = socket.socket(socket.AF_UNIX); s.connect(ep); print("late connect OK")
    s.sendall(b"\x00\x00\x00\x02{}"); s.settimeout(3); print("late reply:", s.recv(200)[:120])
except OSError as e:
    print("late connect failed:", e)
p.terminate(); p.wait(5)
log = (out/"host.log").read_text()
print([l[:250] for l in log.splitlines() if "WARN" in l or "ERROR" in l or "listener" in l][:5])
