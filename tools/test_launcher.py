"""The ui4me-host console script must hand control to the native host."""
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
from ui4me_core import launcher


class LauncherTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "exec handoff is POSIX-only")
    def test_posix_wrapper_execs_host_so_signals_reach_it(self):
        fake = Path("/opt/ui4me/ui4me-host")
        with mock.patch.object(launcher, "host_binary", return_value=fake), \
                mock.patch.object(sys, "argv", ["ui4me-host", "run", "--app", "a b/app.toml"]), \
                mock.patch.object(launcher.os, "execv", side_effect=SystemExit(0)) as execv, \
                mock.patch.object(launcher.subprocess, "call") as call:
            with self.assertRaises(SystemExit):
                launcher.main()
        execv.assert_called_once_with(str(fake), [str(fake), "run", "--app", "a b/app.toml"])
        call.assert_not_called()

    def test_windows_wrapper_waits_and_returns_host_status(self):
        fake = Path("C:/ui4me/ui4me-host.exe")
        with mock.patch.object(launcher, "host_binary", return_value=fake), \
                mock.patch.object(sys, "argv", ["ui4me-host", "--version"]), \
                mock.patch.object(launcher.os, "name", "nt"), \
                mock.patch.object(launcher.subprocess, "call", return_value=3) as call:
            self.assertEqual(launcher.main(), 3)
        call.assert_called_once_with([str(fake), "--version"])

    def test_no_arguments_prints_usage(self):
        with mock.patch.object(sys, "argv", ["ui4me-host"]):
            self.assertEqual(launcher.main(), 2)


if __name__ == "__main__":
    unittest.main()
