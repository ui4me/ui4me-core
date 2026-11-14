# Start-UI4MEScript: launcher/backend roles, manifests and lifecycle (Pester 5+).
# The integration cases run real scripts through the real native host in its headless
# test mode (UI4ME_TEST_MODE=1). They need a host binary: UI4ME_HOST_BINARY, or a
# cargo build of this repository; otherwise they are skipped.
BeforeDiscovery {
    function Find-TestHost {
        # UI4ME_HOST_BINARY, else a host bundled in the staged module under test
        # (UI4ME_TEST_MODULE), else a cargo build of this repository.
        $windows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
        $name = if ($windows) { 'ui4me-host.exe' } else { 'ui4me-host' }
        if ($env:UI4ME_HOST_BINARY) { return $env:UI4ME_HOST_BINARY }
        $candidates = @()
        if ($env:UI4ME_TEST_MODULE) {
            $rid = if ($windows) { 'win-x64' } else { 'linux-x64' }
            $candidates += Join-Path (Split-Path -Parent $env:UI4ME_TEST_MODULE) "runtimes/$rid/ui4me_core/_bin/$name"
        }
        foreach ($mode in 'release', 'debug') { $candidates += Join-Path $PSScriptRoot "../../target/$mode/$name" }
        foreach ($candidate in $candidates) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return [IO.Path]::GetFullPath($candidate) }
        }
        return $null
    }
    $script:SkipNative = -not (Find-TestHost)
}

BeforeAll {
    . (Join-Path $PSScriptRoot 'TestHelpers.ps1')
    Import-Module $script:ModuleManifest -Force -ErrorAction Stop
    & (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
    $name = if ($script:TestIsWindows) { 'ui4me-host.exe' } else { 'ui4me-host' }
    $script:HostBinary = $env:UI4ME_HOST_BINARY
    if (-not $script:HostBinary -and $env:UI4ME_TEST_MODULE) {
        # Let the staged module find (and make executable) its own bundled host.
        $script:HostBinary = Get-UI4MEHostPath
    }
    if (-not $script:HostBinary) {
        foreach ($mode in 'release', 'debug') {
            $candidate = Join-Path $PSScriptRoot "../../target/$mode/$name"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { $script:HostBinary = [IO.Path]::GetFullPath($candidate); break }
        }
    }
    $script:Shell = (Get-Process -Id $PID).Path

    function Invoke-DirectScript {
        param([string]$Body, [string[]]$Arguments = @(), [hashtable]$Environment = @{})
        $folder = Join-Path $TestDrive ('direct ü ' + [guid]::NewGuid().ToString('N').Substring(0, 6))
        [void](New-Item -ItemType Directory -Path $folder)
        $script = Join-Path $folder 'app.ps1'
        $text = $Body.Replace('__MODULE__', $script:ModuleManifest.Replace("'", "''"))
        [IO.File]::WriteAllText($script, $text, (New-Object Text.UTF8Encoding($true)))
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = $script:Shell
        $info.UseShellExecute = $false
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $info.StandardOutputEncoding = New-Object Text.UTF8Encoding($false)
        $info.StandardErrorEncoding = New-Object Text.UTF8Encoding($false)
        $info.WorkingDirectory = $folder
        $all = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $script) + $Arguments
        $info.Arguments = (@($all | ForEach-Object { [UI4ME.HostProcess]::QuoteWindowsArgument($_) }) -join ' ')
        foreach ($k in @($info.EnvironmentVariables.Keys)) { if ($k -like 'UI4ME_*') { $info.EnvironmentVariables.Remove($k) } }
        $info.EnvironmentVariables['UI4ME_TEST_MODE'] = '1'
        $info.EnvironmentVariables['UI4ME_HOST_BINARY'] = $script:HostBinary
        # A private temporary folder: the launcher's manifest must appear and vanish only there.
        $temp = Join-Path $folder 'tmp'
        [void](New-Item -ItemType Directory -Path $temp)
        foreach ($k in 'TMPDIR', 'TEMP', 'TMP') { $info.EnvironmentVariables[$k] = $temp }
        foreach ($k in $Environment.Keys) { $info.EnvironmentVariables[$k] = $Environment[$k] }
        $process = [Diagnostics.Process]::Start($info)
        $stderr = $process.StandardError.ReadToEndAsync()
        $stdout = $process.StandardOutput.ReadToEnd()
        if (-not $process.WaitForExit(120000)) { $process.Kill(); throw 'Direct-start script did not finish' }
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Out = $stdout; Err = $stderr.Result
            Leftovers = @(Get-ChildItem -LiteralPath $temp -Force | ForEach-Object Name) }
    }
}

