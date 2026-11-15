#!/usr/bin/env python3
"""Adversarial UI4ME backend. Launched by ui4me-host (headless) from a manifest.

Each scenario writes JSON lines to the evidence file given as argv[2].
Never prints the token.
"""
import json
import os
import socket
import struct
import sys
import threading
import time

SCENARIO = sys.argv[1]
EVIDENCE = sys.argv[2]
ENDPOINT = os.environ.pop("UI4ME_ENDPOINT")
TOKEN = os.environ.pop("UI4ME_TOKEN")
T0 = time.monotonic()
READ_DELAY = float(os.environ.get("ADV_READ_DELAY", "0"))
_lock = threading.Lock()


def log(event, **fields):
    fields.update(event=event, t=round(time.monotonic() - T0, 3))
    with _lock, open(EVIDENCE, "a", encoding="utf-8") as f:
        f.write(json.dumps(fields) + "\n")


def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(ENDPOINT)
    return s


def frame(obj):
    body = json.dumps(obj, separators=(",", ":")).encode()
    return struct.pack(">I", len(body)) + body


def send(s, obj):
    s.sendall(frame(obj))


def recv_exact(s, n):
    data = b""
    while len(data) < n:
        chunk = s.recv(n - len(data))
        if not chunk:
            return None
        data += chunk
    return data


def recv(s, timeout=5.0):
    s.settimeout(timeout)
    try:
        header = recv_exact(s, 4)
        if header is None:
            return "EOF"
        (length,) = struct.unpack(">I", header)
        body = recv_exact(s, length)
        if body is None:
            return "EOF"
        return json.loads(body)
    except socket.timeout:
        return "TIMEOUT"
    except (ConnectionResetError, BrokenPipeError) as e:
        return "RESET:" + type(e).__name__


def req(method, params=None, id_=None):
    m = {"jsonrpc": "2.0", "method": method, "params": params or {}}
    if id_ is not None:
        m["id"] = id_
    return m


