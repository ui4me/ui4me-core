"""Packaging, source identity, cross-check and visual comparison regressions."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import install
import native_acceptance
from windows_source_check import source_copy


class PackageTests(unittest.TestCase):
    def test_renderer_copy_is_complete_and_pins_real_interpreter(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "Příliš workspace"
            app = install.copy_reference(destination)
            manifest = tomllib.loads(app.read_text(encoding="utf-8"))
            self.assertEqual(manifest["app"]["id"], "org.ui4me.RendererReference")
            self.assertEqual(manifest["backend"]["program"], sys.executable)
            for name in ("client.py", "application.py", "renderer_demo.py", "ui/index.html", "ui/details.html",
                         "ui/style.css", "ui/mark.svg", "ui/landscape.png", "ui/fonts/NotoSans-Regular.ttf",
                         "ui/fonts/FONT-LICENSE.txt", "ui4me.png"):
                self.assertTrue((destination / name).is_file(), name)
            self.assertNotIn("--headless", str(manifest))
            self.assertFalse((destination / "demo.py").exists())

    def test_copy_never_overwrites_existing_app(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            (destination / "user.py").write_text("keep my application", encoding="utf-8")
            with self.assertRaises(ValueError):
                install.copy_reference(destination)
            self.assertEqual(list(destination.iterdir()), [destination / "user.py"])

    def test_host_probe_remains_separately_installable(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            manifest = tomllib.loads(install.copy_reference(destination, probe=True).read_text(encoding="utf-8"))
            self.assertEqual(manifest["app"]["id"], "org.ui4me.HostReference")
            self.assertTrue((destination / "demo.py").is_file())
            self.assertFalse((destination / "ui").exists())


class SourceIdentityTests(unittest.TestCase):
    def test_reports_do_not_change_identity_but_source_and_lockfile_do(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "source.rs").write_text("source", encoding="utf-8")
            with patch.object(native_acceptance, "ROOT", root):
                original = native_acceptance.source_identity()
                for folder in ("verification", "docs", "target", ".git", "__pycache__"):
                    (root / folder).mkdir()
                    (root / folder / "result").write_text("new evidence", encoding="utf-8")
                self.assertEqual(native_acceptance.source_identity(), original)
                (root / "source.rs").write_text("changed executable source", encoding="utf-8")
                changed = native_acceptance.source_identity()
                self.assertNotEqual(changed, original)
                (root / "Cargo.lock").write_text("locked dependency", encoding="utf-8")
                self.assertNotEqual(native_acceptance.source_identity(), changed)

    def test_windows_mirror_preserves_all_workspace_members_and_patches(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, output = Path(temporary) / "source", Path(temporary) / "mirror"
            content = {
                "Cargo.toml": '[workspace]\nmembers=["crates/ui4me-host", "crates/ui4me-renderer"]\n[patch.crates-io]\n',
                "Cargo.lock": "locked",
                "crates/ui4me-host/Cargo.toml": '[package]\nname="ui4me-host"\nbuild="build.rs"\n',
                "crates/ui4me-host/build.rs": "SDK input",
                "crates/ui4me-renderer/Cargo.toml": '[package]\nname="ui4me-renderer"\n',
                "patches/stylo/Cargo.toml": "local patch",
                "vendor/softbuffer/src/lib.rs": "qualified patch",
                "regression/expected.png": "test fixture",
                "target/large-output": "excluded",
            }
            for name, text in content.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
            source_copy(root, output)
            for name, text in content.items():
                if name == "crates/ui4me-host/Cargo.toml":
                    self.assertIs(tomllib.loads((output / name).read_text())["package"]["build"], False)
                elif name.startswith("target/"):
                    self.assertFalse((output / name).exists())
                else:
                    self.assertEqual((output / name).read_text(), text, name)

    def test_native_qualification_cannot_ignore_failed_or_missing_smoke(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, output = Path(temporary) / "source", Path(temporary) / "report"
            root.mkdir()
            output.mkdir()
            context = {"os": "fixture", "release": "1", "architecture": "test", "machine": "test", "username": "test"}
            report = {"schema": 2, "checks": {"R01": {"status": "PASS"}}}
            with patch.object(native_acceptance, "ROOT", root), \
                 patch.object(native_acceptance, "interactive_context", return_value=(True, context)), \
                 patch.object(sys, "argv", ["renderer_acceptance.py", "--output", str(output)]), \
                 patch("builtins.print"):
                report["source_sha256"] = native_acceptance.source_identity()
                for automated, expected_status, expected_code in ((None, "BLOCKED", 2), ("FAIL", "FAIL", 1), ("PASS", "PASS", 0)):
                    if automated:
                        report["automated_subset"] = {"status": automated}
                    (output / "acceptance.json").write_text(json.dumps(report), encoding="utf-8")
                    self.assertEqual(native_acceptance.main({"R01": "fixture"}, "renderer"), expected_code)
                    actual = json.loads((output / "acceptance.json").read_text(encoding="utf-8"))
                    self.assertEqual(actual["qualification"], expected_status)


@unittest.skipUnless(importlib.util.find_spec("PIL"), "Optional Pillow visual oracle is not installed")
class ImageComparisonTests(unittest.TestCase):
    def test_difference_metrics_and_thresholds_cannot_mask_size_mismatch(self):
        from PIL import Image
        from browser_reference import compare_images, within_tolerance
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected, actual, delta = (root / name for name in ("expected.png", "actual.png", "difference.png"))
            Image.new("RGB", (10, 10), "white").save(expected)
            changed = Image.new("RGB", (10, 10), "white")
            changed.putpixel((0, 0), (0, 0, 0))
            changed.save(actual)
            metrics = compare_images(expected, actual, delta)
            self.assertAlmostEqual(metrics["mean_absolute_channel_error"], 2.55)
            self.assertAlmostEqual(metrics["fraction_pixels_over_threshold"], 0.01)
            self.assertTrue(delta.exists())
            tolerance = {"mean_absolute_channel_error": 2, "fraction_pixels_over_threshold": 0.015}
            self.assertFalse(within_tolerance(metrics, tolerance))
            Image.new("RGB", (9, 10), "white").save(actual)
            self.assertFalse(within_tolerance(compare_images(expected, actual, delta), tolerance))


if __name__ == "__main__":
    unittest.main()
