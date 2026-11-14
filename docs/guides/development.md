# Developing UI4ME Core

This page is for people working on UI4ME Core itself. To build apps with it you only need the [Python](install-python.md) or [PowerShell](install-powershell.md) package; Rust is not required.

## Repository map

| Path | Purpose |
| --- | --- |
| `crates/ui4me-host/` | Native host: windows, event loop, IPC server, backend supervision, desktop services |
| `crates/ui4me-renderer/` | Style, layout, painting, input, editing and accessibility projection |
| `crates/ui4me-document/` | Document model, HTML parsing, forms and mutations |
| `crates/ui4me-assets/` | Fonts, images, SVG and local resource policy |
| `crates/ui4me-engine-types/` | Shared geometry and engine types |
| `python/ui4me_core/` | Python package: `Application` API, IPC client, direct startup (`_bootstrap.py`), `ui4me-new`, `ui4me-skill`, host launcher |
| `powershell/UI4ME.Core/` | PowerShell module and its C# helper (`private/UI4ME.Transport.cs`) |
| `powershell/tests/` | Pester suites and the native qualification harness |
| `skills/` | Canonical coding-agent skills, bundled into both packages |
| `examples/` | Python and PowerShell example apps and test fixtures |
| `tools/` | Unit and integration tests, runners and qualification tools |
| `tools/release/` | Build, staging and artifact verifiers, `SOURCE_FILES.sha256`, release records |
| `patches/`, `third_party/` | Pinned, locally patched dependencies (build inputs) |
| `.github/workflows/` | CI, and the manually dispatched release workflows |

## Set up

Linux (Debian/Ubuntu packages):

```bash
bash tools/bootstrap_linux.sh --install-system
cargo build --locked -p ui4me-host
```

Windows: run `tools/bootstrap_windows.ps1` in the development account (`-InstallBuildTools` from an administrative session if the C++ build tools are missing), then `cargo build --locked -p ui4me-host`. Use `CARGO_BUILD_JOBS=1` or `2` on small virtual machines.

To run apps against your build, point the bindings at it and put the package source on the path:

```bash
export UI4ME_HOST_BINARY="$PWD/target/debug/ui4me-host"
export PYTHONPATH="$PWD/python"
python examples/python/hello.py
```

```powershell
$env:UI4ME_HOST_BINARY = "$PWD/target/debug/ui4me-host"   # ui4me-host.exe on Windows
Import-Module ./powershell/UI4ME.Core/UI4ME.Core.psd1
pwsh -File ./powershell/examples/hello/app.ps1
```

## Checks while you work

Run the narrowest checks that cover your change:

```bash
python -m unittest tools/test_bootstrap.py          # direct start (real host, headless)
python -m unittest discover -s tools -p 'test_*.py' # all Python and tooling tests
cargo test --locked -p ui4me-host                    # one crate
cargo fmt -p ui4me-host -p ui4me-renderer -p ui4me-document -p ui4me-assets -p ui4me-engine-types -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

```powershell
cd powershell/tests
$c = New-PesterConfiguration; $c.Run.Path = '.'; Invoke-Pester -Configuration $c
```

`UI4ME_TEST_MODE=1` runs the host headless: protocol, lifecycle and document behaviour without a desktop or visible windows. Both direct-start helpers honour it, which is how `tools/test_bootstrap.py` and `powershell/tests/DirectStart.Tests.ps1` exercise `python app.py` and `pwsh -File app.ps1` with the real host. A headless pass is **not** desktop qualification.

When you edit `docs/reference/powershell-api.md`, regenerate the skill copy with `python tools/release/sync_powershell_docs.py`; `--check` fails if they drift.

## Integration checkpoint

```bash
python tools/verify.py                  # fmt, check, test, python, headless, renderer, faults
bash tools/release/check_linux.sh       # full Linux source gate
```

`tools/verify.py --stage native --stage renderer-native` and the PowerShell harness in `powershell/tests/native/` drive real windows. They need an interactive desktop session (KDE Plasma on Wayland, or an interactive Windows session).

## Documentation audiences

| File | Audience |
| --- | --- |
| `README.md` | GitHub landing page |
| `docs/publish/pypi.md` | PyPI project description (`readme` in `pyproject.toml`) |
| `powershell/UI4ME.Core/README.md` | PowerShell Gallery package readme |
| `docs/` | Full documentation |
| `skills/*/SKILL.md` | Coding agents |

When a change affects how people get started, update all of them. `tools/test_public_docs.py` checks links, screenshots and code examples.

See [Building and verifying releases](../release/building.md) and [Publishing](../release/publishing.md).
