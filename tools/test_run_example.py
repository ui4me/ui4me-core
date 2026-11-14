"""Generated reference manifests carry the product version."""
import tomllib
import unittest

import run_example


class ManifestVersionTests(unittest.TestCase):
    def test_reference_manifest_uses_the_workspace_version(self):
        with open(run_example.ROOT / "Cargo.toml", "rb") as stream:
            expected = tomllib.load(stream)["workspace"]["package"]["version"]
        app = tomllib.loads(run_example.manifest([]))["app"]
        self.assertEqual(app["version"], expected)
        self.assertEqual(expected, "1.0.1")

    def test_explicit_version_is_kept(self):
        app = tomllib.loads(run_example.manifest([], app_version="2.3.4"))["app"]
        self.assertEqual(app["version"], "2.3.4")


if __name__ == "__main__":
    unittest.main()
