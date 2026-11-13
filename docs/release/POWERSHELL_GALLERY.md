# Publishing UI4ME.Core to the PowerShell Gallery

The `UI4ME.Core` PowerShell module is published separately from the Python package, by the
manually dispatched workflow `.github/workflows/publish-powershell.yaml`. No push or tag
publishes anything. `publish.yaml` (PyPI) is unrelated and unchanged.

## What the package contains

| Path | Content |
| --- | --- |
| `UI4ME.Core.psd1`, `UI4ME.Core.psm1` | Module manifest and PowerShell implementation (ASCII) |
| `private/UI4ME.Transport.cs` | C# helper compiled with `Add-Type` on first use (C# 5, .NET Framework 4.7.2+ and .NET 6+) |
| `runtimes/linux-x64/ui4me_core/_bin/ui4me-host` | Native host taken unchanged from `ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl` |
| `runtimes/win-x64/ui4me_core/_bin/ui4me-host.exe` | Native host taken unchanged from `ui4me_core-1.0.0-py3-none-win_amd64.whl` |
| `runtimes/*/ui4me_core/_bin/THIRD_PARTY_LICENSES.txt`, `licenses/` | License texts from the wheels |
| `skills/ui4me-powershell/` | Coding-agent skill installed by `Install-UI4MESkill` |
| `docs/` | Guide and command reference (links point to GitHub) |
| `HOST-SOURCE-MANIFEST.json` | Wheel and host SHA-256, source commit, SHA-256 of every packaged file |

The wheels contain no relocated shared libraries: the Linux host links only glibc
(`libc`, `libm`, `libgcc_s`), and the Windows host only system DLLs. NuGet extraction drops
Unix execute permissions; `Get-UI4MEHostPath` restores the bit on first use.

No Python or Rust is needed to install or run the module. Python is needed only at
build time to run the staging scripts.

## Preconditions

- GitHub release `v1.0.0` exists with both wheels and `SHA256SUMS` (it does; published by
  `publish.yaml`).
- The PowerShell sources on `main` passed the gates in
  [`docs/verification/powershell-1.0/`](../verification/powershell-1.0/README.md):
  Pester on Linux pwsh, Windows pwsh and Windows PowerShell 5.1, and the native
  qualification runs on both desktops.
- GitHub environment `psgallery` holds the secret `PSGALLERY_API_KEY`: a PowerShell Gallery
  API key scoped to **Push new packages and package versions** for the glob `UI4ME.Core`
  only. The environment is restricted to the `main` branch. Never print or commit the key.
- `UI4ME.Core` must be free on the Gallery or owned by the publishing account.

## Stage and verify locally (no publication)

```bash
gh release download v1.0.0 --repo ui4me/ui4me-core --dir wheels \
  --pattern '*manylinux_2_34_x86_64.whl' --pattern '*win_amd64.whl' --pattern SHA256SUMS
(cd wheels && sha256sum -c SHA256SUMS --ignore-missing)
python tools/release/build_psgallery.py \
  --linux-wheel wheels/ui4me_core-1.0.0-py3-none-manylinux_2_34_x86_64.whl \
  --windows-wheel wheels/ui4me_core-1.0.0-py3-none-win_amd64.whl \
  --source-identity "$(git rev-parse HEAD)" --out dist/psgallery
python tools/release/verify_psgallery.py dist/psgallery/UI4ME.Core
UI4ME_TEST_MODULE=$PWD/dist/psgallery/UI4ME.Core/UI4ME.Core.psd1 pwsh -c 'Invoke-Pester powershell/tests'
```

Only real release wheels may be used as input. The staging script refuses an output folder
that is not empty and checks the wheel tag, executable signature and size.

## Publish

1. GitHub → Actions → **Publish UI4ME.Core to PowerShell Gallery** → Run workflow on `main`
   with `version = 1.0.0` and `confirmation = publish-powershell-1.0.0`.
2. The workflow downloads the released wheels, checks them against `SHA256SUMS`, stages and
   verifies the module, runs Pester against the staged package on Linux pwsh, Windows pwsh
   and Windows PowerShell 5.1, packs it with `Compress-PSResource`, publishes the `.nupkg`
   with the `psgallery` key and attaches it to the existing `v1.0.0` release (the Python
   assets are not modified).
3. Afterwards, from a clean user profile on each platform:

   ```powershell
   Find-PSResource UI4ME.Core -Repository PSGallery
   Install-Module UI4ME.Core -Scope CurrentUser
   New-UI4MEProject -Path ./check -AppId org.example.Check
   Start-UI4MEHost -Manifest ./check/app.toml
   ```

   Compare `Get-FileHash (Get-UI4MEHostPath)` with `HOST-SOURCE-MANIFEST.json`, and record the
   Gallery URL, `.nupkg` SHA-256 and workflow run in `docs/STATUS.md`.

A Gallery version cannot be overwritten. A fix requires a new module version (and a new
`ModuleVersion` in the manifest and workflow guard).
