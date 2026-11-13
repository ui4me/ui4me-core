# Building UI4ME Core 1.0.0 for Windows (x64)

The Windows distribution is the shared source distribution plus one platform
wheel, `ui4me_core-1.0.0-py3-none-win_amd64.whl`. The wheel contains the pure
Python client and the compiled `ui4me-host.exe`; it has no CPython extension
module, so the single wheel serves CPython 3.10 and newer on 64-bit Windows.
Nothing in these steps publishes anything.

## Prerequisites

- Windows 10/11 or Windows Server 2019+ x64 with the MSVC C++ build tools
  (`tools/bootstrap_windows.ps1 -InstallBuildTools` in an elevated provisioning
  session installs them).
- The Rust toolchain pinned in `rust-toolchain.toml` (`rustup` selects it).
- Python 3.10+ for the build front end. Use a virtual environment; the build
  installs `build`, `setuptools`, `wheel`, `twine` and `packaging` into it.

## Build and validate

```powershell
py -3.13 -m venv $env:LOCALAPPDATA\ui4me-build
powershell -ExecutionPolicy Bypass -File tools\release\build_windows.ps1 `
    -Python $env:LOCALAPPDATA\ui4me-build\Scripts\python.exe -OutDir dist-windows
```

The script builds the sdist, then the wheel **from that sdist** (proving the
archive is a complete build input), runs `twine check`, and runs
`tools/release/verify_windows_artifact.py`, which requires:

- exactly one `ui4me_core/_bin/ui4me-host.exe`, a PE32+ x86-64 image;
- the single tag `py3-none-win_amd64`, platlib root (no `.data/purelib`);
- `ui4me_core/_bin/THIRD_PARTY_LICENSES.txt` generated for
  `x86_64-pc-windows-msvc` (includes the Windows-only `windows-sys` family and
  the MPL-2.0 Stylo crates) and the project, font and MPL license files;
- no local agent files, VCS metadata, caches, logs or credentials.

`setup.py` honours `CARGO_TARGET_DIR`. Use a **fresh** target directory for
each wheel built from an sdist: every build extracts the sdist to a new
temporary path with the archive's original timestamps, so Cargo may consider an
old build-script binary of a path crate fresh and run it with the previous
extraction path baked in (`assets/last-resort/SHA256SUMS is missing`). The same steps run in
`.github/workflows/release-windows.yml` (manual dispatch only; uploads CI
artifacts only).

## Install and check without Rust

```powershell
py -3.12 -m venv $env:LOCALAPPDATA\ui4me-venv
& $env:LOCALAPPDATA\ui4me-venv\Scripts\python -m pip install --no-index --only-binary :all: dist-windows\ui4me_core-1.0.0-py3-none-win_amd64.whl
& $env:LOCALAPPDATA\ui4me-venv\Scripts\python tools\release\smoke_installed_wheel.py
& $env:LOCALAPPDATA\ui4me-venv\Scripts\ui4me-host doctor
```

An application manifest names the interpreter to run its backend (for example
the venv's `Scripts\python.exe`) and the installed host is
`...\Lib\site-packages\ui4me_core\_bin\ui4me-host.exe`. Register a Start Menu
shortcut for the current user (no administrative rights) with
`python tools\create_launcher.py install --app <app.toml> --host <ui4me-host.exe>`.

The host is a console-subsystem program so `--version`, `doctor` and `render`
print in a terminal; a launch from Explorer or the Start Menu releases its own
console at startup, so applications show no console window.
