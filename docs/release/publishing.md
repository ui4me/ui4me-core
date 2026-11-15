# Publishing a release

Publishing is **always a deliberate, manual step**. Pushes and tags never publish anything. Both registries keep versions forever: if a version number has been used on PyPI or the PowerShell Gallery, choose the next patch version and never try to overwrite it. What the version number promises, and how to yank a broken release, is in [Versioning, compatibility and support](versioning.md).

Below, `X.Y.Z` is the version being published.

## Before you publish

1. The release commit is on `origin/main` and **CI passed for that exact commit** (`.github/workflows/ci.yml`, every job listed in `tools/release/required_checks.json`). The publish workflows check this themselves (`tools/release/ci_gate.py`) and refuse otherwise, so wait for CI to finish.
2. The version is bumped everywhere: `python3 tools/release/bump_version.py X.Y.Z`, then `python3 tools/release/version.py --check --strict`. Workflows and verifiers read the version from `Cargo.toml` and need no edits.
3. `tools/release/SOURCE_FILES.sha256` is regenerated in the release commit (`refresh_source_manifest.py`, then `python3 tools/verify.py --stage release-manifest`).
4. `CHANGELOG.md` has the `X.Y.Z` section, and `docs/release/RELEASE_NOTES_X.Y.Z.md` exists (the GitHub release uses it; the workflow refuses without it). It lists the known open High-severity issues.
5. Native qualification on Linux and Windows is done ([Qualifying a release candidate](qualification.md)); for Windows, the exact wheel that will be published.

## 1. PyPI and the GitHub release

The workflow `.github/workflows/publish.yaml` is the PyPI Trusted Publisher: owner `ui4me`, repository `ui4me-core`, workflow `publish.yaml`, environment `pypi`. No API token is stored.

Recommended path (the published Windows wheel is the qualified one):

```bash
# 1. Dry run: gate, build and verify; nothing is published.
gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
  -f version=X.Y.Z -f confirmation=publish-X.Y.Z -f dry_run=true
# 2. Qualify the dry run's Windows wheel (qualification.md), then promote it:
gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
  -f version=X.Y.Z -f confirmation=publish-X.Y.Z \
  -f candidate_run_id=<dry-run id> -f expected_windows_wheel_sha256=<recorded hash>
gh run watch --repo ui4me/ui4me-core
```

Without `candidate_run_id` the workflow builds and publishes in one run (the Windows wheel is then not the natively qualified binary). Optional: `-f check_metadata_urls=true` also requires every URL in the package metadata to resolve without credentials (F-20).

The workflow:

1. **preflight** refuses unless it runs on `main`, the version equals `Cargo.toml`, the confirmation phrase is exact, the tree is clean, the release notes exist, **the required CI succeeded for the exact commit**, the source manifest is current, and neither PyPI nor GitHub already has the version;
2. **build** (`build-dist.yml`) builds the sdist, then the manylinux and Windows wheels from that sdist, smoke-tests the installed wheels (including `python:3.10-slim` and `python:3.14-slim`), generates the host SBOMs and writes `SHA256SUMS` and `SOURCE_COMMIT.txt`; or, with `candidate_run_id`, the artifacts of that dry run are taken instead;
3. **stage** re-verifies the exact files (and the qualified Windows hash, when given);
4. **attest** creates GitHub build-provenance attestations for them;
5. **draft-release** creates a **draft** GitHub release `vX.Y.Z` with every file, `SHA256SUMS`, `SOURCE_COMMIT.txt` and the SBOMs;
6. **pypi** uploads the same files to PyPI in the `pypi` environment with OIDC and PEP 740 attestations (`id-token: write` only in this job and in **attest**);
7. **publish-release** publishes the draft, which creates the tag `vX.Y.Z` at the source commit, and checks it.

`dry_run=true` stops after step 3. If a job after the PyPI upload fails, re-run only that job; never re-run the PyPI upload. If the run fails before PyPI, delete the draft release (if one was created) before dispatching again.

## 2. PowerShell Gallery

Run this only after step 1 succeeded and the release is published, because it stages the native hosts from the wheels attached to the GitHub release `vX.Y.Z`:

```bash
gh workflow run publish-powershell.yaml --repo ui4me/ui4me-core --ref main \
  -f version=X.Y.Z -f confirmation=publish-powershell-X.Y.Z
```

It refuses unless `main` is exactly the commit of tag `vX.Y.Z` (a module-only fix needs a new patch release) and CI passed for it. It downloads and checksum-verifies the released wheels, stages and verifies the module (`build_psgallery.py`, `verify_psgallery.py`), runs Pester against the staged package on Linux, Windows PowerShell 7 and Windows PowerShell 5.1 with no skipped tests, checks an all-users install used by a non-root account, publishes it in the `psgallery` environment with the `PSGALLERY_API_KEY` secret, and attaches the `.nupkg` and `SHA256SUMS-powershell` to the GitHub release.

## After publishing

- Dispatch `verify-published.yml` with the version: it compares the PyPI digests with `SHA256SUMS`, installs from PyPI into clean `python:3.10-slim` and `python:3.14-slim` images and on Windows, and installs the Gallery module (all users on Linux, current user on Windows) and runs the smoke tests and Pester against it.
- For Windows: `python tools/release/qualify_windows_rc.py --compare qualification-windows-X.Y.Z.json --pypi X.Y.Z`.
- Check the rendered pages: the GitHub README, the PyPI description and the Gallery listing.
