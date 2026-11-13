"""Build a platform wheel containing the Rust host without a Python extension ABI.

The Rust executable runs independently and talks to Python over a private
versioned local protocol. No shared extension library or CPython embedding is
required. Python `py3-none-PLATFORM` wheels include the host binary as data.
"""
from __future__ import annotations
import os
from pathlib import Path
import shutil
import subprocess
import sys
from setuptools import Distribution, setup
from setuptools.command.build_py import build_py
from wheel.bdist_wheel import bdist_wheel

ROOT = Path(__file__).resolve().parent


class BuildWithHost(build_py):
    def run(self):
        super().run()
        if sys.platform not in ("linux", "win32"):
            raise RuntimeError("The v1.0 wheel build supports Linux and Windows only.")
        exe = "ui4me-host.exe" if sys.platform == "win32" else "ui4me-host"
        custom = os.environ.get("UI4ME_HOST_BINARY")
        if custom:
            host = Path(custom).expanduser().resolve()
            if not host.is_file():
                raise RuntimeError("UI4ME_HOST_BINARY must identify an existing executable")
        else:
            command = ["cargo", "build", "--locked", "--release", "-p", "ui4me-host"]
            env = os.environ.copy()
            env.setdefault("CARGO_BUILD_JOBS", "2")
            subprocess.run(command, cwd=ROOT, env=env, check=True)
            target = Path(env.get("CARGO_TARGET_DIR") or ROOT / "target")
            host = (target if target.is_absolute() else ROOT / target) / "release" / exe
        dest = Path(self.build_lib) / "ui4me_core" / "_bin" / exe
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(host, dest)
        dest.chmod(0o755)
        subprocess.run([str(dest), "--version"], check=True)
        # The executable statically links its Rust dependencies: ship every
        # linked package's license texts next to it.
        subprocess.run([sys.executable, str(ROOT / "tools" / "release" / "collect_licenses.py"),
                        "--output", str(dest.parent / "THIRD_PARTY_LICENSES.txt")],
                       cwd=ROOT, check=True)


class PlatformWheel(bdist_wheel):
    def finalize_options(self):
        super().finalize_options()
        # The Python code is ABI-independent, but the wheel contains a native
        # (ELF or PE) executable and absolutely must not have a platform-neutral tag.
        self.root_is_pure = False

    def get_tag(self):
        # No CPython extension module: any Python 3 on this platform can use
        # it (`py3-none-<platform>`), not only the interpreter that built it.
        _, _, platform = super().get_tag()
        return "py3", "none", platform


class BinaryDistribution(Distribution):
    """Install into platlib at the wheel root (not `.data/purelib`)."""

    def has_ext_modules(self):
        return True


setup(distclass=BinaryDistribution,
      cmdclass={"build_py": BuildWithHost, "bdist_wheel": PlatformWheel})
