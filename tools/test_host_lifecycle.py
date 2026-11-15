"""T15 (F-18, F-25): host lifecycle edge cases against the real host.

Headless test mode, real IPC, the raw-protocol backend `headless_backend.py`. Needs a built host
(`cargo build -p ui4me-host`); skipped without one. `UI4ME_HOST_BINARY` selects a specific host.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from test_headless_clock import CLEAN, built_host  # noqa: E402


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


if __name__ == "__main__":
    unittest.main()
