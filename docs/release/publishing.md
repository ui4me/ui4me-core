# Publishing a release

Publishing is **always a deliberate, manual step**. Pushes and tags never publish anything. Both registries keep versions forever: if a version number has been used on PyPI or the PowerShell Gallery, choose the next patch version and never try to overwrite it.

## Before you publish

1. All gates in [Building and verifying releases](building.md) pass for the exact commit on `main`, including native qualification on Linux and Windows.
2. The version is the same everywhere. `python3 tools/release/verify_source.py` checks Cargo, `Cargo.lock` and `pyproject.toml`. The module manifest, workflows and verifiers name the version explicitly.
3. `tools/release/SOURCE_FILES.sha256` is current (`refresh_source_manifest.py --check` and `tools/verify_package.py` pass).
4. `docs/release/RELEASE_NOTES_<version>.md` exists and records the qualification.
5. The commit is pushed to `origin/main` (fast-forward only).

## 1. PyPI and the GitHub release

The workflow `.github/workflows/publish.yaml` is the PyPI Trusted Publisher: owner `ui4me`, repository `ui4me-core`, workflow `publish.yaml`, environment `pypi`. No API token is stored.

```bash
gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
  -f version=1.0.1 -f confirmation=publish-1.0.1
gh run watch --repo ui4me/ui4me-core
```

The workflow:

1. refuses to run unless it is on `main`, the version matches the source, the confirmation phrase is exact, and neither PyPI nor GitHub already has the version;
2. builds the sdist, then the manylinux and Windows wheels **from that sdist**, and smoke-tests each installed wheel;
3. verifies the exact three-file set and writes `SHA256SUMS`;
4. uploads to PyPI in the `pypi` environment with OIDC (`id-token: write` only in that job);
5. creates the GitHub release `v1.0.1` with the artifacts, `SHA256SUMS` and the release notes.

If only the final GitHub-release job fails, re-run that job alone. Never re-run the PyPI upload.

## 2. PowerShell Gallery

Run this only after step 1 succeeded, because it stages the native hosts from the wheels attached to the GitHub release `v1.0.1`:

```bash
gh workflow run publish-powershell.yaml --repo ui4me/ui4me-core --ref main \
  -f version=1.0.1 -f confirmation=publish-powershell-1.0.1
```

It downloads and checksum-verifies the released wheels, stages and verifies the module (`build_psgallery.py`, `verify_psgallery.py`), runs Pester against the staged package on Linux, Windows PowerShell 7 and Windows PowerShell 5.1, publishes it in the `psgallery` environment with the `PSGALLERY_API_KEY` secret, and attaches the `.nupkg` to the GitHub release.

## After publishing

- Install from the registries into clean environments: `pip install ui4me-core==1.0.1` / `uv add ui4me-core==1.0.1`, and `Install-Module UI4ME.Core -RequiredVersion 1.0.1`. Run the quickstart apps.
- Compare the PyPI file hashes with `SHA256SUMS` on the GitHub release.
- Check the rendered pages: the GitHub README, the PyPI description and the Gallery listing.
