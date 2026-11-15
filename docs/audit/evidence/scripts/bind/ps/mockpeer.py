"""Minimal UI4ME host peer for PowerShell client experiments (not a real host)."""
import json, os, socket, struct, subprocess, sys, threading, time, uuid

def frame(obj):
    b = json.dumps(obj, separators=(",", ":")).encode()
    return struct.pack(">I", len(b)) + b

def read_exact(s, n):
    buf = b""
    while len(buf) < n:
        p = s.recv(n - len(buf))
        if not p:
            return None
        buf += p
    return buf

def read_msg(s):
    h = read_exact(s, 4)
    if h is None:
        return None
    (n,) = struct.unpack(">I", h)
    return json.loads(read_exact(s, n))

def main():
    script = sys.argv[1]
    extra = sys.argv[2:]
    path = f"/tmp/claude-1000/ui4me-mock-{uuid.uuid4().hex[:10]}.sock"
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path); srv.listen(1)
    env = dict(os.environ, UI4ME_ENDPOINT=path, UI4ME_TOKEN="tok")
    proc = subprocess.Popen(["pwsh", "-NoProfile", "-NonInteractive", "-File", script, *extra], env=env)
    srv.settimeout(60)
    conn, _ = srv.accept()
    wlock = threading.Lock()
    def send(obj):
        with wlock:
            conn.sendall(frame(obj))
    log = lambda *a: print("[mock]", *a, flush=True)
    pending_policy = {}
    while True:
        m = read_msg(conn)
        if m is None:
            log("client closed"); break
        if "method" not in m:
            rid = m.get("id")
            if rid in pending_policy:
                log(f"policy reply {rid} after {time.monotonic()-pending_policy.pop(rid):.3f}s: {m.get('result', m.get('error'))}")
            else:
                log("response", m)
            continue
        meth, p, rid = m["method"], m.get("params", {}), m.get("id")
        if rid is None:
            log("notification", meth, p); continue
        if meth == "host.hello":
            send({"jsonrpc": "2.0", "id": rid, "result": {"protocol_version": 1}})
        elif meth == "window.create":
            send({"jsonrpc": "2.0", "id": rid, "result": {"window_id": "w-1"}})
        elif meth == "document.load":
            send({"jsonrpc": "2.0", "id": rid, "result": {"document_id": "d-1"}})
        elif meth == "test.burst":
            # Reply first, then a burst written as fast as the socket accepts.
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            n, kind = int(p.get("n", 1000)), p.get("type", "transitionrun")
            t0 = time.monotonic()
            for i in range(n):
                send({"jsonrpc": "2.0", "method": "document.event", "params": {"window_id": "w-1", "document_id": "d-1",
                      "event": {"type": kind, "element": f"e{i}", "path": [f"e{i}", "root"], "seq": i}}})
            if p.get("click"):
                send({"jsonrpc": "2.0", "method": "document.event", "params": {"window_id": "w-1", "document_id": "d-1",
                      "event": {"type": "click", "element": "btn", "path": ["btn"], "seq": n}}})
            if p.get("close"):
                send({"jsonrpc": "2.0", "method": "window.event", "params": {"window_id": "w-1", "type": "closed"}})
            log(f"burst of {n} written in {time.monotonic()-t0:.3f}s")
        elif meth == "test.before_close":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            pid = "host-bc-1"
            pending_policy[pid] = time.monotonic()
            send({"jsonrpc": "2.0", "id": pid, "method": "window.before_close", "params": {"window_id": "w-1", "reason": "user"}})
        elif meth == "test.slow":
            def later(rid=rid, d=float(p.get("delay", 1))):
                time.sleep(d); send({"jsonrpc": "2.0", "id": rid, "result": {"slow": True}})
            threading.Thread(target=later, daemon=True).start()
        elif meth == "window.list":
            send({"jsonrpc": "2.0", "id": rid, "result": [{"window_id": "w-1"}, {"window_id": "w-2"}]})
        elif meth == "test.array1":
            send({"jsonrpc": "2.0", "id": rid, "result": [{"a": 1}]})
        elif meth == "test.array0":
            send({"jsonrpc": "2.0", "id": rid, "result": []})
        elif meth.startswith("dialog."):
            log(f"dialog request {rid} opened at {time.monotonic():.1f} (never answered)")
        elif meth == "test.close_and_shutdown":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            send({"jsonrpc": "2.0", "method": "window.event", "params": {"window_id": "w-1", "type": "closed"}})
            send({"jsonrpc": "2.0", "method": "app.shutdown", "params": {"deadline_ms": 5000}})
        elif meth == "test.stall":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            log("stalling reads for 6s"); time.sleep(6); log("resuming reads")
        elif meth == "test.shutdown":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            send({"jsonrpc": "2.0", "method": "app.shutdown", "params": {"deadline_ms": 5000}})
        else:
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
    rc = proc.wait(timeout=60)
    log("client exit", rc)
    os.unlink(path)

main()
