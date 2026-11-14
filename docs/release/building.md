# Building and verifying releases

Release artifacts are built **from one canonical source distribution**: one sdist, one Linux `manylinux_2_34_x86_64` wheel and one Windows `win_amd64` wheel, all from the same commit. The PowerShell Gallery module reuses the native hosts from those exact wheels; it is never compiled separately.

The release workflows (see [Publishing](publishing.md)) do all of this on GitHub Actions. This page describes the same steps for a local release candidate.

## 1. Source gates

```bash
python3 tools/release/verify_source.py              # versions agree, required files present
python3 tools/release/sync_powershell_docs.py --check
python3 -m unittest discover -s tools -p 'test_*.py'
cargo fmt -p ui4me-host -p ui4me-renderer -p ui4me-document -p ui4me-assets -p ui4me-engine-types -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 tools/verify.py                              # adds headless, renderer and fault suites
```

Run the Pester suites with PowerShell 7 (and Windows PowerShell 5.1 on Windows):

```powershell
$c = New-PesterConfiguration; $c.Run.Path = 'powershell/tests'; Invoke-Pester -Configuration $c
```

## 2. Source manifest

`tools/release/SOURCE_FILES.sha256` lists the SHA-256 of every tracked source file. Regenerate it deliberately, after review and before the release commit, never to silence a failed check:

```bash
python3 tools/release/refresh_source_manifest.py
python3 tools/release/refresh_source_manifest.py --check
python3 tools/verify_package.py
```

## 3. Source distribution

```bash
python3 -m build --sdist --outdir dist
python3 -m twine check dist/ui4me_core-1.0.1.tar.gz
```

`twine check` validates the long description, which is `docs/publish/pypi.md` as declared in `pyproject.toml`.

## 4. Linux manylinux wheel (from the sdist)

```bash
CIBW_BUILD=cp312-manylinux_x86_64 CIBW_ARCHS_LINUX=x86_64 CIBW_MANYLINUX_X86_64_IMAGE=manylinux_2_34 \
CIBW_BEFORE_ALL_LINUX='dnf install -y wayland-devel libxkbcommon-devel dbus-devel pkgconf-pkg-config curl && curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.99.1' \
CIBW_ENVIRONMENT_LINUX='PATH="$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=2' \
CIBW_REPAIR_WHEEL_COMMAND_LINUX='auditwheel repair -w {dest_dir} {wheel}' \
CIBW_TEST_COMMAND_LINUX='python {project}/tools/release/smoke_installed_wheel.py' \
python3 -m cibuildwheel --platform linux --output-dir dist dist/ui4me_core-1.0.1.tar.gz
```

These are the same settings as `.github/workflows/release-linux.yml`. The smoke test installs the wheel and runs a direct-start app (`python app.py`) through the bundled host in headless test mode. `tools/release/build_linux.sh` builds a quick local `linux_x86_64` wheel for testing, which is not publishable.

## 5. Windows wheel (from the same sdist)

On Windows x64 with the pinned Rust toolchain and C++ build tools:

```powershell
tar -xzf ui4me_core-1.0.1.tar.gz
cd ui4me_core-1.0.1
$env:CARGO_BUILD_JOBS = '1'
python -m pip wheel --no-deps --wheel-dir ..\dist .
```

Use a fresh `CARGO_TARGET_DIR` for each extracted sdist. `tools/release/build_windows.ps1` builds the sdist and wheel together from a checkout.

## 6. Verify the release set

```bash
python3 -m twine check dist/*
python3 tools/release/verify_publish_dist.py dist --write-hashes dist/SHA256SUMS
```

`verify_publish_dist.py` requires exactly the three expected files. It checks each wheel's tag, bundled host, licenses and skill, and checks that both wheels came from the sdist's sources. Then install each wheel into a clean virtual environment (Python 3.10, 3.12 and 3.13) and run `tools/release/smoke_installed_wheel.py`.

## 7. PowerShell Gallery stage

```bash
python3 tools/release/build_psgallery.py \
  --linux-wheel dist/ui4me_core-1.0.1-py3-none-manylinux_2_34_x86_64.whl \
  --windows-wheel dist/ui4me_core-1.0.1-py3-none-win_amd64.whl \
  --source-identity "$(git rev-parse HEAD)" --out dist/psgallery
python3 tools/release/verify_psgallery.py dist/psgallery/UI4ME.Core
```

The stage contains the module, the packaged README and documentation, the `ui4me-powershell` skill, both native hosts with their license files, and `HOST-SOURCE-MANIFEST.json` recording every hash. Run the Pester suites against it with `UI4ME_TEST_MODULE=dist/psgallery/UI4ME.Core/UI4ME.Core.psd1`.

## 8. Native qualification

Headless and CI results do not qualify a release. Install the candidate artifacts on the supported desktops and test real windows:

- **Linux, KDE Plasma (Wayland):** `python app.py`, `uv run app.py`, `python -m package.app`, `pwsh -File app.ps1`, the generated projects, real mouse and keyboard input, callbacks, dialogs, clipboard, closing and quitting, Ctrl+C, and that no host processes or `systemd --user` units are left behind.
- **Windows x64 interactive session:** the same with Python, PowerShell 7 and Windows PowerShell 5.1.

Record the source commit, artifact hashes and results in the release notes.
