# Build the release source distribution and the Windows x64 wheel (from that
# sdist). This script never publishes packages.
[CmdletBinding()]
param([string]$Python = 'python', [string]$OutDir = 'dist')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..\..')
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust toolchain is required for compiled wheels' }
function Invoke-Checked { param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '2' }
Invoke-Checked $Python @('-m', 'pip', 'install', '--upgrade', 'build>=1.2', 'setuptools>=77', 'wheel>=0.44', 'twine>=6', 'packaging>=24')
# The wheel is built FROM the sdist: proves the source archive alone is a
# complete build input (no checkout-only files).
Invoke-Checked $Python @('-m', 'build', '--outdir', $OutDir)
Invoke-Checked $Python (@('-m', 'twine', 'check') + (Get-ChildItem $OutDir -Filter 'ui4me_core-1.0.1*' | ForEach-Object FullName))
Invoke-Checked $Python @('tools/release/verify_windows_artifact.py', $OutDir)
Write-Output "Windows wheel and source artifacts are in $OutDir; nothing was uploaded."
