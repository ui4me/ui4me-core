[CmdletBinding()]
param([switch]$InstallBuildTools)
$ErrorActionPreference = 'Stop'
$repoDir = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repoDir
$vcLocator = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$hasVc = $false
if (Test-Path -LiteralPath $vcLocator) {
    $found = & $vcLocator -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $hasVc = [bool]$found
}
if (-not $hasVc) {
    if (-not $InstallBuildTools) {
        throw 'MSVC C++ build tools are missing. Run this script with -InstallBuildTools in an elevated provisioning session, then resume as the development user.'
    }
    $installer = Join-Path $env:TEMP ('ui4me-vs-buildtools-' + [guid]::NewGuid().ToString('N') + '.exe')
    Invoke-WebRequest -UseBasicParsing -Uri 'https://aka.ms/vs/17/release/vs_buildtools.exe' -OutFile $installer
    try {
        $process = Start-Process -FilePath $installer -ArgumentList '--quiet','--wait','--norestart','--nocache','--add','Microsoft.VisualStudio.Workload.VCTools','--includeRecommended' -Wait -PassThru
        if ($process.ExitCode -notin @(0,3010)) { throw "Build-tools installer failed: $($process.ExitCode)" }
        if ($process.ExitCode -eq 3010) { throw 'Build tools installed; Windows requires a restart before validation.' }
    } finally { Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue }
}
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    $installer = Join-Path $env:TEMP ('ui4me-rustup-' + [guid]::NewGuid().ToString('N') + '.exe')
    Invoke-WebRequest -UseBasicParsing -Uri 'https://win.rustup.rs/x86_64' -OutFile $installer
    try {
        & $installer -y --profile minimal --default-toolchain none
        if ($LASTEXITCODE -ne 0) { throw 'Rustup installation failed' }
    } finally { Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue }
    $cargoDir = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
    $env:PATH = "$cargoDir;$env:PATH"
}
$toolchain = [regex]::Match((Get-Content -Raw -LiteralPath 'rust-toolchain.toml'), 'channel\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $toolchain) { throw 'Cannot read pinned Rust toolchain' }
& rustup toolchain install $toolchain --profile minimal --component rustfmt --component clippy
if ($LASTEXITCODE -ne 0) { throw 'Toolchain installation failed' }
if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    throw 'Python 3.11+ is required for the reference client. Reuse an existing approved interpreter (including an all-users install on managed Server 2022), make it available on PATH, then rerun.'
}
& python -c 'import sys; assert sys.version_info >= (3, 11), sys.version'
if ($LASTEXITCODE -ne 0) { throw 'A working Python 3.11+ interpreter is required; Windows Store aliases are insufficient.' }
$env:CARGO_BUILD_JOBS = '2'
& cargo build --locked -p ui4me-host
if ($LASTEXITCODE -ne 0) { throw 'Host build failed' }
Write-Host 'Build complete. Launch tools/run_renderer.py inside the development user RDP session. tools/run_example.py retains the v0.1 host probe.'