Describe 'Start-UI4MEScript roles and validation' {
    It 'is exported with help' {
        (Get-Command Start-UI4MEScript -Module UI4ME.Core).Name | Should -Be 'Start-UI4MEScript'
        (Get-Help Start-UI4MEScript).Synopsis | Should -Match 'native host'
    }
    It 'refuses incomplete or looping host environments' {
        $saved = @{}
        foreach ($k in 'UI4ME_ENDPOINT', 'UI4ME_TOKEN', 'UI4ME_DIRECT_START') { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
        try {
            foreach ($case in @(@{ UI4ME_ENDPOINT = 'x' }, @{ UI4ME_TOKEN = 'x' }, @{ UI4ME_DIRECT_START = '1' })) {
                foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $null) }
                foreach ($k in $case.Keys) { [Environment]::SetEnvironmentVariable($k, $case[$k]) }
                { Start-UI4MEScript -ScriptPath $PSCommandPath } | Should -Throw
            }
        } finally {
            foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
        }
    }
    It 'validates options before starting anything' {
        { Start-UI4MEScript -AppId 'noperiod' } | Should -Throw
        { Start-UI4MEScript -AppId 'org.1bad' } | Should -Throw
        { Start-UI4MEScript -Name "tab`tname" } | Should -Throw
        { Start-UI4MEScript -Instance many } | Should -Throw
        { Start-UI4MEScript -MaxWindows 0 } | Should -Throw
    }
    It 'writes TOML strings that keep Unicode, quotes and backslashes' {
        $module = Get-Module UI4ME.Core
        $value = & $module { ConvertTo-UI4METomlString ('C:\Zoë\"q"' + [char]9 + '😀') }
        $value | Should -BeExactly ('"C:\\Zoë\\\"q\"\u0009😀"')
    }
    It 'derives a stable, valid identity per script' {
        $module = Get-Module UI4ME.Core
        $a = & $module { Get-UI4MEDefaultAppId -ScriptPath '/home/a/app.ps1' -Label 'Café app' }
        $b = & $module { Get-UI4MEDefaultAppId -ScriptPath '/home/b/app.ps1' -Label 'Café app' }
        $a | Should -Match '^local\.ui4me\.Cafe-app\.h[0-9a-f]{12}$'
        $a | Should -Not -Be $b
        (& $module { Get-UI4MEDefaultAppId -ScriptPath '/x.ps1' -Label '日本語' }) | Should -Match '^local\.ui4me\.app\.h'
        (& $module { Get-UI4MEDefaultAppId -ScriptPath '/x.ps1' -Label '123' }) | Should -Match '^local\.ui4me\.app-123\.h'
    }
    It 'quotes Windows command-line arguments like CommandLineToArgvW' {
        [UI4ME.HostProcess]::QuoteWindowsArgument('plain') | Should -BeExactly 'plain'
        [UI4ME.HostProcess]::QuoteWindowsArgument('') | Should -BeExactly '""'
        [UI4ME.HostProcess]::QuoteWindowsArgument('a b') | Should -BeExactly '"a b"'
        [UI4ME.HostProcess]::QuoteWindowsArgument('say "hi"') | Should -BeExactly '"say \"hi\""'
        [UI4ME.HostProcess]::QuoteWindowsArgument('C:\dir with space\') | Should -BeExactly '"C:\dir with space\\"'
    }
    It 'relays host records as plain text' {
        $out = New-Object IO.StringWriter; $err = New-Object IO.StringWriter
        [UI4ME.HostProcess]::RelayLine('{"level":"INFO","fields":{"message":"backend output","source":"backend","stream":"stdout","text":"Příliš"}}', $false, $out, $err)
        [UI4ME.HostProcess]::RelayLine('{"level":"INFO","fields":{"message":"window created"}}', $false, $out, $err)
        [UI4ME.HostProcess]::RelayLine('{"level":"ERROR","fields":{"message":"application failed","reason":"x"}}', $false, $out, $err)
        [UI4ME.HostProcess]::RelayLine('ui4me-host: plain failure', $false, $out, $err)
        $out.ToString().Trim() | Should -BeExactly 'Příliš'
        $lines = @($err.ToString().Trim() -split "\r?\n")
        $lines | Should -Be @('ui4me-host error: application failed (reason=x)', 'ui4me-host: plain failure')
    }
}

