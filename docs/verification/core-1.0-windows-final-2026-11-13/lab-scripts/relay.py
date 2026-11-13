"""Admin-SSH relay: JSON lines on stdin -> lab agent pipe -> JSON lines on stdout."""
import json, sys
from multiprocessing.connection import Client
from pathlib import Path
key = (Path(__file__).resolve().parent / "agent.key").read_text().strip().encode()
conn = Client(r"\\.\pipe\ui4me-lab-agent", family="AF_PIPE", authkey=key)
for line in sys.stdin:
    conn.send(json.loads(line))
    sys.stdout.write(json.dumps(conn.recv()) + "\n"); sys.stdout.flush()
