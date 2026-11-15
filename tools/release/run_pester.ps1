<#
.SYNOPSIS
Run the UI4ME.Core Pester suites with a pinned Pester and fail on any skip.

.DESCRIPTION
Used by CI and the PowerShell publish workflow (RELEASE-08 / F-22). The four
DirectStart tests need a real native host and are skipped without one, which
used to happen in every CI run unnoticed. This runner fails when more than
-MaxSkipped tests were skipped, so "real host" is enforced, not assumed.
Tests tagged PosixOnly (signals, chmod) are excluded on Windows instead of
being counted as skips.

-HostBinary sets UI4ME_HOST_BINARY (a source build); -TestModule sets
UI4ME_TEST_MODULE (a staged or installed module whose bundled host is used).
Works on PowerShell 7 and Windows PowerShell 5.1.
#>
[CmdletBinding()]
param(
    [string]$HostBinary,
    [string]$TestModule,
    [int]$MaxSkipped = 0,
    [string]$PesterVersion = '5.9.0'
)
$ErrorActionPreference = 'Stop'
Write-Output ("PowerShell {0} ({1})" -f $PSVersionTable.PSVersion, $PSVersionTable.PSEdition)
$pester = Get-Module -ListAvailable Pester | Where-Object { $_.Version -eq [version]$PesterVersion } | Select-Object -First 1
if (-not $pester) {
    Install-Module Pester -RequiredVersion $PesterVersion -Repository PSGallery -Scope CurrentUser -Force -SkipPublisherCheck
}
Import-Module Pester -RequiredVersion $PesterVersion
if ($HostBinary) {
    if (-not (Test-Path -LiteralPath $HostBinary -PathType Leaf)) { throw "Host binary not found: $HostBinary" }
    $env:UI4ME_HOST_BINARY = (Resolve-Path -LiteralPath $HostBinary).Path
}
if ($TestModule) {
    $env:UI4ME_TEST_MODULE = (Resolve-Path -LiteralPath $TestModule).Path
}
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$config = New-PesterConfiguration
$config.Run.Path = Join-Path $root 'powershell/tests'
$config.Run.PassThru = $true
$config.Output.Verbosity = 'Detailed'
if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
    $config.Filter.ExcludeTag = @('PosixOnly')
}
$result = Invoke-Pester -Configuration $config
Write-Output ("Pester {0}: passed {1}, failed {2}, skipped {3}, not run {4}" -f $PesterVersion,
    $result.PassedCount, $result.FailedCount, $result.SkippedCount, $result.NotRunCount)
if ($result.FailedCount -gt 0 -or $result.Result -ne 'Passed') { throw 'Pester tests failed' }
if ($result.SkippedCount -gt $MaxSkipped) {
    throw "$($result.SkippedCount) tests were skipped (allowed: $MaxSkipped); the real-host tests must run"
}