Describe 'Start-UI4MEScript with the real native host (headless test mode)' -Skip:$script:SkipNative {
    BeforeAll {
        $script:AppBody = @'
param([string]$Greeting = 'none', [string]$Other)
$ErrorActionPreference = 'Stop'
Import-Module '__MODULE__'
# Exact Unicode on every platform: in a process without a console, Windows PowerShell's
# own output (Write-Output, Write-Host) uses a legacy code page; [Console]::Out follows
# the UTF-8 encoding that Start-UI4MEScript selects for the app process.
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
"ROLE $(if ($env:UI4ME_ENDPOINT) { 'backend' } else { 'launcher' })"
$app = Start-UI4MEScript -Name 'Direct test' -Instance multiple
[Console]::Out.WriteLine("PARAMS [$Greeting] [$Other] secret=$([bool]$env:UI4ME_TOKEN)")
$window = New-UI4MEWindow -Application $app -Title 'Direct test'
$doc = Open-UI4MEHtml -Window $window -Html "<p id='t'>Hello Příliš</p>"
[Console]::Out.WriteLine("TEXT $(Get-UI4MEDocumentText -Document $doc -Element t)")
if ($Greeting -eq 'crash') { throw 'deliberate failure' }
Set-UI4MEReady -Application $app
$null = Request-UI4MEQuit -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
[Console]::Error.WriteLine('ORDERLY')
'@
    }
    It 'runs the script twice, keeps its arguments and exits cleanly' {
        $r = Invoke-DirectScript -Body $script:AppBody -Arguments @('-Greeting', 'hello world', '-Other', 'Ünï "q"')
        $r.ExitCode | Should -Be 0 -Because $r.Err
        $lines = @($r.Out -split "\r?\n" | Where-Object { $_ })
        $lines[0..1] | Should -Be @('ROLE launcher', 'ROLE backend')
        $lines | Should -Contain 'PARAMS [hello world] [Ünï "q"] secret=False'
        $lines | Should -Contain 'TEXT Hello Příliš'
        $r.Err | Should -Match 'ORDERLY'
        $r.Err | Should -Not -Match '"level"'
    }
    It 'reports a backend error and a nonzero exit code' {
        $r = Invoke-DirectScript -Body $script:AppBody -Arguments @('-Greeting', 'crash')
        $r.ExitCode | Should -Not -Be 0
        $r.Err | Should -Match 'deliberate failure'
        $r.Err | Should -Match 'ui4me-host error: application failed'
    }
    It 'fails clearly when the host is missing' {
        $r = Invoke-DirectScript -Body $script:AppBody -Environment @{ UI4ME_HOST_BINARY = (Join-Path $TestDrive 'missing-host') }
        $r.ExitCode | Should -Not -Be 0
        $r.Err | Should -Match 'UI4ME_HOST_BINARY does not exist'
        @($r.Out -split "\r?\n" | Where-Object { $_ }) | Should -Be @('ROLE launcher')
    }
    It 'removes its temporary manifest' {
        $r = Invoke-DirectScript -Body $script:AppBody
        $r.ExitCode | Should -Be 0
        @($r.Leftovers | Where-Object { $_ -like 'ui4me-start-*' }).Count | Should -Be 0
    }
}