class Client:
    """Simple synchronous client; host->backend requests are auto-answered."""

    def __init__(self, s, allow=True):
        self.s = s
        self.n = 0
        self.notifications = []
        self.allow = allow
        self.host_requests = []

    def call(self, method, params=None, timeout=5.0):
        self.n += 1
        rid = f"c{self.n}"
        send(self.s, req(method, params, rid))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            m = recv(self.s, max(0.01, deadline - time.monotonic()))
            if isinstance(m, str):
                return {"transport": m}
            if "method" in m:
                if "id" in m:
                    self.host_requests.append(m)
                    allow = self.allow(m) if callable(self.allow) else self.allow
                    if allow is not None:
                        send(self.s, {"jsonrpc": "2.0", "id": m["id"], "result": {"allow": allow}})
                else:
                    self.notifications.append(m)
                continue
            if m.get("id") == rid:
                return m
        return {"transport": "TIMEOUT"}

    def drain(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            m = recv(self.s, max(0.01, end - time.monotonic()))
            if isinstance(m, str):
                if m == "TIMEOUT":
                    continue
                return m
            if "method" in m:
                if "id" in m:
                    self.host_requests.append(m)
                    allow = self.allow(m) if callable(self.allow) else self.allow
                    if allow is not None:
                        send(self.s, {"jsonrpc": "2.0", "id": m["id"], "result": {"allow": allow}})
                else:
                    self.notifications.append(m)
        return None


def hello(c):
    return c.call("host.hello", {"token": TOKEN, "protocol_version": 1})


def summarize(m):
    if isinstance(m, dict):
        if "error" in m:
            return {"error": m["error"].get("code"), "message": m["error"].get("message")}
        if "result" in m:
            r = json.dumps(m["result"])
            return {"result": r[:160]}
        return m
    return m


def wait_closed(s, limit):
    """Seconds until the host closes the connection, or None."""
    start = time.monotonic()
    s.settimeout(limit)
    try:
        while True:
            d = s.recv(65536)
            if not d:
                return round(time.monotonic() - start, 2)
    except socket.timeout:
        return None
    except OSError as e:
        return f"{round(time.monotonic() - start, 2)} ({type(e).__name__})"


def ready(c):
    w = c.call("window.create", {"title": "adv", "width": 400, "height": 300})
    wid = w.get("result", {}).get("window_id")
    r = c.call("app.ready")
    log("ready", window=wid, ready=summarize(r))
    return wid


# --------------------------------------------------------------------------

def scenario_auth():
    s = connect()
    c = Client(s)
    log("prehello_window_create", r=summarize(c.call("window.create", {})))
    log("prehello_app_ready", r=summarize(c.call("app.ready")))
    log("prehello_diag", r=summarize(c.call("host.diagnostics")))
    log("hello_bad_version", r=summarize(c.call("host.hello", {"token": TOKEN, "protocol_version": 2})))
    log("hello_wrong_token", r=summarize(c.call("host.hello", {"token": "0" * 64, "protocol_version": 1})))
    log("hello_short_token", r=summarize(c.call("host.hello", {"token": TOKEN[:10], "protocol_version": 1})))
    log("hello_nontoken", r=summarize(c.call("host.hello", {"token": 5, "protocol_version": 1})))
    log("hello_extra", r=summarize(c.call("host.hello", {"token": TOKEN, "protocol_version": 1, "x": 1})))
    # Second connection authenticating first while the first is unauthenticated.
    s2 = connect()
    c2 = Client(s2)
    log("second_activate_prestart", r=summarize(c2.call("app.activate", {"args": ["x"]})))
    log("hello_ok", r=summarize(hello(c)))
    log("hello_again_same_conn", r=summarize(hello(c)))
    log("hello_second_conn", r=summarize(hello(c2)))
    wid = ready(c)
    log("second_activate_running", r=summarize(c2.call("app.activate", {"args": ["x"], "cwd": "/"})))
    log("second_activate_big", r=summarize(c2.call("app.activate", {"args": ["x" * 40000]})))
    log("second_window_create", r=summarize(c2.call("window.create", {})))
    c.drain(0.3)
    log("client_notifications", methods=[n["method"] for n in c.notifications])
    # Unauthenticated idle connection: when does the host close it?
    s3 = connect()
    log("idle_unauth_closed_after", seconds=wait_closed(s3, 20))
    log("second_conn_closed_after", seconds=wait_closed(s2, 20))
    # Admission: open 12 connections at once.
    socks = [connect() for _ in range(12)]
    time.sleep(0.5)
    closed = 0
    for x in socks:
        x.settimeout(0.05)
        try:
            if x.recv(1) == b"":
                closed += 1
        except socket.timeout:
            pass
        except OSError:
            closed += 1
    log("admission_12_immediately_closed", count=closed)
    for x in socks:
        x.close()
    # Unauthenticated app.activate flood from secondary connections (no token).
    c.notifications.clear()
    t = time.monotonic()
    accepted = 0
    for i in range(200):
        x = connect()
        r = Client(x).call("app.activate", {"args": [str(i)]}, timeout=2)
        if r.get("result", {}).get("accepted"):
            accepted += 1
        x.close()
    c.drain(0.5)
    log("unauth_activate_flood", accepted=accepted, seconds=round(time.monotonic() - t, 2),
        delivered=sum(1 for n in c.notifications if n["method"] == "app.activated"))
    log("diag_after", r=summarize(c.call("host.diagnostics")))
    c.call("app.request_quit")
    c.drain(2)


def scenario_malformed_secondary():
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    cases = {
        "zero_length": struct.pack(">I", 0),
        "oversize_header": struct.pack(">I", 2 * 1024 * 1024 + 1),
        "max_u32_header": struct.pack(">I", 0xFFFFFFFF),
        "invalid_utf8": struct.pack(">I", 2) + b"\xff\xfe",
        "invalid_json": struct.pack(">I", 1) + b"{",
        "truncated_header": b"\x00\x00",
        "truncated_body": struct.pack(">I", 100) + b"{}",
        "batch_array": frame([req("host.diagnostics", id_="a")]),
        "deep_nesting": (lambda b: struct.pack(">I", len(b)) + b)(b"[" * 200 + b"]" * 200),
        "deep_nesting_obj": (lambda b: struct.pack(">I", len(b)) + b)(
            (b'{"a":' * 100000 + b"1" + b"}" * 100000)),
        "numeric_id": frame({"jsonrpc": "2.0", "id": 1, "method": "host.diagnostics"}),
        "nul_method": frame({"jsonrpc": "2.0", "id": "x", "method": "a\u0000b"}),
    }
    for name, data in cases.items():
        x = connect()
        try:
            x.sendall(data)
            if name.startswith("truncated"):
                x.shutdown(socket.SHUT_WR)
        except OSError as e:
            log("send_failed", case=name, error=type(e).__name__)
        reply = recv(x, 3)
        closed = wait_closed(x, 8) if reply != "EOF" else 0
        healthy = summarize(c.call("host.diagnostics"))
        log("malformed_secondary", case=name, reply=summarize(reply) if not isinstance(reply, str) else reply,
            closed_after=closed, client_healthy="result" in healthy)
        x.close()
    # A 2 MiB frame exactly is legal.
    big = {"jsonrpc": "2.0", "id": "big", "method": "host.diagnostics", "params": {}}
    body = json.dumps(big).encode()
    pad = 2 * 1024 * 1024 - len(body)
    big["params"] = {"x": "a" * (pad - len(',"params":{"x":""}') + len(',"params":{}'))}
    log("exact_2mib", size=len(json.dumps(big, separators=(",", ":")).encode()))
    log("exact_2mib_reply", r=summarize(c.call("host.diagnostics", {"x": "a" * (2 * 1024 * 1024 - 200)})))
    c.call("app.request_quit")
    c.drain(2)


def scenario_malformed_client():
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    log("sending_invalid_json_on_client")
    s.sendall(struct.pack(">I", 1) + b"{")
    log("client_closed_after", seconds=wait_closed(s, 10))


def scenario_invalid_response():
    """Odd responses on the authenticated connection."""
    s = connect()
    c = Client(s, allow=None)  # do not auto-answer
    hello(c)
    wid = ready(c)
    w2 = c.call("window.create", {"title": "two"})["result"]["window_id"]
    log("unknown_id_response")
    send(s, {"jsonrpc": "2.0", "id": "host:decision:999", "result": {"allow": True}})
    send(s, {"jsonrpc": "2.0", "id": "nope", "result": 1, "error": {"code": 1, "message": "x"}})
    send(s, {"jsonrpc": "2.0"})
    log("diag", r=summarize(c.call("host.diagnostics")))
    # Close request -> before_close; answer with error -> treated as deny
    c.call("window.request_close", {"window_id": w2})
    c.drain(0.3)
    bc = [m for m in c.host_requests if m["method"] == "window.before_close"]
    log("before_close_received", n=len(bc))
    if bc:
        send(s, {"jsonrpc": "2.0", "id": bc[-1]["id"], "error": {"code": -32603, "message": "boom"}})
    log("windows_after_error_answer", r=summarize(c.call("window.list")))
    # reentrancy: close again, then quit supersedes it, then answer the stale close with allow
    c.host_requests.clear()
    c.call("window.request_close", {"window_id": w2})
    c.call("window.request_close", {"window_id": w2})  # coalesced
    c.call("app.request_quit")
    c.drain(0.3)
    names = [m["method"] for m in c.host_requests]
    log("host_requests", names=names, ids=[m["id"] for m in c.host_requests],
        notes=[n for n in c.notifications if n["method"] == "request.cancelled"])
    close = next((m for m in c.host_requests if m["method"] == "window.before_close"), None)
    quit_ = next((m for m in c.host_requests if m["method"] == "app.before_quit"), None)
    if close:
        send(s, {"jsonrpc": "2.0", "id": close["id"], "result": {"allow": True}})
    log("window_create_during_quit_pending", r=summarize(c.call("window.create", {})))
    if quit_:
        send(s, {"jsonrpc": "2.0", "id": quit_["id"], "result": {"allow": False}})
    log("windows_after_stale_and_veto", r=summarize(c.call("window.list")))
    log("phase", r=summarize(c.call("host.diagnostics")))
    # Now a real quit: allow and then keep issuing requests
    c.host_requests.clear()
    c.call("app.request_quit")
    c.drain(0.3)
    quit_ = next((m for m in c.host_requests if m["method"] == "app.before_quit"), None)
    send(s, {"jsonrpc": "2.0", "id": quit_["id"], "result": {"allow": True}})
    c.drain(0.2)
    log("after_allow_notifications", methods=[n["method"] for n in c.notifications][-5:])
    log("window_create_after_quit", r=summarize(c.call("window.create", {})))
    log("diag_after_quit", r=summarize(c.call("host.diagnostics")))
    log("force_quit_after_quit", r=summarize(c.call("app.force_quit")))
    log("closed_after", seconds=wait_closed(s, 10))


def scenario_flood_no_read():
    """Pipelined requests without reading responses."""
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    n = int(os.environ.get("ADV_N", "20000"))
    t = time.monotonic()
    sent = 0
    s.settimeout(30)
    try:
        for i in range(n):
            s.sendall(frame(req("host.diagnostics", id_=f"f{i}")))
            sent += 1
    except OSError as e:
        log("flood_send_error", error=type(e).__name__, sent=sent)
    log("flood_sent", sent=sent, seconds=round(time.monotonic() - t, 2))
    time.sleep(1)
    got = 0
    while True:
        m = recv(s, 3)
        if isinstance(m, str):
            log("flood_read_end", reason=m, responses=got)
            break
        got += 1


def scenario_slow_reader_burst():
    """Issue #1 reproduction: a burst of host notifications while reading slowly."""
    s = connect()
    c = Client(s)
    hello(c)
    wid = ready(c)
    html = "<form id=f><input id=i required></form>"
    d = c.call("document.load", {"window_id": wid, "html": html})
    did = d["result"]["document_id"]
    burst = int(os.environ.get("ADV_N", "600"))
    delay = float(os.environ.get("ADV_DELAY", "0.005"))
    # Notifications (no id) so the host sends no responses: only document.event output.
    for i in range(burst):
        send(s, req("document.command", {"window_id": wid, "document_id": did,
                                          "command": {"op": "request_submit", "element": "f"}}))
    log("burst_sent", n=burst)
    got = 0
    types = {}
    while True:
        m = recv(s, 3)
        if isinstance(m, str):
            log("slow_reader_end", reason=m, received=got, types=types)
            break
        got += 1
        if m.get("method") == "document.event":
            k = m["params"]["event"].get("type")
            types[k] = types.get(k, 0) + 1
        time.sleep(delay)


def scenario_window_churn():
    s = connect()
    c = Client(s, allow=True)
    hello(c)
    wid = ready(c)
    n = int(os.environ.get("ADV_N", "500"))
    pid = c.call("host.diagnostics")["result"]["pid"]

    def rss():
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS"):
                    return int(line.split()[1])

    log("rss_start_kib", rss=rss())
    t = time.monotonic()
    for i in range(n):
        w = c.call("window.create", {"title": f"w{i}"})
        w2 = w["result"]["window_id"]
        c.call("document.load", {"window_id": w2, "html": f"<p id=p>hello {i}</p><b>bold</b><code>m</code>"})
        c.call("window.request_close", {"window_id": w2})
        c.drain(0.0)
        if i % 100 == 0:
            log("churn", i=i, rss=rss(), windows=len(c.call("window.list")["result"]))
    log("churn_done", seconds=round(time.monotonic() - t, 2), rss=rss(),
        windows=len(c.call("window.list")["result"]))
    # load generation churn on the main window + stale generation use
    first = c.call("document.load", {"window_id": wid, "html": "<p id=p>a</p>"})["result"]["document_id"]
    t = time.monotonic()
    last = first
    for i in range(n):
        last = c.call("document.load", {"window_id": wid, "html": f"<p id=p>gen {i}</p><b>x</b>"})["result"]["document_id"]
        if i % 100 == 0:
            log("load_churn", i=i, rss=rss())
    log("load_churn_done", seconds=round(time.monotonic() - t, 2), rss=rss())
    stale = c.call("document.command", {"window_id": wid, "document_id": first,
                                         "command": {"op": "get_text", "element": "p"}})
    cur = c.call("document.command", {"window_id": wid, "document_id": last,
                                       "command": {"op": "get_text", "element": "p"}})
    log("stale_generation", stale=summarize(stale), current=summarize(cur))
    c.call("app.request_quit")
    c.drain(2)


def scenario_no_hello():
    s = connect()
    log("connected_no_hello")
    log("closed_after", seconds=wait_closed(s, 40))


def scenario_shutdown_ignore():
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    c.call("app.request_quit")
    c.drain(0.5)
    log("notifications", methods=[n["method"] for n in c.notifications])
    log("closed_after", seconds=wait_closed(s, 20))
    time.sleep(30)


def scenario_quit_reentrancy():
    """Backend answers before_quit allow, but meanwhile spams close requests."""
    s = connect()
    c = Client(s, allow=None)
    hello(c)
    wid = ready(c)
    wids = [c.call("window.create", {"title": str(i)})["result"]["window_id"] for i in range(3)]
    for w in wids:
        c.call("window.request_close", {"window_id": w})
    c.drain(0.2)
    closes = [m for m in c.host_requests if m["method"] == "window.before_close"]
    log("closes", n=len(closes))
    # Allow all closes: the last allowed close of the final window turns into a quit request.
    for m in closes:
        send(s, {"jsonrpc": "2.0", "id": m["id"], "result": {"allow": True}})
    c.drain(0.2)
    log("windows", r=summarize(c.call("window.list")))
    c.host_requests.clear()
    c.call("window.request_close", {"window_id": wid})
    c.drain(0.2)
    log("final_close_requests", names=[m["method"] for m in c.host_requests])
    bc = next((m for m in c.host_requests if m["method"] == "window.before_close"), None)
    if bc:
        send(s, {"jsonrpc": "2.0", "id": bc["id"], "result": {"allow": True}})
    c.host_requests.clear()
    c.drain(0.3)
    log("after_final_allow", names=[m["method"] for m in c.host_requests],
        diag=summarize(c.call("host.diagnostics")))
    bq = next((m for m in c.host_requests if m["method"] == "app.before_quit"), None)
    if bq:
        send(s, {"jsonrpc": "2.0", "id": bq["id"], "result": {"allow": False}})
    c.drain(0.2)
    log("after_quit_veto", r=summarize(c.call("window.list")), diag=summarize(c.call("host.diagnostics")))
    c.call("app.force_quit")
    c.drain(1)




def scenario_fast_reader_single_command_burst():
    """One request (one GUI turn) emits N 'invalid' events; a dedicated fast reader."""
    s = connect()
    c = Client(s)
    hello(c)
    wid = ready(c)
    n = int(os.environ.get("ADV_N", "2000"))
    html = "<form id=f>" + "".join(f"<input id=i{i} required>" for i in range(n)) + "</form>"
    d = c.call("document.load", {"window_id": wid, "html": html}, timeout=60)
    did = d["result"]["document_id"]
    counts = {"events": 0, "types": {}, "end": None, "responses": []}

    def reader():
        while True:
            m = recv(s, 300)
            if isinstance(m, str):
                counts["end"] = m
                return
            if m.get("method") == "document.event":
                counts["events"] += 1
                counts.setdefault("size", len(json.dumps(m, separators=(",", ":"))))
                if READ_DELAY:
                    time.sleep(READ_DELAY)
                k = m["params"]["event"].get("type")
                counts["types"][k] = counts["types"].get(k, 0) + 1
            elif "id" in m and "method" not in m:
                counts["responses"].append(summarize(m))

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    t0 = time.monotonic()
    send(s, req("document.command", {"window_id": wid, "document_id": did,
                                      "command": {"op": "request_submit", "element": "f"}}, "burst"))
    while not counts["responses"] and counts["end"] is None and time.monotonic() - t0 < 300:
        time.sleep(0.01)
    log("burst_command_latency", seconds=round(time.monotonic() - t0, 3))
    t.join(5)
    log("fast_reader_result", n=n, size=counts.get("size"), events=counts["events"], types=counts["types"], end=counts["end"],
        responses=counts["responses"][:3])


def scenario_wait_notifications():
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    log("waiting")
    end = c.drain(30)
    log("drain_end", reason=end, methods=[n["method"] for n in c.notifications])


def scenario_endpoint_token_relation():
    base = os.path.basename(ENDPOINT).split(".")[0]
    log("endpoint_contains_token_prefix", equal=(base == TOKEN[:len(base)]), key_hex_len=len(base))
    s = connect()
    c = Client(s)
    hello(c)
    ready(c)
    c.call("app.force_quit")


# MAIN
SCENARIOS = {k[len("scenario_"):]: v for k, v in globals().items() if k.startswith("scenario_")}

if __name__ == "__main__":
    log("start", scenario=SCENARIO)
    try:
        SCENARIOS[SCENARIO]()
    except Exception as e:  # noqa
        log("exception", error=f"{type(e).__name__}: {e}")
    log("end")
