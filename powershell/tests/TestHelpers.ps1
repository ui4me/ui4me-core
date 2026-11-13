# Dot-sourced by the Pester suites (Pester 5+).
# UI4ME_TEST_MODULE tests a staged or installed module instead of the source tree.
$script:ModuleManifest = if ($env:UI4ME_TEST_MODULE) { (Resolve-Path $env:UI4ME_TEST_MODULE).Path } else { (Resolve-Path (Join-Path $PSScriptRoot '../UI4ME.Core/UI4ME.Core.psd1')).Path }
$script:TestIsWindows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT

function Start-MockHost {
    $token = [guid]::NewGuid().ToString('N')
    $endpoint = if ($script:TestIsWindows) { '\\.\pipe\ui4me-test-' + $token } else { Join-Path ([IO.Path]::GetTempPath()) ('ui4me-' + $token.Substring(0, 12) + '.sock') }
    $ready = Join-Path ([IO.Path]::GetTempPath()) ('ui4me-ready-' + $token)
    $exe = (Get-Process -Id $PID).Path
    $mock = Join-Path $PSScriptRoot 'MockHost.ps1'
    $arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $mock + '"'),
        '-Endpoint', ('"' + $endpoint + '"'), '-ReadyFile', ('"' + $ready + '"'), '-ModulePath', ('"' + $script:ModuleManifest + '"'))
    $log = Join-Path ([IO.Path]::GetTempPath()) ('ui4me-mock-' + $token + '.log')
    $launch = @{ FilePath = $exe; ArgumentList = $arguments; PassThru = $true; RedirectStandardError = $log }
    if ($script:TestIsWindows) { $launch.WindowStyle = 'Hidden' }
    $process = Start-Process @launch
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while (-not (Test-Path -LiteralPath $ready) -and [DateTime]::UtcNow -lt $deadline -and -not $process.HasExited) { Start-Sleep -Milliseconds 25 }
    if (-not (Test-Path -LiteralPath $ready)) {
        $detail = if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Raw } else { '' }
        throw "Mock host failed to start: $detail"
    }
    return [pscustomobject]@{ Process = $process; Endpoint = $endpoint; Ready = $ready; Log = $log }
}

function Stop-MockHost($Mock) {
    if ($null -eq $Mock) { return }
    if (-not $Mock.Process.WaitForExit(5000)) { Stop-Process -Id $Mock.Process.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $Mock.Ready, $Mock.Log -Force -ErrorAction SilentlyContinue
}

function Wait-Until([scriptblock]$Condition, $Application, [int]$Seconds = 5) {
    $end = [DateTime]::UtcNow.AddSeconds($Seconds)
    while (-not (& $Condition) -and [DateTime]::UtcNow -lt $end) { [void](Wait-UI4ME -Application $Application -TimeoutMilliseconds 20) }
    return [bool](& $Condition)
}
