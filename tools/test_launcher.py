"""The ui4me-host console script must hand control to the native host."""
import os
import stat
import sys
import tempfile
import time
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



class HostDiscoveryTests(unittest.TestCase):
    """BIND-04: without a packaged host, never resolve to the console script."""

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        self.package = self.root / "pkg"  # an installed ui4me_core without _bin
        self.package.mkdir()
        self.name = "ui4me-host.exe" if os.name == "nt" else "ui4me-host"

    def put(self, folder, content):
        folder = self.root / folder
        folder.mkdir()
        target = folder / self.name
        target.write_bytes(content)
        target.chmod(target.stat().st_mode | stat.S_IXUSR)
        return target

    def discover(self, *folders):
        env = {"PATH": os.pathsep.join(str(self.root / f) for f in folders)}
        with mock.patch.dict(os.environ, env, clear=False), \
                mock.patch.object(launcher.importlib.resources, "files", return_value=self.package):
            os.environ.pop("UI4ME_HOST_BINARY", None)
            return launcher.host_binary()

    def wrapper(self):
        if os.name == "nt":  # pip/uv launcher: a PE stub with a zip appended
            return b"MZ" + b"\0" * 200 + b"PK\x03\x04 __main__.py PK\x05\x06" + b"\0" * 18
        return b"#!/usr/bin/python3\nfrom ui4me_core.launcher import main\nmain()\n"

    def native(self):
        return (b"MZ" if os.name == "nt" else b"\x7fELF") + b"\0" * 256

    def test_console_script_on_path_is_refused_quickly(self):
        self.put("scripts", self.wrapper())
        started = time.monotonic()
        with self.assertRaisesRegex(FileNotFoundError, "Python wrapper"):
            self.discover("scripts")
        self.assertLess(time.monotonic() - started, 1.0)

    def test_native_host_later_on_path_is_found(self):
        self.put("scripts", self.wrapper())
        native = self.put("native", self.native())
        self.assertEqual(self.discover("scripts", "native"), native)


if __name__ == "__main__":
    unittest.main()
