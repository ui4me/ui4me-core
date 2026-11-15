# RELEASE audit: CI, release engineering, test inventory, repository process

> **Coordinator note.** `<scratch>/` and `exp/RELEASE/` paths were session-temporary; container scripts are retained in [../evidence/scripts/release](../evidence/scripts/release/). The open question on MPL-2.0 source availability (INPUTSEC) was checked by the coordinator: the 1.0.1 sdist contains `patches/stylo/**` and `MPL-2.0.txt`.


Audited commit: c6fd71d (tag v1.0.1). Area: RELEASE. Auditor ran no write operation against the repo, GitHub, PyPI or the PowerShell Gallery. `git status` is clean at the end.

## 1. Scope and method

**Read:** `.github/workflows/{ci.yml,publish.yaml,publish-powershell.yaml,release-linux.yml,release-windows.yml,test-powershell.yaml}`, `tools/verify.py`, `tools/verify_package.py`, `tools/test_source_manifest.py`, `tools/test_publish_workflow.py`, `tools/native_acceptance.py`, `tools/renderer_acceptance.py`, `tools/completion_acceptance.py`, `tools/install.py`, `tools/lab/lab.py` (header), `tools/windows_lab.py` (grep), `tools/release/*.py` (verify_source, refresh_source_manifest, verify_publish_dist, build_psgallery, smoke_installed_wheel, verify_linux_artifact (grep)), `tools/release/records/*.json`, `docs/release/*.md`, `docs/guides/development.md`, `docs/guides/platforms.md` (grep), `CONTRIBUTING.md`, `setup.py`, `pyproject.toml`, `MANIFEST.in`, `Cargo.toml`, `rust-toolchain.toml`, `third_party/README.md`, `patches/*/verify.sh`, `powershell/UI4ME.Core/UI4ME.Core.psm1:1297-1322`, `powershell/UI4ME.Core/UI4ME.Core.psd1`, the Pester suites (counts), and `git log --stat`.

**Commands run (all results reproduced by me):**

