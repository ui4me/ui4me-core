"""The diagnostic watchdog must report a hang even when Windows cleanup fails."""
import io
import subprocess
import unittest
from unittest.mock import Mock, patch

from windows import uia_probe_matrix


class ProbeWatchdogTests(unittest.TestCase):
    def test_failed_taskkill_still_kills_and_reaps_only_the_probe(self):
        for error in (OSError("taskkill missing"),
                      subprocess.TimeoutExpired(["taskkill"], 10)):
            with self.subTest(error=type(error).__name__):
                process = Mock(pid=4321)
                process.poll.return_value = None
                with patch.object(uia_probe_matrix.subprocess, "run", side_effect=error) as run:
                    errors = uia_probe_matrix.stop_probe(process, io.BytesIO())
                self.assertTrue(errors)
                self.assertIn("taskkill", errors[0])
                self.assertEqual(run.call_args.args[0],
                                 ["taskkill", "/PID", "4321", "/T", "/F"])
                process.kill.assert_called_once_with()
                process.wait.assert_called_once_with(timeout=10)

    def test_cleanup_errors_are_preserved_instead_of_aborting_the_report(self):
        process = Mock(pid=4321)
        process.poll.return_value = None
        process.kill.side_effect = OSError("kill denied")
        process.wait.side_effect = subprocess.TimeoutExpired(["probe"], 10)
        with patch.object(uia_probe_matrix.subprocess, "run", return_value=Mock(returncode=1)):
            errors = uia_probe_matrix.stop_probe(process, io.BytesIO())
        self.assertEqual(len(errors), 3)
        self.assertTrue(errors[0].startswith("taskkill"))
        self.assertTrue(errors[1].startswith("kill"))
        self.assertTrue(errors[2].startswith("reap"))


if __name__ == "__main__":
    unittest.main()
