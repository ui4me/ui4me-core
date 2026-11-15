# F-22 / T11 / T12 / RELEASE-18: local runs of the new CI jobs (2026-11-15)

Nothing can be dispatched on GitHub from this workstream; each new job was run locally with the same script and pinned image.

## clean wheel (python:3.10-slim, python:3.14-slim, pinned digests)

Wheel: locally built `ui4me_core-1.0.1-py3-none-linux_x86_64.whl` packaging the published 1.0.1 release host via `UI4ME_WHEEL_HOST_BINARY`. Command per image: `docker run --rm -v $PWD:/src:ro -v <wheels>:/wheels:ro python:3.1x-slim@sha256:… sh -c "pip install --no-index --no-deps /wheels/*.whl && cd /tmp && python /src/tools/release/smoke_installed_wheel.py"` → both `PASS installed Python wheel, native host, direct start, …` (exit 0). This also proves `tools/release/version.py` works on Python 3.10.

## powershell allusers (linux): `tools/release/smoke_psmodule_allusers.sh` in mcr.microsoft.com/powershell:7.4-ubuntu-22.04@sha256:62300a21…

Module: source `powershell/UI4ME.Core` plus the published 1.0.1 Linux host. Baseline result (RELEASE-06 reproduced, exit 1):

```
Packed: UI4ME.Core.1.0.1.nupkg
Installed (AllUsers): /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1
-rw-r--r-- 1 root root 41029064 Nov 15 03:05 /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
chmod: changing permissions of '/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host': Operation not permitted
Exception: The native host is not executable and could not be made executable:
/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_co
re/_bin/ui4me-host
-rw-r--r-- 1 root root 41029064 Nov 15 03:05 /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
FAIL: a non-root user cannot use the AllUsers install (exit 1)
```

Positive control (same script with one added `chmod a+rx` of the installed host, not committed): `PS-APP-CONNECTED` / `PASS: AllUsers install as root, headless direct start as a non-root user`. The job turns green once workstream E fixes `Get-UI4MEHostPath`.

## Pester with a real host: `tools/release/run_pester.ps1`

* pwsh 7.4.6 container, published 1.0.1 Linux host, `XDG_RUNTIME_DIR` set: `Pester 5.9.0: passed 54, failed 0, skipped 0, not run 0` (the 4 real-host DirectStart tests ran; on GitHub CI they were skipped in every run: "Passed: 50, Skipped: 4", run 43551153035).
* Same without `XDG_RUNTIME_DIR` (root container): 3 DirectStart failures, `ui4me-host: runtime directory is too long for a Unix-domain socket` (the host falls back to a runtime folder under the long Pester TestDrive TMPDIR). Not a CI risk (GitHub runners set `XDG_RUNTIME_DIR`), but the PowerShell analogue of RELEASE-16; reported to the lead for the host/binding owners.
* Windows PowerShell 5.1 and pwsh on Windows: not run (no Windows here); needs the lead.

## Linux release wheel from the sdist with the new `[tool.cibuildwheel]` config (RELEASE-18) and the hash-pinned tools

`python -m build --sdist`, then `python -m cibuildwheel --platform linux --output-dir out ui4me_core-1.0.1.tar.gz` (cibuildwheel 4.3.5 from requirements-release.txt, `CIBW_ENVIRONMENT` overridden only to use 4 build jobs). rustup-init pinned by SHA-256: `/tmp/rustup-init: OK`. Result:

```
1 wheel produced in 5 minutes
  cp312-manylinux_x86_64: ui4me_core-1.0.1-py3-none-manylinux_2_34_x86_64.whl 16.4 MB in 4 minutes, SHA256=8346475aa30706a768ad8148216345147efef4e6dab86dab52698e281075e4bf
embedded ui4me_core/_bin/ui4me-host sha256 be08dd8837158694c0a348f3a2a54b8815edcff648424e7630667fc8d64e9136 (41029064 bytes)
```

The embedded host is **bit-for-bit identical to the published 1.0.1 Linux host** (`be08dd88…`, audit §1 #7): the consolidated build definition, the pinned rustup-init and the pinned release tools reproduce the release. `verify_linux_artifact.py` (with the new debug-host check) passes on it.

## Structural workflow tests against the baseline workflows

`tools/test_workflows.py` with the baseline `.github/workflows` restored from 1e49a6a:

```
FAILED (failures=12, errors=6)
```

On this branch: 22/22 pass. Eight hand mutations (drop the clippy `!cancelled()`, remove the CI gate, add `${{ inputs.version }}` to a script, unpin one action, let pypi skip the draft, drop Python 3.10 from the matrix, add a path filter, rename a required job) are each caught by at least one test.