| # | Command / experiment | Result |
|---|---|---|
| 1 | `gh repo view`, `gh api .../branches/main/protection`, `.../rulesets` | Private repo, org on the **free** plan. Branch protection and rulesets return HTTP 403 ("Upgrade to GitHub Pro or make this repository public"). |
| 2 | `gh api .../environments`, `.../deployment-branch-policies`, `.../environments/psgallery/secrets` | `pypi` and `psgallery` have **only** a `main` branch policy, `can_admins_bypass: true`, no reviewers or wait timer. Secret `PSGALLERY_API_KEY` exists only in `psgallery`. There are no repo-level secrets. There is also an unrelated `LuaRocks` environment with no protection. |
| 3 | `gh api .../actions/permissions{,/workflow}`, `vulnerability-alerts`, `automated-security-fixes`, `immutable-releases`, `actions/cache/usage` | Any action is allowed and SHA pinning is not required. The default token is read-only. Dependabot alerts are on (0 alerts), security updates are off, immutable releases are off, and the Actions cache holds 0 bytes. |
| 4 | `gh run list` + `gh run view --json jobs` + job logs | Full CI/publish timeline (§3, RELEASE-01/02). Logs saved: `exp/RELEASE/job-win-d053e09.log`, `scratchpad/pr3-linux.log`. |
| 5 | `gh api check-runs/<job>/annotations` | Node.js 20 deprecation warning for `actions/checkout@v4`, `actions/setup-python@v5` and `actions/upload-artifact@v4`. |
| 6 | PyPI JSON API plus the PEP 740 integrity API (`/integrity/ui4me-core/1.0.1/<sdist>/provenance`) | The PyPI hashes equal the release `SHA256SUMS`. The publish attestation names publisher `{GitHub, ui4me/ui4me-core, publish.yaml, env pypi}`. `gh api .../attestations/sha256:<sdist>` returns 404: there are no GitHub build-provenance attestations. |
| 7 | Downloaded the v1.0.1 wheels and hashed the embedded host | The Linux host is `be08dd88…`, **identical** to the qualified hash in `qualification-1.0.1.json`. The Windows host is `8d7eab92…`, which **differs** from the qualified `4f9b4b47…`. |
| 8 | `docker run --rm python:{3.10,3.14}-slim` with `pip install ui4me-core==1.0.1`, `ldd`, `ui4me-host --version`, `tools/release/smoke_installed_wheel.py` and a custom headless app (`UI4ME_TEST_MODE=1`) | **PASS on both.** The host links only `libc`, `libm` and `libgcc_s`, because Wayland, xkb and fontconfig are dlopen'ed. Script: `exp/RELEASE/pypi-smoke/run.sh`. |
| 9 | `docker run --rm mcr.microsoft.com/powershell:latest` (pwsh 7.4.2) with `Install-Module UI4ME.Core -RequiredVersion 1.0.1` | A CurrentUser install as a non-root user with a headless `Start-UI4MEScript` passes (exit 0). An **AllUsers install fails for non-root users** (RELEASE-06). Scripts: `exp/RELEASE/psg/run.sh` and `run2.sh`. |
| 10 | Clean-room contributor build: `git archive HEAD` copied read-only into `debian:13` (4 CPUs, 12 GB). Build deps installed, then `bash tools/bootstrap_linux.sh`, `python3 -m unittest discover -s tools` and `tools/run_example.py --headless` | Deps took 31 s. The **debug host built in 4 min 41 s** and reports `ui4me-host 1.0.1`. Tool tests: 120 run, **1 failure** (RELEASE-16), 4 skipped. The headless example passes. Log: `exp/RELEASE/contributor.log`. |
| 11 | `cargo deny --locked check advisories` (local cargo-deny 0.20.2, default config) | **0 vulnerabilities.** 4 `unmaintained` advisories: RUSTSEC-2026-0192 (`ttf-parser` via `fontdue`, a direct host dependency) and RUSTSEC-2026-0370/0371/0372 (`zvariant`, `zbus_names` and `zvariant_derive` via zbus/atspi/ashpd). Output: `exp/RELEASE/cargo-deny-advisories-full.txt`. |
| 12 | `zizmor 1.32.0 --offline --persona=auditor` over `.github/` (in a container) | 70 findings: 42 `unpinned-uses`, 2 `template-injection` (publish-powershell.yaml:215-216), 3 `artipacked`, 4 `concurrency-limits` and others. Output: `exp/RELEASE/zizmor.txt`. |
| 13 | `bash patches/stylo/verify.sh`, `bash patches/taffy/verify.sh` (against local cargo git checkouts of the locked SHAs) | **Both OK.** Each vendored tree equals upstream@SHA plus the listed patches. |
| 14 | Unpacked `~/.cargo/registry/cache/*/<crate>.crate` for the 4 `third_party` crates and diffed against `git archive <"Vendor pristine" commit>` | Identical apart from `.cargo_vcs_info.json` (and the crate's `Cargo.lock` for softbuffer). The crate SHA-256 values match `third_party/README.md`. |
| 15 | `python3 -m unittest tools/test_bootstrap.py` (documented single-file command) | 20 tests OK. |
| 16 | `curl` without credentials on the PyPI project URLs | All return **404** (RELEASE-05). |
| 17 | Main agent's `tools/verify.py --keep-going --jobs 8` (read only) | All 7 stages pass in 5 min 33 s (incremental). 1094 Rust tests passed, 11 ignored, across 59 binaries. 120 Python tests run, 1 skipped. |

All Docker containers ran with `--rm` and unique names (`ui4me-release-audit-*`). I deleted the images I pulled (`python:3.10-slim`, `python:3.14-slim`, `debian:13`, `mcr.microsoft.com/powershell:latest`) afterwards. I did not use the Windows lab or Proxmox, and I did not drive the KDE desktop.

### Test inventory (measured)

| Suite | Count | Kind | Runs in CI? |
|---|---|---|---|
| Rust, `ui4me-renderer` lib | 501 passed + 6 ignored (+5 doctests) | in-crate unit/scenario tests. Pixel probes (`assert_eq!(at(&px,75,25), RED)`), **no opaque golden images**. 1 proptest file. Fuzz harnesses `#[ignore]` (`fuzz_gate.rs:465,534,640`, `fuzz_resources.rs:291`) | yes, except ignored tests |
| Rust, `ui4me-document` | 47 unit + ~145 integration in 20 files (+1 doctest). 2 proptest files. 1M-op misuse test ignored (`tests/handles_misuse.rs:156`) | unit, integration, property | yes |
| Rust, `ui4me-assets` | 52 unit + ~161 integration in 18 files. 2 ignored (`bombs.rs:22`, `shutdown.rs:110`) | integration, sleep-synchronized in places | yes |
| Rust, `ui4me-engine-types` | 16 unit + 59 integration in 7 files (+3 doctests). 4 proptest files | unit, property | yes |
| Rust, `ui4me-host` | 94 unit + 8 integration (`tests/host_integration.rs`), 1 ignored fixture | unit plus headless process integration | yes |
| Python tool tests (`tools/test_*.py`) | 120 tests in 15 files | protocol client, direct start with the **real host in headless mode**, docs and examples against the real host, release tooling | yes (Python 3.12 only) |
| Headless suites (`verify.py` headless/renderer/faults) | 3 stages, 7 fault modes (+ forced host kill on Windows) | real host, `--headless` | yes |
| Pester (`powershell/tests`) | 54 `It`: Unit 19, Transport 24 (mock host), DirectStart 11 | **4 DirectStart tests need a real host and are skipped in every CI run** | only `test-powershell.yaml`, path-filtered. The real host runs only inside `publish-powershell.yaml` |
| Native ledgers: `native_acceptance.py` N01-N15, `renderer_acceptance.py` R01-R12, `completion_acceptance.py` C01-C12, `powershell/tests/native/Qualify.ps1` (28 checks), `drive_linux.py`, `drive_windows.py`, `tools/lab/*` (root uinput daemon, KWin scripting), `tools/windows/*` (UIA probes over RDP), `tools/windows_lab.py` (SSH to Proxmox and a VM via a private JSON config) | 39 ledger rows + 28 harness checks | real windows and real input | **never in CI**. They need the private lab and cannot be reproduced by a third party without rebuilding it |

What CI actually verifies: compile on Linux and Windows, clippy and fmt, Rust unit and integration tests, the protocol and lifecycle and DOM in headless mode, fault injection, docs examples running headless, and release-tool logic. **Not verified automatically:** any real window (Wayland, Win32), input, IME, clipboard, portals and dialogs, AT-SPI/UIA exposure, DPI, the GPU-less present path (`softbuffer`), PowerShell against the real host (outside of publishing), Python versions other than 3.12, Windows 10/11, and the release-profile binary outside the publish smoke test.

## 2. Strengths to preserve

- **Publication is deliberate and narrowly privileged.** It runs only on `workflow_dispatch`, is guarded to `main`, requires a typed confirmation phrase, and refuses duplicate versions on PyPI and GitHub (`publish.yaml:27-77`). It holds `id-token: write` in exactly one job (`publish.yaml:259-261`) and uses no API token. The PyPI Trusted Publisher is configured, and **PEP 740 publish attestations exist** (verified).
- **Wheels are built from the sdist.** The manylinux wheel comes from cibuildwheel on the sdist; the Windows wheel comes from the extracted sdist with a fresh `CARGO_TARGET_DIR`. Then auditwheel repair runs, each installed wheel is smoke-tested without Rust, the exact three-file set is checked, and `SHA256SUMS` and `SOURCE_COMMIT.txt` are written. The PyPI hashes equal `SHA256SUMS` (verified).
- **The Linux host build is bit-for-bit reproducible across machines.** The locally qualified candidate host and the CI-published host have the same SHA-256 (`be08dd88…`). Keep the manylinux container build, the pinned toolchain, `--locked` and `.gitattributes eol=lf`.
- **The PowerShell module promotes, it does not rebuild.** It reuses the exact released host binaries after checksum verification (`publish-powershell.yaml:41-63`) and runs Pester against the staged package on Linux, PS 7 and PS 5.1. `HOST-SOURCE-MANIFEST.json` records every hash.
- **The host has minimal runtime linkage.** It links only libc, libm and libgcc_s, and dlopens Wayland, xkb and fontconfig. A clean `python:3.10-slim` or `python:3.14-slim` container runs the published wheel headless (verified).
- **Vendoring discipline is good.** "Pristine" commits are byte-identical to the crates.io packages (verified), patch rationale is documented in `third_party/README.md`, and the Stylo and Taffy patch stacks are verified by `patches/*/verify.sh` (both pass).
- **The verifier reports honestly.** `tools/verify.py` keeps exact argv, exit codes and logs, kills process groups on its watchdog, and labels `"native_desktop_qualified": false`. `native_acceptance.py` defaults to BLOCKED, needs an evidence file plus its hash, and binds the report to a source digest.
- **Renderer tests use targeted pixel and semantic probes** instead of snapshot blobs.
- **Workflow defaults are least-privilege.** `permissions: contents: read` sits at the top level, the default workflow token is read-only, and most release checkouts use `persist-credentials: false`.
- **The published artifacts work on clean machines.** The PyPI wheel works on Python 3.10 and 3.14, and the PSGallery module works on pwsh 7.4 (CurrentUser scope), both in headless mode.

## 3. Findings

### RELEASE-01: Publishing is not gated on CI, and 1.0.0 shipped from a commit whose Windows CI failed
- **Classification:** Confirmed defect (process). Proven from run history.
- **Component:** `.github/workflows/publish.yaml`, `publish-powershell.yaml`, GitHub repo settings.
- **Evidence:**
  - Run 43484564824 (CI "Host verification", `d053e09`, started 18:38:17Z): `verify (windows-2022)` failed in the python stage (`FAIL: test_agent_locations_are_distinct_and_portable`, `FAIL: test_checked_manifest_matches_entire_source`; `job-win-d053e09.log:2615,2623`).
  - Run 43484619109 (`Publish UI4ME Core to PyPI and GitHub`, **same SHA d053e09**) was dispatched at 18:38:38Z, 21 s after CI started and 20 min before CI went red. It published 1.0.0 to PyPI and created tag `v1.0.0` → `d053e09`.
  - The next commit, `a4d4614` ("Make the release tooling tests pass on Windows CI"), fixed the tests.
  - Neither publish workflow queries check runs for `source_sha`.
  - Branch protection and rulesets are unavailable (HTTP 403, free plan, private repo). The `pypi` and `psgallery` environments have no required reviewers, and admins can bypass.
  - All 34 commits were pushed straight to `main`. No PR was ever merged.
- **Reproduction:** `gh run list --repo ui4me/ui4me-core --limit 30` and compare rows 43484564824 and 43484619109. `gh api repos/ui4me/ui4me-core/environments`.
- **Severity:** High. **Likelihood:** High (it already happened once, and nothing enforces the gate).
- **Impact:** A release can be cut from an untested or red commit. Here the failures were test-only (Windows 8.3 path and manifest sort order), so the 1.0.0 artifacts were probably not defective. That was luck, not control. Anyone with write access can publish to PyPI with no second person involved.
- **Root cause:** The gate is documentation only (`docs/release/publishing.md:7`, "All gates … pass"). The plan has no protection features, and the workflow has no CI-status check.
- **Remediation:**
  1. In `publish.yaml` `preflight` (after line 54) and `publish-powershell.yaml` `build-and-check`, add a step that fails unless every required check run for `$SHA` concluded `success`. For example, `gh api repos/$GITHUB_REPOSITORY/commits/$SHA/check-runs --paginate --jq '.check_runs[]|[.name,.conclusion]|@tsv'` must contain `verify (ubuntu-24.04)\tsuccess` and `verify (windows-2022)\tsuccess`, plus the PowerShell CI jobs once RELEASE-08 makes them unconditional. Keep the list of required names in one file, e.g. `tools/release/required_checks.txt`.
  2. Alternatively, have `publish.yaml` call `ci.yml` as a reusable workflow (`on: workflow_call`) and `needs:` it. This is more robust but adds ~25 min of runner time per publish.
  3. Organization: either make the repo public, which gives free branch protection, rulesets, required reviewers on environments and tag protection, or move to GitHub Team. Then require PRs plus status checks on `main`, add a required reviewer on `pypi` and `psgallery`, and set `can_admins_bypass: false`.
- **Test:** Extend `tools/test_publish_workflow.py` after RELEASE-15 to assert, semantically via YAML parsing, that the preflight job has a CI-status step and that `pypi` needs it.
- **Alternatives/tradeoffs:** A manual "wait for CI" checklist is what exists now, and it failed. A reusable CI call costs minutes, while the check-run query is cheap but depends on CI having run for that SHA (push CI always runs).
- **Complexity:** S for the workflow; the organization change is a decision rather than engineering work. **Confidence:** High.

### RELEASE-02: The committed `SOURCE_FILES.sha256` is a change-detector in the unit suite. Every non-release change fails CI and hides clippy
- **Classification:** Confirmed defect (CI design).
- **Component:** `tools/test_source_manifest.py:14-17`, `tools/release/refresh_source_manifest.py`, `tools/release/SOURCE_FILES.sha256` (1,472 entries), `.github/workflows/ci.yml:28-31`.
- **Evidence:**
  - `test_checked_manifest_matches_entire_source` runs `refresh_source_manifest.py --check`, which hashes every Git-visible file.
  - PR #3 run 43625326001 (and 43625321431) and PR #4 run 43632418264 (and 43632413038) all fail with `FAIL: test_checked_manifest_matches_entire_source` → `python: FAIL`, even though fmt, check, test, headless, renderer and faults all PASS (`pr3-linux.log:3022-3035`).
  - Because the `Verify…` step fails, the next step, **`Lints` (clippy), is `skipped` on both OSes** (`gh run view 43625326001 --json jobs` shows `Lints skipped`). So no PR has ever had a clippy signal.
  - `git_visible_files()` uses `git ls-files -co --exclude-standard` (`refresh_source_manifest.py:129`), so any stray untracked file in a contributor's tree also fails the test locally.
  - The manifest header hard-codes "UI4ME Core 1.0.1" (`:164`).
- **Reproduction:** Push any one-line source change: CI goes red. Locally, `touch notes.txt && python3 tools/release/refresh_source_manifest.py --check` exits 1 (follows from the code: an untracked, non-ignored file is included).
- **Severity:** High. **Likelihood:** Certain on every PR.
- **Impact:** CI is permanently red for real work, so reviewers learn to ignore red. The clippy gate is silently lost. The draft fixes for #1 and #4 cannot show a green build without also committing a "release" manifest, which is the action the docs say must only happen at release time (`docs/release/building.md:27`).
- **Root cause:** The manifest is a legacy of archive-based delivery. `provenance-1.0.0.json` records an input zip with no `.git`, and `verify_package.py`'s docstring says "Verify the delivered source manifest before importing". In a Git repository the commit or tree hash already is a tamper-evident manifest, and an unsigned in-tree hash list adds no security: whoever edits a file can regenerate it. Placing a release-freeze check in the per-commit unit suite turned it into a change-detector.
- **Remediation (recommended design):**
  1. Delete `test_checked_manifest_matches_entire_source`. Keep the generator unit tests (`:19-51`).
  2. Stop committing `SOURCE_FILES.sha256`. In `publish.yaml` `preflight`, generate it from **tracked files only** at `source_sha`: change `git_visible_files` to `git ls-files -z` and also require `git status --porcelain` to be empty. Put `git rev-parse HEAD^{tree}` in `SOURCE_COMMIT.txt`, inject the manifest into the sdist (generate it before `python -m build --sdist`), and attach it to the GitHub release. Cover it with a build-provenance attestation (RELEASE-09).
  3. Update `verify_source.py:84` and `verify_linux_artifact.py:93` (the sdist must contain the generated file) and `docs/release/{building,publishing}.md`.
  4. Make lint independent of test failure: either add a `clippy` stage to `tools/verify.py` (so `--keep-going` covers it) or set `if: ${{ !cancelled() }}` on the `Lints` step.
- **Minimal alternative:** Keep the committed file but run `--check` only in the publish workflows, which already run `verify_package.py`, and in CI only when `tools/release/**` changes.
- **Test:** A CI self-test: a PR that changes only a `.rs` comment must be green. Also a unit test that an untracked file does not change the manifest.
- **Complexity:** S-M. **Confidence:** High.

### RELEASE-03: The published Windows host binary was never natively qualified, and the release notes contradict the qualification record
- **Classification:** Confirmed defect (evidence and provenance).
- **Component:** `docs/release/RELEASE_NOTES_1.0.1.md:38`, `tools/release/records/qualification-1.0.1.json:5,15`, `publish.yaml` windows-wheel job.
- **Evidence:**
  - The release notes say "Qualified … with release-candidate artifacts built from one sdist".
  - The record says `"windows_wheel_host": "prebuilt from Rust sources identical to the candidate commit … instead of recompiled from the sdist extraction, because the Proxmox test node rebooted twice"`.
  - The published `ui4me_core-1.0.1-py3-none-win_amd64.whl` contains `ui4me-host.exe` with SHA-256 `8d7eab92c21bdb4e…`, while the qualified Windows host is `4f9b4b47f42f7201…`.
  - Linux, by contrast, is identical (`be08dd88…` in both).
  - Native Windows checks ran against a binary that was never published. The published binary got only CI's headless `smoke_installed_wheel.py`.
  - The source of the shipped code is identical between candidate `3e50227` and release `c6fd71d`: the diff touches only records, notes, the manifest and one `print` in the smoke script (verified with `git diff --stat`).
- **Reproduction:** `gh release download v1.0.1 --pattern '*.whl'`, then hash `ui4me_core/_bin/ui4me-host*` inside each wheel and compare with `qualification-1.0.1.json`.
- **Severity:** Medium. **Likelihood:** Every release under the current process (rebuild-after-qualify).
- **Impact:** The "qualified" label does not cover what Windows users run. MSVC builds are not reproducible across paths and machines, so compiler, linker or environment differences go unexamined. The notes overstate the evidence.
- **Root cause:** The process qualifies local candidates, and then CI rebuilds them for publication.
- **Remediation:** Switch to promote-not-rebuild, as PSGallery already does:
  1. Split `publish.yaml` into a build workflow and a promote workflow. The build workflow builds the sdist, wheels, `SHA256SUMS` and attestations and uploads them as a **draft** GitHub release or a run artifact.
  2. Native qualification downloads exactly those artifacts. `native_acceptance.py` already records `tested_host.sha256`; make the record list the artifact hashes.
  3. The promote workflow takes the build `run_id` as input, re-verifies the hashes against the committed qualification record, and uploads those exact files to PyPI.
  4. Until then, correct the release-notes sentence.
- **Test:** In the promote workflow, `assert sha256(wheel) in qualification_record['artifacts']['files'].values()` for every file.
- **Alternatives:** Making Windows builds reproducible (e.g., `--remap-path-prefix`, a fixed `CARGO_TARGET_DIR`, deterministic linker flags `/Brepro`) is a longer project. Promotion is simpler.
- **Complexity:** M. **Confidence:** High.

### RELEASE-04: The PowerShell Gallery module source is not tied to the release tag. UI4ME.Core 1.0.0 was built from a different commit than v1.0.0
- **Classification:** Confirmed defect (provenance).
- **Component:** `publish-powershell.yaml:22-73`.
- **Evidence:**
  - The workflow checks out the current `main` HEAD and passes `--source-identity "$(git rev-parse HEAD)"` (`:71`), but it only checks that release `v$VERSION` *exists* (`:48`). It never checks that HEAD equals the tag's commit.
  - Run 43512337180 (PSGallery 1.0.0) ran at `023859c`, while `v1.0.0` points to `d053e09`. `git diff --stat d053e09 023859c` shows 96 files changed, +11,022 lines: the whole PowerShell module was added after the tag.
  - The 1.0.0 nupkg is attached to the v1.0.0 GitHub release, whose `SOURCE_COMMIT.txt` names a commit that does not contain the module.
  - For 1.0.1 both happen to be `c6fd71d`, by convention only.
- **Reproduction:** `gh run list` (row 43512337180) and `git ls-remote --tags origin`.
- **Severity:** Medium. **Likelihood:** Medium.
- **Impact:** A Gallery version cannot be reproduced from its tag, and the provenance chain (tag → source → package) is broken for the Gallery channel.
- **Remediation:**
  1. In `build-and-check`, resolve `TAG_SHA=$(git ls-remote origin "refs/tags/v$VERSION^{}" …)`, falling back to the lightweight ref, and `test "$(git rev-parse HEAD)" = "$TAG_SHA"`. Or check out `ref: v${{ inputs.version }}` explicitly instead of `main`.
  2. Record `source_identity` = the tag SHA in `HOST-SOURCE-MANIFEST.json`.
  3. Document that a module-only fix needs a new patch version and tag.
- **Complexity:** S. **Confidence:** High.

### RELEASE-05: All public package metadata and docs links point to a private repository (404 for users)
- **Classification:** Confirmed defect (release metadata).
- **Component:** `pyproject.toml:33-38`, `docs/publish/pypi.md` (PyPI description), `powershell/UI4ME.Core/UI4ME.Core.psd1:42-43`, `build_psgallery.py` (`standalone()` turns repo-relative links into absolute links), README CI badge.
- **Evidence:** Unauthenticated `curl` returns 404 for `https://github.com/ui4me/ui4me-core`, `/blob/main/docs/index.md` and `/issues`. The PyPI `project_urls` (Homepage, Documentation, Source, Issues, Changelog) all point there, and the PyPI description contains 10 such links. The Gallery `LicenseUri` and `ProjectUri` point there too.
- **Severity:** Medium (Low if the repo is about to be made public). **Likelihood:** Certain today.
- **Impact:** Users of a "Production/Stable" package have no reachable documentation, license URL, changelog or issue tracker. The PSGallery LicenseUri is broken.
- **Remediation:** Decide on visibility. If the repo stays private, point metadata at a public docs site or Pages site, and at the sdist-contained LICENSE. Add a check to the publish preflight that every URL in `project.urls` and `PrivateData.PSData.*Uri` returns 200 when fetched without credentials.
- **Complexity:** S. **Confidence:** High.

### RELEASE-06: A Linux AllUsers install of UI4ME.Core cannot be used by non-root users (host stays non-executable)
- **Classification:** Confirmed defect (packaging). Reproduced.
- **Component:** `powershell/UI4ME.Core/UI4ME.Core.psm1:1315-1319` (`chmod u+x` at first use). The nupkg format drops Unix modes.
- **Evidence:** In `mcr.microsoft.com/powershell:latest` (pwsh 7.4.2):
  - `Install-Module UI4ME.Core -RequiredVersion 1.0.1 -Scope AllUsers` as root installs `…/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host` with mode `-rw-r--r-- root`.
  - A non-root `Get-UI4MEHostPath` then prints `chmod: … Operation not permitted` and throws "The native host is not executable and could not be made executable".
  - After root runs the module once, the mode is `-rwxr--r--` (`u+x` only), so non-root use **still fails**.
  - A CurrentUser install works (headless `Start-UI4MEScript` exits 0).
- **Reproduction:** `exp/RELEASE/psg/run.sh` (shown in §1 #9). Run it with `docker run --rm -v <dir>:/psg:ro mcr.microsoft.com/powershell:latest sh /psg/run.sh`.
- **Severity:** Medium (the docs only show `-Scope CurrentUser` for install, but AllUsers is the PSResourceGet/PowerShellGet norm on shared machines, CI images and containers). **Likelihood:** High for those setups.
- **Remediation:** In `Get-UI4MEHostPath`:
  1. If the file is executable, use it.
  2. Otherwise, if the file is owned by the current user, run `chmod a+rx` (not `u+x`).
  3. Otherwise, copy the host to a per-user cache (`$XDG_CACHE_HOME/ui4me/host/<sha256>/ui4me-host`), verify its SHA-256 against `HOST-SOURCE-MANIFEST.json`, `chmod 0755` the copy, and use it.

  Add a Linux CI job that installs the staged nupkg with AllUsers as root and runs Pester as a non-root user.
- **Alternatives:** A post-install hook does not exist for PSGallery modules. Executing through `ld-linux` is fragile (breaks `/proc/self/exe`).
- **Complexity:** S-M. **Confidence:** High.

### RELEASE-07: The version is hard-coded in about 61 files, including the workflows and verifiers. Every release edits the release machinery itself
- **Classification:** Maintenance problem (proven from code).
- **Component:**
  - `publish.yaml` (11 occurrences, e.g. `:49 assert version == '1.0.1'`, `:99,140,145,177-184`)
  - `publish-powershell.yaml` (7, `:36 test "$VERSION" = 1.0.1`)
  - `release-linux.yml:33,41` and `release-windows.yml:31`
  - `tools/release/{verify_source.py:77, verify_publish_dist.py:19, build_psgallery.py:21, smoke_installed_wheel.py:14-19, verify_linux_artifact.py, verify_windows_artifact.py, verify_psgallery.py, refresh_source_manifest.py:164, build_windows.ps1}`
  - tests (`tools/test_scaffold.py`, `test_run_example.py`, `test_public_docs.py`, `powershell/tests/Unit.Tests.ps1`)
  - 15 example manifests and scripts
- **Evidence:** `git grep -c '1\.0\.1'` gives 61 non-doc files. The memory note says the 1.0.0 workflows needed the same edits.
- **Severity:** Medium. **Likelihood:** High.
- **Impact:** A release requires a large reviewed diff to CI and release code. A missed spot fails only at publish time. And the workflow that published 1.0.1 cannot re-run a dry build of 1.0.2.
- **Remediation:**
  1. Make `Cargo.toml [workspace.package].version` the single source.
  2. Add `tools/release/version.py` (read it) and have every verifier import it; the workflows compute filenames from `inputs.version` after asserting that it equals the source.
  3. Keep literal versions only where the format forces them (`pyproject.toml`, `__init__.py`, `client.py`, `.psd1`, `.psm1`) and update them with a `tools/release/bump_version.py X.Y.Z`. `verify_source.py` remains the consistency check.
  4. Make examples version-agnostic (`-MinimumVersion 1.0`, no `version=` in example `app.toml`) so they do not churn.
- **Test:** `test_version_single_source`: grep that no workflow, verifier or test file under `tools/release` contains a literal `\d+\.\d+\.\d+` other than through `version.py`.
- **Complexity:** M. **Confidence:** High.

### RELEASE-08: CI coverage is narrower than the support claims
- **Classification:** Reproducible risk / test gap.
- **Component:** `ci.yml`, `test-powershell.yaml`.
- **Evidence:**
  - **Python:** CI uses only 3.12 (`ci.yml:24-26`; publish smoke tests 3.12 only), yet the classifiers and docs claim 3.10–3.14 (`pyproject.toml:22-26`). I verified 3.10 and 3.14 manually (§1 #8), so the current artifacts work, but nothing prevents a 3.11+-only construct from landing.
  - **PowerShell:** `ci.yml` never runs Pester. `test-powershell.yaml` builds no host, so the 4 real-host `DirectStart` tests are skipped on every OS (Pester summary "Passed: 50, Skipped: 4" in run 43551153035). Its path filter (`:6,9`) excludes `crates/**` and `python/**`, so host or protocol changes never trigger the PowerShell CI. PowerShell against the real host is exercised only inside `publish-powershell.yaml`, i.e. after PyPI is already published.
  - **GUI:** No Wayland compositor or Windows UI session test exists in CI. Every window, input and accessibility behaviour is covered only by lab ledgers (§1 inventory).
  - **Windows:** Windows 10/11 is claimed but not qualified. `docs/guides/platforms.md:14` states this honestly, but `install-python.md:11` lists it as supported without the caveat.
- **Severity:** Medium. **Likelihood:** Medium.
- **Remediation:**
  1. Add a cheap Linux matrix job: build the host once, then `uv run --python {3.10,3.11,3.13,3.14} python -m unittest tools/test_bootstrap.py tools/test_application.py tools/test_reference_client.py` with `UI4ME_HOST_BINARY=target/debug/ui4me-host`.
  2. In `ci.yml`, after the host build, run Pester (pwsh 7 on both OSes and 5.1 on Windows) with `UI4ME_HOST_BINARY` set, so DirectStart runs on every push. Then delete the path filter, or add `crates/**` and `python/**`.
  3. Prototype a Linux GUI smoke under `weston --backend=headless` (or a headless `sway`/`cage`): open a window, wait for the first presented frame, and query AT-SPI over a private D-Bus.
  4. Prototype a Windows smoke on `windows-2022`: GitHub-hosted Windows runners have an interactive desktop, so test window creation, the UIA tree via `tools/windows/uia_probe_matrix.py` and a click via SendInput. This last item is a plausible approach that needs verification.
- **Complexity:** S for items 1-2; M-L for item 3/4 GUI smoke. **Confidence:** High for the gaps, Med for GUI-in-CI feasibility.

### RELEASE-09: Supply-chain hygiene is below the bar for a binary-shipping stable package
- **Classification:** Mixed. The unpinned actions and template injection are confirmed (zizmor). The rest are optional improvements, rated per item.
- **Component:** All workflows, release tooling, repo settings.
- **Evidence:**
  - **Actions pinned by tag or branch, not SHA:** 42 `unpinned-uses`. This includes `pypa/gh-action-pypi-publish@release/v1` (a *branch*) in the only job holding `id-token: write` (`publish.yaml:268`). SHA pinning is not required by repo policy.
  - **Node 20:** the deprecation annotation is present on every run.
  - **Unpinned build inputs:** `cibuildwheel>=2.21`, `build>=1.2`, `twine>=6`, `wheel>=0.44` (`publish.yaml:93,139,175`). The `manylinux_2_34` image is pulled by floating tag. rustup is installed with `curl -fsSL https://sh.rustup.rs | sh` inside the manylinux container (`:118`), and `Install-Module Pester` / `PSResourceGet` use `-MinimumVersion` with `-SkipPublisherCheck`.
  - **No dependency scanning in CI:** no `cargo deny`/`cargo audit` and no `deny.toml`. A local run gives 0 vulnerabilities and 4 unmaintained advisories (RUSTSEC-2026-0192 `ttf-parser` via `fontdue`, which `ui4me-host` uses directly at `crates/ui4me-host/Cargo.toml:37`; RUSTSEC-2026-0370/0371/0372 zbus wire crates).
  - **No Dependabot config:** Dependabot alerts are on, but they cannot see the path-vendored stylo, taffy, winit and softbuffer.
  - **No SBOM**, although `cargo-cyclonedx` and `cargo-about` are installed locally.
  - **No GitHub build-provenance attestation** for the release assets (404). There is no Authenticode signature on `ui4me-host.exe` or the PS module (`SIG # Begin signature block` count 0). Tags are lightweight (`git/refs/tags/v1.0.1` → `commit`) and unsigned, and releases are mutable (`immutable: false`).
  - **Template injection:** `${{ inputs.version }}` is interpolated into a shell in a `contents: write` job (`publish-powershell.yaml:215-216`). The input was earlier validated to equal `1.0.1`, and only writers can dispatch, so the real risk is low.
  - **Artipacked:** `ci.yml`, `release-linux.yml` and `release-windows.yml` check out with persisted credentials and upload artifacts. `verification/` does not contain `.git`, so this is low risk.
- **Severity:** Medium overall (the floating `release/v1` publisher action is the highest-value item). **Likelihood:** Low per item.
- **Remediation:**
  1. Pin all `uses:` to full SHAs with a version comment and add `.github/dependabot.yml` for `github-actions`, `cargo` and `pip`.
  2. Upgrade to Node-24 action majors.
  3. Pin build tools with hashes in `tools/release/requirements-release.txt` (`pip install --require-hashes`). Pin the manylinux image by digest. Verify `rustup-init` by SHA-256, or use the image's own rustup.
  4. Add a `deny.toml` and a CI job running `cargo deny check advisories bans sources licenses`, ignoring the 4 unmaintained advisories with written rationale and a follow-up issue to replace `fontdue` with skrifa/parley.
  5. Add `actions/attest-build-provenance` for the sdist, wheels and nupkg, and a CycloneDX SBOM (`cargo cyclonedx` for the host plus the wheel inventory) attached to the release.
  6. Create annotated tags before publishing, enable immutable releases (requires the draft-then-publish ordering in RELEASE-13), and Authenticode-sign the Windows host and the module if a certificate is available.
  7. Replace the `${{ }}` interpolation with `env: VERSION`.
  8. Run zizmor and actionlint in CI.
- **Complexity:** M (spread over many small items). **Confidence:** High.

### RELEASE-10: Release evidence is not durable or verifiable, and records reference deleted files
- **Classification:** Confirmed maintenance problem.
- **Component:** `tools/release/records/*.json`, commit 805bc86.
- **Evidence:**
  - `provenance-1.0.0.json` references `docs/STATUS.md`, `docs/RC2_LINUX_FINAL.md`, `docs/RC2_IMPLEMENTATION_NOTES.md`, `docs/verification/…/`, `docs/SOURCE_FILES.sha256` and `LOCAL_FINISH_PROMPT.md` (`:13,16-21,51-52,92`). All are missing at HEAD. 805bc86 deleted 446 files, including all `docs/verification/**`, and `LOCAL_FINISH_PROMPT.md` was never committed.
  - `qualification-1.0.1.json` is a hand-written summary ("4/4 PASS", "28/28 PASS") with no raw logs, no evidence hashes and no `native_acceptance.py` report attached.
  - `capability-scope-1.0.json` is for `1.0.0-rc.1` and states "no native pass is asserted here" for 38 rows.
  - The qualification lab (root uinput daemon at hard-coded `/run/user/1000/…`, KWin scripting, SSH to a Proxmox VM through a private config) cannot be reproduced by a third party.
- **Severity:** Medium. **Likelihood:** n/a (already true).
- **Impact:** Nobody, including the maintainers, can later verify what was qualified. The records read as evidence but are assertions. And the capability ledger can be mistaken for current status.
- **Remediation:**
  1. Make the qualification record machine-generated: `native_acceptance.py` (schema 2), the Pester `Qualify.ps1` output and `verify.py` `report.json` merged by a `tools/release/make_record.py` that embeds the artifact hashes and the evidence-file SHA-256s.
  2. Store the raw evidence (logs, screenshots, AT-SPI/UIA dumps) as a release asset `qualification-evidence-X.Y.Z.tar.zst` instead of in-tree, so it survives doc cleanups and does not bloat the repo.
  3. Mark `capability-scope-1.0.json` as historical, or regenerate it.
  4. Fix or remove the dangling references.
  5. Add a test that every path referenced by a record either exists or is a release-asset URL.
- **Complexity:** M. **Confidence:** High.

### RELEASE-11: The stability claim has no supporting policy (classifier, compatibility, changelog, security reporting)
- **Classification:** Design tradeoff / maintenance problem.
- **Component:** `pyproject.toml:18,38`, docs.
- **Evidence:**
  - The package declares `Development Status :: 5 - Production/Stable` three days after the first commit (8365c26 on 2026-11-12).
  - 1.0.0 and 1.0.1 shipped about 9 h apart, with two open defects (#1, where a backend disconnect becomes an app failure; #2, unbounded RSS growth) filed after 1.0.1.
  - No semver, compatibility or deprecation policy exists for the Python and PowerShell APIs or the manifest format. Only the protocol handshake is described (`docs/reference/protocol.md:3`).
  - There is no `CHANGELOG.md`. `RELEASE_NOTES_1.0.0.md` was deleted from the tree, and the PyPI "Changelog" URL is pinned to `RELEASE_NOTES_1.0.1.md` (and is private, see RELEASE-05).
  - There is no `SECURITY.md` or vulnerability-reporting channel for a host that runs local backends behind a token-authenticated IPC.
  - There is no documented yank or rollback procedure (`git grep -i yank` finds nothing).
  - Issues carry no labels or milestones.
- **Severity:** Medium (expectation mismatch for adopters). **Likelihood:** n/a.
- **Remediation:**
  1. Add `docs/release/versioning.md` stating SemVer for the public Python and PowerShell APIs and the manifest schema, the protocol version policy, the supported-platform policy, and the deprecation window.
  2. Add a cumulative `CHANGELOG.md` (restore the 1.0.0 entry from git `v1.0.0:docs/release/RELEASE_NOTES_1.0.0.md`) and point `project.urls.Changelog` at it.
  3. Add `SECURITY.md`.
  4. Document the yank procedure (PyPI yank plus a GitHub release note; PSGallery unlist) and the "next patch" rule.
  5. Consider `4 - Beta` for the next release until a defect-triage gate exists (no open High-severity bug at release), or keep 5 and adopt the gate.
- **Complexity:** S. **Confidence:** High.

### RELEASE-12: The repository process does not support review
- **Classification:** Maintenance problem.
- **Component:** Repo settings, history.
- **Evidence:**
  - `.github/` contains only workflows: no `CODEOWNERS`, PR template, issue templates or `dependabot.yml`.
  - All 34 commits were pushed directly to `main` by one account. Every message carries a Co-Authored-By trailer, and every commit is unsigned (`%G? = N`).
  - Very large single commits: `05b8f8f` "Import supplied UI4ME Core 0.2 source" (1,195 files, +480,514 lines, mostly vendored Stylo but also +319 renderer files), `db91445` (+17,070), `33e1317` (+15,039), `670356f` (+15,322), `023859c` (+10,968), `805bc86` (−55,323).
  - Merge, squash and rebase are all allowed, `deleteBranchOnMerge` is false, and there is a stray `LuaRocks` environment.
- **Severity:** Low-Medium. **Likelihood:** n/a.
- **Impact:** The history cannot be reviewed or bisected inside imports, so blame and bisect stop at "import". The PR-based workflow that the draft PRs (#3, #4) start to use is not enforced.
- **Remediation:**
  - Going forward: PR-only `main` (requires RELEASE-01's plan or visibility change), squash-merge only, delete-branch-on-merge, CODEOWNERS for `crates/ui4me-host/src/transport.rs`, `.github/`, `tools/release/` and `third_party/`, and PR and issue templates with a "native-qualification needed?" checkbox.
  - Do **not** rewrite history (tags and PyPI provenance reference these SHAs). Instead, document in `docs/guides/development.md` which commits are imports.
  - Delete the `LuaRocks` environment if it is unused.
- **Complexity:** S. **Confidence:** High.

### RELEASE-13: Publish ordering makes the tag and GitHub release trail PyPI, and the PowerShell asset is appended later
- **Classification:** Design tradeoff with a recoverable failure mode.
- **Component:** `publish.yaml:252-319`, `publish-powershell.yaml:198-216`.
- **Evidence:**
  - `github-release` has `needs: [preflight, pypi]`. The tag is created by `gh release create --target $SOURCE_SHA` only after the PyPI upload, so if that job fails, PyPI holds a version with no tag. `publishing.md:31` documents rerunning the job, which works.
  - The PowerShell workflow later `gh release upload`s the nupkg into the already-published release. That would break if immutable releases were enabled, and it means the release's `SHA256SUMS` does not cover the nupkg.
  - No staging (TestPyPI or draft release) exists before the irreversible upload.
- **Severity:** Low. **Likelihood:** Low.
- **Remediation:**
  1. Create an annotated tag and a **draft** release with all assets first.
  2. Upload to PyPI from the draft's assets.
  3. Publish the release.
  4. For PSGallery, attach the nupkg plus its hash to the draft before publishing, or publish a separate `SHA256SUMS-powershell`.
  5. Optionally add a TestPyPI dry-run job on `workflow_dispatch` with `dry_run: true`.
- **Complexity:** S-M. **Confidence:** High.

### RELEASE-14: CI is slow and wasteful: no cache, duplicate runs, no cancellation, no job timeout
- **Classification:** Simplification and optimization opportunity.
- **Component:** `ci.yml`.
- **Evidence:**
  - The workflow runs on `on: push` (every branch and tag) **and** `pull_request`, so each PR push runs 4 jobs (e.g. 43625321431 plus 43625326001).
  - There is no `concurrency` and no `timeout-minutes` (the default is 360 min). The Actions cache holds 0 bytes because there is no rust-cache, so Stylo is cold-built every run.
  - Durations for run 43551153042: Linux 19 min, Windows 26 min.
  - The org is on the free plan with a private repo, where Windows minutes are billed at 2×. I could not query usage (needs `admin:org`).
- **Severity:** Low. **Likelihood:** High.
- **Remediation:**
  - Use `on: push: branches: [main]` plus `pull_request`.
  - Add `concurrency: {group: ci-${{ github.ref }}, cancel-in-progress: ${{ github.event_name == 'pull_request' }}}` and `timeout-minutes: 75`.
  - Add `Swatinem/rust-cache` (pinned by SHA) keyed on `Cargo.lock` plus `rust-toolchain.toml`, with `save-if: github.ref == 'refs/heads/main'`.
  - Keep `CARGO_BUILD_JOBS=2`, which matches 2-vCPU private-repo runners.
- **Complexity:** S. **Confidence:** High for the facts; Med for the speedup (expect about 40-60% less on warm cache, not measured).

### RELEASE-15: Release-tooling tests and docs that assert form, not behaviour
- **Classification:** Simplification opportunity / minor doc defect.
- **Component:** `tools/test_publish_workflow.py:15-34`, `tools/release/verify_source.py:88-89`, `docs/release/building.md:77`.
- **Evidence:**
  - `test_publishing_never_runs_on_push_or_tag` asserts substrings like `'needs: [preflight, pypi]'` and `"'publish-' + version"`. Reformatting the YAML breaks it, and semantically wrong YAML that keeps those strings passes it.
  - `verify_source.py` fails if the bytes `--force` appear in `pyproject.toml` or `setup.py`, a check that tests nothing meaningful.
  - `building.md:77` claims `verify_publish_dist.py` "checks that both wheels came from the sdist's sources". It does not: `verify_publish_dist.validate` only validates each file independently (`:33-39`), with no wheel-to-sdist content comparison. The workflow's structure provides the property, the verifier does not.
- **Severity:** Low. **Likelihood:** n/a.
- **Remediation:**
  - Parse the workflows with `yaml.safe_load` and assert on the structure (triggers, `jobs.pypi.needs`, `permissions`, `environment.name`). Add actionlint and zizmor (RELEASE-09).
  - Either implement the cross-check in `verify_publish_dist.py` (every `ui4me_core/*.py` in each wheel byte-equals `python/ui4me_core/*.py` in the sdist, and `Version` in METADATA equals the sdist's PKG-INFO) or fix the sentence in `building.md`.
  - Drop the `--force` check.
- **Complexity:** S. **Confidence:** High.

### RELEASE-16: A test assumes `XDG_RUNTIME_DIR` is set, and assets tests synchronize with sleeps
- **Classification:** Confirmed environment-dependent test failure (reproduced). Sleeps are a plausible flakiness concern.
- **Component:** `tools/test_bootstrap.py:258-260`, `crates/ui4me-host/src/platform/linux.rs:71`, `crates/ui4me-assets/tests/{shutdown.rs:96,114-117,231-236, lifecycle.rs:121,538, svg.rs:392}`.
- **Evidence:**
  - In a clean `debian:13` container (root, no `XDG_RUNTIME_DIR`), `test_temporary_manifest_is_removed` fails: `[PosixPath('/tmp/ui4me direct …/tmp/ui4me-runtime-0')] != []`. The host's fallback `std::env::temp_dir().join("ui4me-runtime-{uid}")` lands in the test's TMPDIR. This passes on CI and the KDE workstation only because `XDG_RUNTIME_DIR` is set there.
  - Several assets tests use fixed sleeps (e.g. 400 ms "let in-flight work finish") before asserting. There are 99 `Duration::from_*` uses in host src and tests. CI history shows no flake from these: the earlier main failures were deterministic Windows defects (`core1_tests.rs:242`, `ui4me-assets/build.rs:33`).
- **Reproduction:** `exp/RELEASE/contributor.sh` (see `contributor.log`).
- **Severity:** Low. **Likelihood:** Medium for contributors in containers and minimal CI images.
- **Remediation:**
  - In the test, either set `XDG_RUNTIME_DIR` to a private temp dir in `run_app`'s env, or assert only that no `*.toml` or temp manifest remains.
  - Decide whether a fallback runtime dir under `/tmp` should be removed at exit (product question for the host area).
  - Replace the fixed sleeps with polling helpers that have a deadline (such as `settle()`, which already exists in `tests/support/mod.rs`).
- **Complexity:** S. **Confidence:** High for the first; Med for the sleeps.

### RELEASE-17: A developer environment variable can silently put the 345 MB debug host into a wheel
- **Classification:** Reproducible risk (proven from code, not run).
- **Component:** `setup.py:33-37`, `docs/guides/development.md:136`, `tools/release/verify_linux_artifact.py:36`.
- **Evidence:**
  - `development.md` tells contributors to `export UI4ME_HOST_BINARY="$PWD/target/debug/ui4me-host"`.
  - `setup.py` packages whatever `UI4ME_HOST_BINARY` points to instead of building the release profile.
  - The verifiers only check the ELF magic and size > 1 MiB, while `target/debug/ui4me-host` is 344,750,648 bytes.
  - `build_linux.sh` and `python -m build` in such a shell therefore produce a wheel with a debug host that "passes". The CI publish path does not set the variable, so published artifacts are unaffected.
- **Severity:** Low. **Likelihood:** Medium for local release candidates.
- **Remediation:**
  - Rename the build override to `UI4ME_WHEEL_HOST_BINARY`, separate from the runtime override.
  - Have `setup.py` refuse a host whose `--version` output lacks a release marker. Add a `--build-info` flag to the host that prints the profile, or check for the absence of `.debug_info` sections and a size below 100 MB.
  - Make `verify_*_artifact.py` assert the same.
- **Complexity:** S. **Confidence:** High.

### RELEASE-18: Build recipes are duplicated across workflows, scripts and docs
- **Classification:** Simplification opportunity.
- **Component:** `release-linux.yml`, `release-windows.yml`, `tools/release/build_windows.ps1`, `docs/release/building.md:47-53`, `publish.yaml:109-203`.
- **Evidence:**
  - The cibuildwheel environment block is copied verbatim three times.
  - Windows is built inline in `publish.yaml` but through `build_windows.ps1` in `release-windows.yml`.
  - `release-linux.yml` builds the sdist itself, so it never exercises the artifact-handoff path used by publishing.
- **Severity:** Low.
- **Remediation:**
  - Move the build into a reusable workflow `build-dist.yml` (`on: workflow_call` and `workflow_dispatch`). `publish.yaml` would call it and then promote. Delete `release-linux.yml` and `release-windows.yml`.
  - Move `CIBW_*` settings into `[tool.cibuildwheel]` in `pyproject.toml` so the docs say "run `cibuildwheel`".
- **Complexity:** S-M. **Confidence:** High.

### RELEASE-19: Heavy and periodic verification exists but is never scheduled
- **Classification:** Test gap.
- **Component:**
  - `#[ignore]` evidence tests: `handles_misuse.rs:156` (1M ops), `shutdown.rs:110` (1,000 iterations), `bombs.rs:22`
  - fuzz harnesses (`fuzz_gate.rs`, `fuzz_resources.rs`)
  - `patches/{stylo,taffy}/verify.sh`, which pass locally but are not in CI. CI has no Stylo git checkout, and `Cargo.toml:31` uses path dependencies
  - `tools/measure_resources.py` (relevant to issue #2)
- **Severity:** Low-Medium.
- **Remediation:**
  - Add `nightly.yml` (`schedule:` plus `workflow_dispatch`, Linux only) with these steps:
    - `cargo test --release -- --ignored`
    - time-boxed fuzz harness runs as documented in `fuzz_gate.rs`
    - `git clone --filter=blob:none https://github.com/servo/stylo && git -C stylo checkout 37c1ed3b…`, then `UI4ME_UPSTREAM_SOURCE=… bash patches/stylo/verify.sh` (same for taffy)
    - a headless soak from `measure_resources.py` with an RSS-slope threshold (would catch #2)
  - Open an issue automatically on failure.
- **Complexity:** M. **Confidence:** High.

## 4. Test gaps and the highest-value tests to add

Ordered by value per cost:

1. **CI-status gate in the publish preflight** (RELEASE-01). Validate it with a dry-run dispatch on a commit whose CI failed: it must refuse.
2. **Remove the manifest change-detector, run clippy independently** (RELEASE-02). Self-test: a comment-only PR is green.
3. **Pester with the real host in `ci.yml`** on pwsh 7 (Linux and Windows) and PS 5.1 with `UI4ME_HOST_BINARY` set, without a path filter (RELEASE-08). Today the 4 real-host direct-start tests never run before publishing.
4. **Python version smoke matrix** (3.10, 3.11, 3.13, 3.14) running `test_bootstrap`, `test_application` and `test_reference_client` against a host built once (RELEASE-08).
5. **Post-publish clean-machine verification workflow**, which turns my experiments into CI:
   - `python:{3.10,3.14}-slim`: `pip install ui4me-core==X` and `smoke_installed_wheel.py`, plus hash comparison with `SHA256SUMS`.
   - `mcr.microsoft.com/powershell`: CurrentUser install with a headless `Start-UI4MEScript`, **plus an AllUsers-as-root then run-as-user scenario** (RELEASE-06).
   - Windows: `pip install` and `Install-Module` on `windows-2022`.
6. **Artifact-promotion check**: the hash of each published file must equal a hash in the committed qualification record (RELEASE-03).
7. **Tag/source consistency in `publish-powershell`**: HEAD must equal `v$VERSION^{}` (RELEASE-04).
8. **Regression tests for the open defects, run headless:**
   - Issue #1: a backend subscribed to nothing receives a burst of more than the outbound queue size (e.g. 1,000 transition events from a theme switch). The connection must survive and the host must not log a disconnect.
   - Issue #2: a 60 s headless soak that inserts and removes a `font-weight:bold` subtree at 50 Hz. The RSS slope must stay below a bound in `measure_resources.py` (nightly).
9. **Workflow linting** (actionlint plus zizmor), and structural YAML tests replacing the substring tests (RELEASE-15).
10. **`cargo deny` in CI** with explicit ignores (RELEASE-09).
11. **Nightly ignored, fuzz and patch-verify jobs** (RELEASE-19).
12. **GUI smoke prototypes** (needs feasibility work): weston-headless on Linux with first-frame and AT-SPI checks; `windows-2022` window creation with a UIA probe. These would be the first automated evidence for any native-window behaviour.

## 5. Recommend NOT changing

- **The manual-dispatch, confirmation-phrase publishing model and the OIDC scoping.** Add the CI gate (RELEASE-01) but keep the shape.
- **Building wheels from the sdist, and promoting binaries into the PS module.** Extend promotion to PyPI (RELEASE-03) rather than replacing it.
- **One `py3-none-<platform>` wheel per OS.** The host is a standalone executable with no CPython ABI, so a cp310–cp314 cibuildwheel matrix would only add cost. Test the interpreter versions instead (RELEASE-08).
- **No MSRV job.** All crates are `publish = false` (`Cargo.toml:12`, `crates/ui4me-host/Cargo.toml:8`), the toolchain is pinned in `rust-toolchain.toml`, and `rust-version = 1.99` simply documents it. An MSRV CI job would test a configuration nobody builds.
- **The `manylinux_2_34` baseline.** It matches Wayland-era distributions, and the binary links only libc, libm and libgcc_s. Going lower buys nothing for a Wayland-only host.
- **Pixel-probe renderer tests over golden images.** They are specific and diagnosable. Do not convert them to snapshot blobs.
- **The honest BLOCKED-by-default acceptance tooling and `"native_desktop_qualified": false`.** Keep them, and feed their output into machine-generated records (RELEASE-10).
- **Vendored-patch structure**: pristine commit, then patch commits, a README rationale, and `verify.sh`. Only schedule the verify scripts (RELEASE-19).
- **History.** Do not rewrite it to split the imports: tags v1.0.0 and v1.0.1 and the PyPI and Sigstore provenance reference these SHAs.
- **`CARGO_BUILD_JOBS=2` in CI.** It is appropriate for 2-vCPU private-repo runners and avoids out-of-memory failures on Stylo.

## 6. Release gates (what a stable release of this kind should require)

| # | Gate | Status | Evidence |
|---|---|---|---|
| G1 | Release SHA has green required CI on every supported OS, **enforced** by the workflow | **FAILING** | 1.0.0 published from d053e09 while Windows CI failed (RELEASE-01). No check in `publish*.yaml`. |
| G2 | `main` protected: PRs, required checks, review; publish environments need a reviewer | **FAILING** | HTTP 403 on protection and rulesets (free plan). Environments have only a branch policy. 0 merged PRs. |
| G3 | CI signal is meaningful: ordinary changes can be green, and lint always runs | **FAILING** | Manifest test fails every PR, and clippy is skipped (RELEASE-02). |
| G4 | Artifacts are built in CI from the tagged commit | **MET** (PyPI and GitHub) / **FAILING** (PSGallery 1.0.0) | `SOURCE_COMMIT.txt` equals the tag. PS 1.0.0 was built from 023859c, not d053e09 (RELEASE-04). |
| G5 | Published binaries are the ones that were natively qualified | **MET** (Linux) / **FAILING** (Windows) | Linux host hashes are identical. Windows `8d7eab92…` ≠ `4f9b4b47…` (RELEASE-03). |
| G6 | Publisher identity and provenance attestations | **MET** (PyPI PEP 740) / **FAILING** (GitHub assets, PSGallery) | Integrity API returns a publisher bundle. Attestations API returns 404. The module is unsigned. |
| G7 | SBOM shipped | **FAILING** | None. |
| G8 | Dependency vulnerability and licence scan in CI | **FAILING** (not in CI) / content **MET** today | Local `cargo deny`: 0 vulnerabilities, 4 unmaintained. |
| G9 | Workflows hardened (SHA pins, no injection, current runtimes) | **FAILING** | zizmor: 42 unpinned, 2 template-injection. Node 20 warnings. |
| G10 | Every claimed Python version tested | **UNVERIFIED in CI** / MET manually | CI tests 3.12 only. I verified 3.10 and 3.14 headless in containers. |
| G11 | Every claimed PowerShell edition tested against the real host before publishing | **FAILING** pre-publish / MET in publish workflow | 4 DirectStart tests are skipped in all CI. They run only inside `publish-powershell`. I verified pwsh 7.4.2 headless (CurrentUser). |
| G12 | Every documented install mode works | **FAILING** | Linux AllUsers install is unusable by non-root (reproduced, RELEASE-06). |
| G13 | Native GUI behaviour verified reproducibly (automated or by retained evidence) | **UNVERIFIED** | Only private-lab ledgers. Raw evidence was deleted from the tree (RELEASE-10). Not reproducible by third parties. |
| G14 | Claimed OS versions qualified | **UNVERIFIED** (documented) | Windows 10/11 and non-KDE Wayland are not run, which `platforms.md:14` discloses. |
| G15 | A fresh contributor can build and test from the docs | **MET** (with one hermeticity bug) | Debian 13 clean container: host built in 4 min 41 s, 119 of 120 tool tests pass (RELEASE-16), headless example passes. |
| G16 | Version single-sourced; a release does not edit the release machinery | **FAILING** | 61 files contain literal `1.0.1`, including workflow assertions (RELEASE-07). |
| G17 | Cumulative changelog and release notes that are accurate against the evidence | **FAILING** | No CHANGELOG. The 1.0.0 notes were deleted. The 1.0.1 notes contradict the record on the Windows sdist build (RELEASE-03/11). |
| G18 | Compatibility, semver, support and security policy published | **FAILING** | None beyond the protocol handshake. No SECURITY.md. |
| G19 | Public metadata links resolve | **FAILING** | All PyPI and Gallery URLs return 404 (private repo, RELEASE-05). |
| G20 | Rollback plan and immutability | **PARTIAL** | PyPI and PSGallery versions are immutable. There is no yank or unlist procedure. GitHub releases are mutable and tags are lightweight and unprotected. |
| G21 | Known High-severity defects triaged before release (defect gate) | **UNVERIFIED** | #1 and #2 were found after 1.0.1. There are no labels or milestones and no release checklist item. |
| G22 | Vendored and patched dependencies verified against upstream | **MET manually** / not in CI | `verify.sh` passes for stylo and taffy, and the pristine `third_party` commits equal the crates.io packages (verified). |
| G23 | Clean-machine smoke of the *published* artifacts | **MET** (by this audit, not by process) | PyPI wheels on 3.10 and 3.14 and the PSGallery CurrentUser install pass headless. |
| G24 | Secrets least-privilege | **MET** | Only `PSGALLERY_API_KEY`, scoped to the `psgallery` environment with a `main` policy. PyPI uses OIDC. The default token is read-only. |

## 7. Open questions

1. Is the repository intended to become public? That would solve G2, G19 and tag protection at no cost, and it changes the priority of RELEASE-05 and RELEASE-12.
2. What are the Actions minutes budget and usage on the free plan? `gh api` needs `admin:org`, so I could not check. Windows is billed at 2× and every PR push currently costs about 4 jobs × 20-26 min.
3. Are the PyPI project owners 2FA-protected, with a single owner or several? Are the PSGallery API key's scope (package-limited?) and expiry known? Neither is visible from here.
4. Is raw native qualification evidence for 1.0.1 archived anywhere outside the repo? If so, link it from the record.
5. Can a GitHub-hosted `windows-2022` runner run the UIA and SendInput subset (`tools/windows/uia_probe_matrix.py`) reliably? This needs one experimental run.
6. Should the host remove a fallback `/tmp/ui4me-runtime-<uid>` that it created? That is a product decision (host area); it determines the right fix for RELEASE-16.
