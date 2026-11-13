import subprocess, json, time, os
def host_pid(app_id="org.ui4me.Rc2Lab"):
    out = subprocess.check_output(["pgrep", "-u", str(os.getuid()), "-x", "ui4me-host"], text=True).split()
    for pid in out:
        try:
            env = open(f"/proc/{pid}/cmdline","rb").read().split(b"\0")
            if any(b"Rc2Lab" in a or b"rc2" in a.lower() or b"labapp" in a for a in env): return int(pid)
        except OSError: pass
    return None
def backend_pid():
    out = subprocess.check_output(["pgrep", "-f", "lab_app.py"], text=True).split()
    return int(out[0]) if out else None
def pss(pid):
    for l in open(f"/proc/{pid}/smaps_rollup"):
        if l.startswith("Pss:"): return int(l.split()[1])
def rss(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("VmRSS:"): return int(l.split()[1])
def ticks(pid):
    p = open(f"/proc/{pid}/stat").read().rsplit(")",1)[1].split(); return int(p[11])+int(p[12])
def threads(pid): return len(os.listdir(f"/proc/{pid}/task"))
def fds(pid): return len(os.listdir(f"/proc/{pid}/fd"))
