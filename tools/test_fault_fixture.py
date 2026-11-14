"""Verify that fault cleanup identifies only its own real worker processes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_faults import ROOT, checkout_environment, process_identity, terminate_owned


class ProcessIdentityTests(unittest.TestCase):
    def test_creation_stamp_guards_real_tree_cleanup(self):
        self.assertFalse(terminate_owned(process_identity()))
        run_id = uuid.uuid4().hex
        identities = []
        process = None
        with tempfile.TemporaryDirectory(prefix="ui4me-owned-tree-") as folder:
            output = Path(folder)
            try:
                process = subprocess.Popen([sys.executable, str(ROOT / "examples/python/faults.py"),
                    "--tree-worker", "--run-id", run_id, "--evidence", str(output / "events.jsonl")],
                    env=checkout_environment())
                deadline = time.monotonic() + 5
                while not all((output / f"tree-{kind}.json").is_file() for kind in ("worker", "leaf")):
                    if time.monotonic() > deadline:
                        self.fail("Test workers did not publish their identities")
                    time.sleep(0.02)
                worker, leaf = [json.loads((output / f"tree-{kind}.json").read_text(encoding="utf-8"))
                                for kind in ("worker", "leaf")]
                self.assertEqual(worker["run_id"], run_id)
                self.assertEqual(worker["pid"], process.pid)
                self.assertEqual(worker["child_pid"], leaf["pid"])
                identities = [{key: row[key] for key in ("pid", "creation_stamp")} for row in (worker, leaf)]
                for identity in identities:
                    self.assertEqual(process_identity(identity["pid"]), identity)
                wrong = {**identities[0], "creation_stamp": "not-the-process-creation-time"}
                self.assertFalse(terminate_owned(wrong))
                self.assertIsNone(process.poll())
            finally:
                # Also recover identities if an assertion failed before the list
                # was assigned, so a broken test does not leave a worker running.
                for kind in ("worker", "leaf"):
                    path = output / f"tree-{kind}.json"
                    if path.is_file():
                        row = json.loads(path.read_text(encoding="utf-8"))
                        identity = {key: row[key] for key in ("pid", "creation_stamp")}
                        terminate_owned(identity)
                if process is not None:
                    if process.poll() is None:
                        process.kill()
                    process.wait(timeout=5)
            deadline = time.monotonic() + 3
            while any(process_identity(row["pid"]) == row for row in identities) and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(all(process_identity(row["pid"]) != row for row in identities))


if __name__ == "__main__":
    unittest.main(verbosity=2)
