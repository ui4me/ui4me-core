"""T15 (F-18, F-25): host lifecycle edge cases against the real host.

Headless test mode, real IPC, the raw-protocol backend `headless_backend.py`. Needs a built host
(`cargo build -p ui4me-host`); skipped without one. `UI4ME_HOST_BINARY` selects a specific host.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from test_headless_clock import CLEAN, built_host  # noqa: E402
from run_example import checkout_environment, manifest  # noqa: E402

BACKEND = ROOT / "tools" / "headless_backend.py"


class WaitingHost:
    """A headless host whose backend (`headless_backend.py wait`) is ready and serving."""

    def __init__(self, test, *options, app_id, runtime=None, preexec_fn=None, extra_env=None,
                 manifest_extra=""):
        folder = tempfile.TemporaryDirectory(prefix="ui4me-lifecycle-")
        test.addCleanup(folder.cleanup)
        self.base = Path(folder.name)
        self.report_path = self.base / "report.json"
        self.app = self.base / "app.toml"
        self.app.write_text(manifest([str(self.report_path), "wait", *options], app_id=app_id,
                                     backend_path=BACKEND) + manifest_extra, encoding="utf-8")
        self.env = checkout_environment(CLEAN)
        self.env["UI4ME_TEST_MODE"] = "1"
        if os.name != "nt":
            if runtime is None:
                runtime = self.base / "runtime"
                runtime.mkdir(mode=0o700)
            self.env["XDG_RUNTIME_DIR"] = str(runtime)
        self.env.update(extra_env or {})
        self.log = open(self.base / "host.log", "wb")
        test.addCleanup(self.log.close)
        self.process = subprocess.Popen([built_host(), "run", "--app", str(self.app), "--headless"],
                                        env=self.env, stdout=self.log, stderr=subprocess.STDOUT,
                                        preexec_fn=preexec_fn)
        test.addCleanup(self.kill)
        ready = Path(str(self.report_path) + ".ready")
        deadline = time.monotonic() + 60
        while not ready.is_file():
            if self.process.poll() is not None or time.monotonic() > deadline:
                raise AssertionError(f"backend never became ready (exit {self.process.poll()}):\n{self.host_log()}")
            time.sleep(0.02)
        time.sleep(0.05)
        self.ready = json.loads(ready.read_text(encoding="utf-8"))

    def kill(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(10)

    def wait(self, timeout=30):
        return self.process.wait(timeout)

    def host_log(self):
        self.log.flush()
        return (self.base / "host.log").read_text(encoding="utf-8", errors="replace")

    def report(self):
        return json.loads(self.report_path.read_text(encoding="utf-8"))


def raw_request(endpoint, method, params=None, timeout=5.0):
    """One unauthenticated request on a fresh connection; returns the decoded answer."""
    sys.path.insert(0, str(ROOT / "python"))
    from ui4me_core.client import connect_stream, encode_frame, read_frame, write_all
    stream = connect_stream(endpoint, timeout=timeout)
    try:
        write_all(stream, encode_frame({"jsonrpc": "2.0", "id": "raw", "method": method,
                                        "params": params or {}}))
        return read_frame(stream)
    finally:
        stream.close()


def systemd_user_manager() -> bool:
    if not sys.platform.startswith("linux") or not shutil.which("systemd-run"):
        return False
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    return bool(runtime) and Path(runtime, "bus").exists()


@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
class SupervisedStartTests(unittest.TestCase):
    @unittest.skipUnless(systemd_user_manager(), "needs a systemd user manager")
    def test_t15_supervised_start_with_rejected_environment(self):
        """F-18: `export -f` / Lmod entries and non-UTF-8 values no longer abort the start."""
        env = {os.fsencode(key): os.fsencode(value) for key, value in CLEAN.items()}
        env[b"BASH_FUNC_ui4me_test%%"] = b"() {  echo t15\n}"
        env[b"UI4ME_T15_LATIN"] = b"caf\xe9"
        result = subprocess.run([built_host(), "run", "--app", "/nonexistent/ui4me-t15/app.toml"],
                                env=env, capture_output=True, timeout=60, check=False)
        error = result.stderr.decode(errors="replace")
        self.assertNotEqual(result.returncode, 0)
        # The supervised host started and got as far as reading the (missing) manifest.
        self.assertIn("Cannot locate application manifest", error)
        self.assertNotIn("Cannot assign environment variable", error)
        self.assertNotIn("Failed to create bus message", error)



@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
class AcceptLoopTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform.startswith("linux"), "RLIMIT_NOFILE exhaustion is Linux")
    def test_t15_listener_survives_descriptor_exhaustion(self):
        """HOST-04: EMFILE on accept is retried instead of ending the listener for good."""
        import resource
        host = WaitingHost(self, app_id="org.ui4me.test.AcceptEmfile")
        endpoint, pid = host.ready["endpoint"], host.process.pid
        original = resource.prlimit(pid, resource.RLIMIT_NOFILE)
        # No descriptor left: the next accept(2) fails with EMFILE.
        in_use = len(os.listdir(f"/proc/{pid}/fd"))
        resource.prlimit(pid, resource.RLIMIT_NOFILE, (in_use, original[1]))
        waiting = socket.socket(socket.AF_UNIX)
        waiting.connect(endpoint)          # queued in the backlog; accept keeps failing
        time.sleep(1.0)
        resource.prlimit(pid, resource.RLIMIT_NOFILE, original)
        time.sleep(0.5)
        waiting.close()
        self.assertIsNone(host.process.poll(), host.host_log())
        self.assertTrue(os.path.exists(endpoint), "the endpoint must still exist")
        answer = raw_request(endpoint, "host.diagnostics")
        self.assertEqual(answer["error"]["code"], -32011, answer)   # reached the live host
        self.assertIn("transient IPC accept error", host.host_log())
        host.process.send_signal(signal.SIGKILL)



@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
@unittest.skipUnless(os.name == "posix", "POSIX signals; Windows console/session end: see the report")
class TerminationSignalTests(unittest.TestCase):
    def check_signal(self, signum):
        host = WaitingHost(self, app_id=f"org.ui4me.test.Signal{int(signum)}")
        host.process.send_signal(signum)
        code = host.wait(15)
        report = host.report()
        self.assertTrue(report.get("ok"), (report, host.host_log()))
        # HOST-07: the backend got an orderly shutdown with the short deadline ...
        self.assertLessEqual(report["shutdown"]["deadline_ms"], 3000)
        # ... and the host exits 128 + signal once the backend has exited.
        self.assertEqual(code, 128 + int(signum), host.host_log())
        self.assertFalse(os.path.exists(host.ready["endpoint"]), "endpoint removed")
        self.assertIn("termination requested", host.host_log())

    def test_t15_sigterm_delivers_app_shutdown(self):
        self.check_signal(signal.SIGTERM)

    def test_t15_sigint_delivers_app_shutdown(self):
        self.check_signal(signal.SIGINT)

    def test_t15_sighup_delivers_app_shutdown(self):
        self.check_signal(signal.SIGHUP)



@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
@unittest.skipUnless(os.name == "posix", "uses SIGTERM to end the primary")
class RelaunchTests(unittest.TestCase):
    def test_t15_relaunch_during_primary_shutdown_becomes_the_primary(self):
        """HOST-10: a second launch while the primary shuts down waits and takes over."""
        app_id = "org.ui4me.test.Relaunch"
        # The primary's backend quits 0.5 s after ready and then takes 1.5 s to exit.
        first = WaitingHost(self, "quit-soon", "linger", app_id=app_id)
        runtime = Path(first.env["XDG_RUNTIME_DIR"])
        time.sleep(0.8)
        self.assertIsNone(first.process.poll(), "the primary is still shutting down")
        started = time.monotonic()
        second = WaitingHost(self, app_id=app_id, runtime=runtime)   # raises if it exits early
        self.assertEqual(first.wait(10), 0, first.host_log())
        self.assertGreater(time.monotonic() - started, 0.8, "the relaunch waited for the lock")
        self.assertIn("waiting to take over", second.host_log())
        self.assertEqual(second.ready["endpoint"], first.ready["endpoint"], "same single instance")
        second.process.send_signal(signal.SIGTERM)
        self.assertEqual(second.wait(10), 143, second.host_log())
        self.assertTrue(second.report()["ok"])



@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
class PolicyAnswerTests(unittest.TestCase):
    def test_t15_busy_close_answer_is_asked_again_not_a_veto(self):
        """HOST-15: "Callback queue is full" (-32005) no longer silently vetoes the close."""
        host = WaitingHost(self, "close-soon", "busy-close", app_id="org.ui4me.test.BusyClose")
        self.assertEqual(host.wait(30), 0, host.host_log())
        report = host.report()
        self.assertTrue(report.get("ok"), (report, host.host_log()))
        requests = report["policy_requests"]
        closes = [params for method, params in requests if method == "window.before_close"]
        # Two busy answers, then the allow on the third request; the last window then quits.
        self.assertEqual(len(closes), 3, requests)
        self.assertEqual(requests[-1][0], "app.before_quit", requests)
        self.assertIn("asking the close/quit decision again", host.host_log())


if __name__ == "__main__":
    unittest.main()
