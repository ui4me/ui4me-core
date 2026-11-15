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
                raise AssertionError(f"backend never became ready:\n{self.host_log()}")
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

        def low_limit():
            resource.setrlimit(resource.RLIMIT_NOFILE, (19, 19))

        host = WaitingHost(self, app_id="org.ui4me.test.AcceptEmfile", preexec_fn=low_limit)
        endpoint = host.ready["endpoint"]
        flood = []
        for _ in range(40):
            connection = socket.socket(socket.AF_UNIX)
            try:
                connection.connect(endpoint)
            except OSError:
                connection.close()
                break
            flood.append(connection)
        time.sleep(1.0)
        for connection in flood:
            connection.close()
        time.sleep(1.5)
        self.assertIsNone(host.process.poll(), host.host_log())
        self.assertTrue(os.path.exists(endpoint), "the endpoint must still exist")
        answer = raw_request(endpoint, "host.diagnostics")
        self.assertEqual(answer["error"]["code"], -32011, answer)   # reached the live host
        self.assertIn("transient IPC accept error", host.host_log())
        host.process.send_signal(signal.SIGKILL)


if __name__ == "__main__":
    unittest.main()
