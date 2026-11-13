import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOL = Path(__file__).resolve().parent / "create_launcher.py"
spec = importlib.util.spec_from_file_location("create_launcher", TOOL)
create_launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(create_launcher)


class ShortcutNameTests(unittest.TestCase):
    def test_windows_shortcut_uses_a_safe_display_name(self):
        name = create_launcher.windows_shortcut_name
        self.assertEqual(name("UI4ME Host Reference"), "UI4ME Host Reference.lnk")
        self.assertEqual(name('a:b/c?"<>|*'), "a_b_c______.lnk")
        self.assertEqual(name("CON"), "_CON.lnk")
        self.assertEqual(name("Příliš ✓. "), "Příliš ✓.lnk")


class ManifestEncodingTests(unittest.TestCase):
    def test_manifest_with_utf8_byte_order_mark_is_accepted(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "app.toml"
            path.write_bytes(b"\xef\xbb\xbf" + '[app]\nid = "org.example.Bom"\nname = "Příliš"\n'.encode("utf-8"))
            self.assertEqual(create_launcher.read_manifest(path)["app"],
                             {"id": "org.example.Bom", "name": "Příliš"})


@unittest.skipUnless(sys.platform == "linux", "Desktop entries are the Linux launcher")
class LinuxLauncherTests(unittest.TestCase):
    def test_install_refuses_modified_entry_and_uninstall_removes_only_its_own(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            env = dict(os.environ, XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"))
            app = root / "app" / "app.toml"
            app.parent.mkdir()
            app.write_text('[app]\nid = "org.example.LauncherTest"\nname = "Launcher test"\n'
                           '[backend]\nprogram = "/bin/true"\n', encoding="utf-8")
            def run(*arguments):
                return subprocess.run([sys.executable, str(TOOL), *arguments], env=env,
                                      capture_output=True, text=True, check=False)
            installed = run("install", "--app", str(app), "--host", sys.executable)
            self.assertEqual(installed.returncode, 0, installed.stderr)
            entry = Path(json.loads(installed.stdout)["launcher"])
            self.assertIn("Name=Launcher test", entry.read_text(encoding="utf-8"))
            entry.write_text(entry.read_text(encoding="utf-8") + "# edited\n", encoding="utf-8")
            self.assertNotEqual(run("install", "--app", str(app), "--host", sys.executable).returncode, 0)
            self.assertNotEqual(run("uninstall", "--app-id", "org.example.LauncherTest").returncode, 0)
            self.assertTrue(entry.exists(), "a modified launcher must not be replaced or removed")
            entry.write_text(entry.read_text(encoding="utf-8").replace("# edited\n", ""), encoding="utf-8")
            removed = run("uninstall", "--app-id", "org.example.LauncherTest")
            self.assertEqual(removed.returncode, 0, removed.stderr)
            self.assertFalse(entry.exists())
            self.assertTrue(app.exists(), "application files are left intact")


if __name__ == "__main__":
    unittest.main()
