"""A raw JSON-RPC backend for host lifecycle and headless-clock tests (F-12, F-25; T3, T15).

The host starts this script as its backend (`UI4ME_ENDPOINT`/`UI4ME_TOKEN` in the environment).
It speaks the wire protocol directly with the framing helpers of the reference client, on one
thread, so the order in which host messages arrive is observable exactly. It runs one scenario
and writes a JSON report to the path given as the first argument.

    python headless_backend.py REPORT SCENARIO [ARGS...]
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
from ui4me_core.client import connect_stream, encode_frame, read_frame, write_all  # noqa: E402


class HostError(Exception):
    def __init__(self, error):
        super().__init__(f"{error.get('code')}: {error.get('message')}")
        self.code = error.get("code")


class Peer:
    """One authenticated connection; every host message is recorded in arrival order."""

    def __init__(self, stream, policy=None):
        self.stream = stream
        self.next_id = 0
        self.log = []          # every notification and host request, in arrival order
        self.responses = {}
        self.policy = policy or (lambda method, params: {"allow": True})

    def send(self, message):
        write_all(self.stream, encode_frame(message))

    def request(self, method, params=None):
        self.next_id += 1
        ident = f"t{self.next_id}"
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params or {}})
        return ident

    def notify(self, method, params=None):
        self.send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def read_one(self):
        message = read_frame(self.stream)
        if message is None:
            raise EOFError("host closed the connection")
        if "method" in message:
            self.log.append(message)
            if "id" in message:     # a host -> backend policy request
                answer = self.policy(message["method"], message.get("params", {}))
                if isinstance(answer, dict) and "error" in answer:
                    self.send({"jsonrpc": "2.0", "id": message["id"], "error": answer["error"]})
                elif answer is not None:
                    self.send({"jsonrpc": "2.0", "id": message["id"], "result": answer})
        else:
            self.responses[message["id"]] = message
            self.log.append({"response": message["id"]})
        return message

    def wait(self, ident):
        while ident not in self.responses:
            self.read_one()
        message = self.responses.pop(ident)
        if "error" in message:
            raise HostError(message["error"])
        return message["result"]

    def call(self, method, params=None):
        return self.wait(self.request(method, params))

    def wait_for(self, predicate, timeout=30.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for message in self.log:
                if predicate(message):
                    return message
            self.read_one()
        raise TimeoutError("expected host message did not arrive")

    def events(self, start=0):
        return [m["params"]["event"] for m in self.log[start:]
                if m.get("method") == "document.event"]


def connect(policy=None):
    stream = connect_stream(os.environ["UI4ME_ENDPOINT"])
    peer = Peer(stream, policy)
    peer.call("host.hello", {"token": os.environ["UI4ME_TOKEN"], "protocol_version": 1,
                             "client": {"name": "headless_backend"}})
    return peer


def open_document(peer, html, width=800, height=600):
    window = peer.call("window.create", {"title": "test", "width": width, "height": height})
    document = peer.call("document.load", {"window_id": window["window_id"], "html": html})
    return window["window_id"], document["document_id"]


def command(peer, window, document, op, **args):
    return peer.call("document.command", {"window_id": window, "document_id": document,
                                          "command": {"op": op, **args}})


def rss_kib(pid):
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except OSError:
        pass
    return None


# burst2.py's style sheet, with `outline-color` instead of the `border-color` shorthand so each
# control transitions exactly three longhands (the shorthand expands to four border colours).
BURST_CSS = """
button { transition: background-color 120ms, outline-color 120ms, color 120ms;
         background: #eee; outline: 1px solid #999; color: #111 }
@media (prefers-color-scheme: dark) { button { background: #333; outline-color: #777; color: #eee } }
"""


def scenario_burst(peer, report, count="100", rounds="6"):
    """T3: the headless version of burst2.py on the virtual clock."""
    count, rounds = int(count), int(rounds)
    html = "<style>%s</style><div id=root>%s</div><p id=s>x</p>" % (
        BURST_CSS, "".join(f'<button id="b{i}">B{i}</button>' for i in range(count)))
    window, document = open_document(peer, html)
    peer.call("app.ready")
    report["capabilities_test_clock"] = peer.call("host.capabilities").get("test_clock")
    # Settle the initial style on the virtual clock; the first call switches it on.
    peer.call("test.advance_time", {"ms": 0})
    report["rounds"] = []
    for index in range(rounds):
        scheme = "dark" if index % 2 == 0 else "light"
        start = len(peer.log)
        command(peer, window, document, "set_environment", color_scheme=scheme)
        advanced = peer.call("test.advance_time", {"ms": 200})
        # Everything the interval produced arrived before the advance_time answer.
        events = [e for e in peer.events(start) if e.get("type", "").startswith("transition")]
        text = command(peer, window, document, "get_text", element="s")
        report["rounds"].append({
            "scheme": scheme, "advanced": advanced, "text": text,
            "events": [[e.get("type"), e.get("element"), e.get("property_name")]
                       for e in events],
        })
    return window


def scenario_soak(peer, report, count="10000"):
    """RENDER-04: many DOM writes without any read; host RSS is sampled along the way."""
    count = int(count)
    window, document = open_document(peer, "<p id=t>start</p>")
    peer.call("app.ready")
    host_pid = peer.call("host.diagnostics")["pid"]
    samples = []
    pending = []
    for index in range(count):
        pending.append(peer.request("document.command", {
            "window_id": window, "document_id": document,
            "command": {"op": "set_text", "element": "t", "text": f"value {index}"}}))
        if len(pending) >= 64:
            for ident in pending:
                peer.wait(ident)
            pending = []
        if (index + 1) % 1000 == 0:
            for ident in pending:
                peer.wait(ident)
            pending = []
            time.sleep(0.05)   # let the host pump run its frames (as a display would)
            samples.append([index + 1, rss_kib(host_pid)])
    report["rss_kib"] = samples
    report["final_text"] = command(peer, window, document, "get_text", element="t")
    return window


def scenario_drain(peer, report, commands="1000", rows="200"):
    """Gate R5(c): pipeline COMMANDS document commands at once (alternating a write and a
    geometry read, which forces an update each) and report the host's longest GUI-thread turn
    while they drain."""
    commands, rows = int(commands), int(rows)
    html = "<ul>%s</ul>" % "".join(f"<li id=r{i}>row {i}</li>" for i in range(rows))
    window, document = open_document(peer, html)
    peer.call("app.ready")
    peer.call("host.diagnostics")      # reset the turn statistics
    started = time.monotonic()
    pending = []
    for index in range(commands):
        row = f"r{(index // 2) % rows}"
        command = ({"op": "set_text", "element": row, "text": f"row {index}"} if index % 2 == 0
                   else {"op": "get_rect", "element": row})
        pending.append(peer.request("document.command", {
            "window_id": window, "document_id": document, "command": command}))
    for ident in pending:
        peer.wait(ident)
    report["drain_seconds"] = time.monotonic() - started
    report["pump"] = peer.call("host.diagnostics").get("pump")
    return window


def scenario_submit(peer, report, fields="1000", mode="submit"):
    """HOST-06: one request_submit on a form of FIELDS empty required inputs; time the command
    and count the `invalid` events it produces."""
    fields = int(fields)
    wrap = ("<div>", "</div>") if mode == "blocks" else ("", "")
    html = "<form id=f>%s</form>" % "".join(f"{wrap[0]}<input id=i{i} required>{wrap[1]}"
                                             for i in range(fields))
    started = time.monotonic()
    window, document = open_document(peer, html)
    peer.call("app.ready")
    command(peer, window, document, "get_rect", element="f")    # settle the first layout
    report["load_seconds"] = time.monotonic() - started
    report["load_pump"] = peer.call("host.diagnostics").get("pump")
    start = len(peer.log)
    started = time.monotonic()
    if mode in ("submit", "blocks"):
        command(peer, window, document, "request_submit", element="f")
    report["submit_seconds"] = time.monotonic() - started
    command(peer, window, document, "get_rect", element="f")    # the update after it
    report["update_seconds"] = time.monotonic() - started
    report["invalid_events"] = sum(1 for e in peer.events(start) if e.get("type") == "invalid")
    report["pump"] = peer.call("host.diagnostics").get("pump")
    return window


def scenario_wait(peer, report, *options):
    """Ready with one window, announce it (`REPORT.ready`), then serve the host until it sends
    `app.shutdown`. Options: `quit-soon` requests the quit 0.5 s after ready, `close-soon` requests
    closing the window right after ready, `linger` waits 1.5 s
    after `app.shutdown`, `decline-quit` answers app.before_quit with allow=false,
    `busy-close`/`error-close` answer window.before_close with -32005/-32603 the first N=2
    times."""
    answers = {"before_close": 0}

    def policy(method, params):
        if method == "app.before_quit" and "decline-quit" in options:
            return {"allow": False}
        if method == "window.before_close":
            answers["before_close"] += 1
            if answers["before_close"] <= 2 and "busy-close" in options:
                return {"error": {"code": -32005, "message": "Callback queue is full"}}
            if "error-close" in options:
                return {"error": {"code": -32603, "message": "Callback failed: ValueError"}}
        return {"allow": True}

    peer.policy = policy
    window = peer.call("window.create", {"title": "wait", "width": 320, "height": 200})
    peer.call("app.ready")
    diagnostics = peer.call("host.diagnostics")
    ready = Path(str(report["_path"]) + ".ready")
    ready.write_text(json.dumps({"endpoint": os.environ["UI4ME_ENDPOINT"], "pid": diagnostics["pid"],
                                 "window_id": window["window_id"]}), encoding="utf-8")
    if "quit-soon" in options:
        time.sleep(0.5)
        peer.call("app.request_quit")
    if "close-soon" in options:
        peer.call("window.request_close", {"window_id": window["window_id"]})
    timeout = 15 if "close-soon" in options else 120
    shutdown = peer.wait_for(lambda m: m.get("method") == "app.shutdown", timeout=timeout)
    report["shutdown"] = shutdown["params"]
    if "linger" in options:
        time.sleep(1.5)     # a backend that takes a while to save and exit
    report["log"] = [m.get("method") or m for m in peer.log]
    report["policy_requests"] = [[m["method"], m.get("params")] for m in peer.log
                                 if m.get("method") in ("window.before_close", "app.before_quit")]
    report["activated"] = [m["params"] for m in peer.log if m.get("method") == "app.activated"]
    report["ok"] = True
    return None


SCENARIOS = {"burst": scenario_burst, "soak": scenario_soak, "wait": scenario_wait,
             "drain": scenario_drain, "submit": scenario_submit}


def main():
    report_path = Path(sys.argv[1])
    name, args = sys.argv[2], sys.argv[3:]
    report = {"scenario": name, "ok": False, "_path": str(report_path)}
    try:
        peer = connect()
        if SCENARIOS[name](peer, report, *args) is not None:
            peer.call("app.request_quit")
            peer.wait_for(lambda m: m.get("method") == "app.shutdown")
            report["ok"] = True
    except Exception as error:  # the report carries the failure to the test
        report["error"] = f"{type(error).__name__}: {error}"
    report.pop("_path")
    report_path.write_text(json.dumps(report), encoding="utf-8")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
