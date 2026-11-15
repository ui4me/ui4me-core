# Building and verifying releases

Release artifacts are built **from one canonical source distribution**: one sdist, one Linux `manylinux_2_34_x86_64` wheel and one Windows `win_amd64` wheel, all from the same commit. The PowerShell Gallery module reuses the native hosts from those exact wheels; it is never compiled separately.

On GitHub Actions this is `.github/workflows/build-dist.yml`, which `publish.yaml` calls (see [Publishing](publishing.md)) and which can also be dispatched on its own for a non-publishing build. This page describes the same steps for a local release candidate. Below, `X.Y.Z` is the version from `python3 tools/release/version.py`.

## 1. Source gates

```bash
python3 tools/release/version.py --check            # every format-forced version location agrees
python3 tools/release/verify_source.py              # versions agree, required files present
python3 tools/release/sync_powershell_docs.py --check
python3 tools/verify.py --keep-going                # fmt, check, test, python tool tests, headless, renderer, faults
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo deny --locked check                           # advisories, bans, licenses, sources (deny.toml)
```

Run the Pester suites with the real host on PowerShell 7 (and Windows PowerShell 5.1 on Windows); the runner fails if any test is skipped:

```powershell
./tools/release/run_pester.ps1 -HostBinary target/debug/ui4me-host     # ui4me-host.exe on Windows
```

The vendored patch stacks can be checked against fresh upstream clones with `bash tools/release/verify_patches.sh` (the nightly workflow runs it).

## 2. Source manifest (release freeze)

`tools/release/SOURCE_FILES.sha256` lists the SHA-256 of every tracked source file. It is a **release-freeze** record: ordinary changes do not update it, so its completeness check is not part of the default test run. Regenerate it deliberately, after review and as part of the release commit, never to silence a failed check:

```bash
python3 tools/release/refresh_source_manifest.py
python3 tools/verify.py --stage release-manifest     # refresh_source_manifest.py --check + verify_package.py
UI4ME_RELEASE_MANIFEST_CHECK=1 python3 -m unittest discover -s tools -p test_source_manifest.py
```

The publish workflows run the same checks on the release commit and refuse a stale manifest; CI runs them when `tools/release/**` changes (`release-manifest.yml`).

## 3. Source distribution

```bash
python3 -m pip install --require-hashes -r tools/release/requirements-release.txt
python3 -m build --sdist --outdir dist
python3 -m twine check dist/ui4me_core-X.Y.Z.tar.gz
```

`twine check` validates the long description, which is `docs/publish/pypi.md` as declared in `pyproject.toml`. The release tools are pinned with hashes to the versions that built the published releases.

## 4. Linux manylinux wheel (from the sdist)

```bash
python3 -m cibuildwheel --platform linux --output-dir dist dist/ui4me_core-X.Y.Z.tar.gz
python3 tools/release/verify_linux_artifact.py dist
```

The configuration lives in `[tool.cibuildwheel]` in `pyproject.toml` (inside the sdist), so the workflow and local builds cannot drift: the manylinux 2.34 image pinned by cibuildwheel, `rustup-init` pinned by version and SHA-256, the toolchain from `rust-toolchain.toml`, `auditwheel repair`, and the installed-wheel smoke test (a direct-start app through the bundled host in headless test mode). The Linux host is reproducible: a local build from the 1.0.1 sdist with this configuration produced the published host bit for bit. `tools/release/build_linux.sh` builds a quick local `linux_x86_64` wheel for testing, which is not publishable.

`verify_linux_artifact.py` and `verify_windows_artifact.py` refuse a development host: a host with `.debug_*` sections, Rust debug-assertion checks or more than 150 MiB. To package a prebuilt host, set `UI4ME_WHEEL_HOST_BINARY` to a **release** build; the runtime override `UI4ME_HOST_BINARY` is ignored by `setup.py`.

## 5. Windows wheel (from the same sdist)

On Windows x64 with the pinned Rust toolchain and C++ build tools:

```powershell
tar -xzf ui4me_core-X.Y.Z.tar.gz
cd ui4me_core-X.Y.Z
$env:CARGO_TARGET_DIR = "$env:TEMP\ui4me-cargo-rc"   # fresh for each extracted sdist
python -m build --wheel --outdir ..\dist .
python ..\tools\release\verify_windows_artifact.py ..\dist
```

`verify_windows_artifact.py` checks the PE32+ console host, its licenses and the main-thread stack reserve (`SizeOfStackReserve` must be 33554432, `host_checks.WINDOWS_HOST_STACK_RESERVE`; the 1 MiB default aborted on content nested about 40 deep, audit F-01). `tools/release/build_windows.ps1` builds the sdist and wheel together from a checkout.

## 6. Verify the release set

```bash
python3 -m twine check dist/*
python3 tools/release/verify_publish_dist.py dist --write-hashes dist/SHA256SUMS
```

`verify_publish_dist.py` requires exactly the three expected files and validates each one independently (wheel tags, bundled host, licenses, skill, sdist contents). It does **not** compare the wheels with the sdist: that they were built from it is guaranteed by the build procedure above (both wheel builds take the sdist as their only input). Then install each wheel into clean environments and run `tools/release/smoke_installed_wheel.py`; CI does this in `python:3.10-slim` and `python:3.14-slim`.

## 7. PowerShell Gallery stage

```bash
python3 tools/release/build_psgallery.py \
  --linux-wheel dist/ui4me_core-X.Y.Z-py3-none-manylinux_2_34_x86_64.whl \
  --windows-wheel dist/ui4me_core-X.Y.Z-py3-none-win_amd64.whl \
  --source-identity "$(git rev-parse HEAD)" --out dist/psgallery
python3 tools/release/verify_psgallery.py dist/psgallery/UI4ME.Core
```

The stage contains the module, the packaged README and documentation, the `ui4me-powershell` skill, both native hosts with their license files, and `HOST-SOURCE-MANIFEST.json` recording every hash. Test it with `./tools/release/run_pester.ps1 -TestModule dist/psgallery/UI4ME.Core/UI4ME.Core.psd1`, and test an all-users install used by a non-root account in a container:

```bash
docker run --rm -v "$PWD/tools/release:/tools:ro" -v "$PWD/dist/psgallery:/module:ro" \
  mcr.microsoft.com/powershell:7.4-ubuntu-22.04 sh /tools/smoke_psmodule_allusers.sh /module/UI4ME.Core
```

## 8. Native qualification

Headless and CI results do not qualify a release. See [Qualifying a release candidate](qualification.md): the exact Windows wheel to be published is qualified with `tools/release/qualify_windows_rc.py`, and its recorded hash is what the publish workflow promotes. The windowed smoke test (`tools/windowed_smoke.py`) runs the same probes on Linux under a private compositor. In addition, on the supported desktops:

- **Linux, KDE Plasma (Wayland):** `python app.py`, `uv run app.py`, `python -m package.app`, `pwsh -File app.ps1`, the generated projects, real mouse and keyboard input, callbacks, dialogs, clipboard, closing and quitting, Ctrl+C, and that no host processes or `systemd --user` units are left behind.
- **Windows x64 interactive session:** the same with Python, PowerShell 7 and Windows PowerShell 5.1.

Record the source commit, artifact hashes and results in the release notes.
