# Manually publish UI4ME Core to PyPI and GitHub

**Publishing never happens on ordinary pushes or tag creation.** Only `.github/workflows/publish.yaml` can publish, and it has a `workflow_dispatch` trigger plus a typed confirmation phrase. The build-only `release-linux.yml` and `release-windows.yml` workflows are also manual. No repository credential, PyPI token or password is stored in source.

## Trusted Publisher identity (already registered/pending on PyPI)

| Setting | Exact value |
| --- | --- |
| PyPI project | `ui4me-core` |
| GitHub owner | `ui4me` |
| GitHub repository | `ui4me-core` |
| Trusted publisher workflow filename | **`publish.yaml`** |
| GitHub Actions environment | **`pypi`** |
| Publishing authentication | PyPI Trusted Publishing / OIDC (`id-token: write`) |

The workflow filename and environment must match the pending publisher exactly. In GitHub **Settings → Environments → pypi**, configure required reviewers if you want a second manual approval and restrict permitted deployment branches to `main`. The `pypi` job alone receives OIDC permission. If the environment is not configured, the code still requires an explicit dispatch and confirmation but has no separate human approval.

## Before release

1. Finish the README, docs, screenshots, coding-agent skill, wheel installation and all tests on Linux and Windows. Check out and verify the exact source commit on `main`.
2. Verify the source identity and native acceptance records against the committed source. Use a clean checkout and run `python tools/verify_package.py`, `python tools/release/refresh_source_manifest.py --check`, and `python tools/release/verify_source.py`. If reviewed source files change, regenerate once deliberately with `python tools/release/refresh_source_manifest.py`, then commit the reviewed manifest alongside the changes; never refresh it to conceal unreviewed modifications.
3. Keep `pyproject.toml`, Cargo workspace packages, `Cargo.lock`, examples and release version consistent. The first workflow is deliberately limited to **1.0.0** to prevent accidental future-version publication with an out-of-date release recipe.
4. Validate that no `v1.0.0` GitHub release already exists and PyPI does not already contain 1.0.0 (otherwise publishing that version again will be rejected).
5. **Push the reviewed documentation/skill/workflow changes to `main` first.** Publishing is a separate action, not a side effect of this push.

## Publish deliberately

Use GitHub's **Actions → Publish UI4ME Core to PyPI and GitHub → Run workflow** and select `main`. Enter:

- **version:** `1.0.0`
- **confirmation:** `publish-1.0.0`

Or run the equivalent command after setting up `gh auth login`:

```bash
gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
  -f version=1.0.0 -f confirmation=publish-1.0.0
```

The workflow checks out the exact manual-run commit, builds **one canonical sdist**, builds the Linux repaired manylinux wheel and Windows wheel **from that same sdist**, validates exact platform tags/embedded license notices and a three-artifact SHA-256 manifest, then publishes only the three distribution files through PyPI OIDC. After a successful upload it creates the `v1.0.0` GitHub release, with wheel/sdist assets, SHA256SUMS and release notes, against the same commit.

**Do not push a tag as a substitute for running this workflow.** No tag push triggers publishing. The GitHub release job creates the tag only after PyPI upload succeeds.

## If a job fails

- **Before PyPI upload:** fix the source/build, repeat tests, push a new reviewed commit, and manually dispatch again. No version has been published yet.
- **During PyPI upload:** inspect PyPI's project/version files before retrying. PyPI never allows overwriting published files of the same version. Never enable `skip-existing` to conceal a partial/ambiguous upload.
- **PyPI succeeded but GitHub release failed:** rerun only the failed `github-release` job with GitHub Actions; do **not** run the entire workflow again or re-upload to PyPI. It reuses the validated workflow artifacts and exact source SHA.
- **Version already exists on PyPI:** use a new SemVer release (e.g. `1.0.1`) after updating the release recipe and thoroughly testing it. Do not replace the previous files.

## After release

- Confirm [PyPI](https://pypi.org/project/ui4me-core/1.0.0/) shows one sdist and both platform wheels, with provenance/attestations.
- Confirm [GitHub Release v1.0.0](https://github.com/ui4me/ui4me-core/releases/tag/v1.0.0) points to the exact tested source commit.
- Verify artifact SHA-256 hashes against the release's SHA256SUMS and the PyPI file list.
- Test clean `pip install ui4me-core==1.0.0` into Linux/Windows virtual environments, then `ui4me-host doctor`, `ui4me-new`, and `ui4me-skill install --dry-run`.
- Update `docs/STATUS.md` with the actual publishing result and GitHub run URL; do not claim publication before the workflow reports success.

## Future releases

The workflow currently pins version 1.0.0 and release notes deliberately. A new version requires updating the version in Cargo, Python metadata, artifact verifiers and release notes, along with the hardcoded workflow release checks. Keep publishing a separate **manual-only** action and retain the `publish.yaml` + `pypi` trusted-publisher identity.
