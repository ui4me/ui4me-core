# Module, codec, generator and skill-installer tests (no host required). Pester 5+.
BeforeDiscovery {
    # -Skip is evaluated during discovery, before BeforeAll; 5.1-safe (no $IsWindows there).
    $script:DiscoveryIsWindows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
}
BeforeAll {
    . (Join-Path $PSScriptRoot 'TestHelpers.ps1')
    Import-Module $script:ModuleManifest -Force -ErrorAction Stop
    & (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
}

Describe 'UI4ME.Core manifest and exports' {
    It 'has the 1.0.1 identity and matching manifest/module exports' {
        $m = Test-ModuleManifest $script:ModuleManifest
        $m.Version.ToString() | Should -Be '1.0.1'
        $m.CompatiblePSEditions | Should -Contain 'Desktop'
        $m.CompatiblePSEditions | Should -Contain 'Core'
        $declared = @($m.ExportedFunctions.Keys | Sort-Object)
        $actual = @((Get-Command -Module UI4ME.Core -CommandType Function).Name | Sort-Object)
        ($actual -join ',') | Should -Be ($declared -join ',')
        $actual.Count | Should -Be 84   # 1.0.1 had 83; Get-UI4MEAttributes was added (BIND-PS-01)
    }
    It 'documents every exported command with a synopsis or help' {
        foreach ($c in Get-Command -Module UI4ME.Core) {
            (Get-Help $c.Name).Name | Should -Be $c.Name
        }
    }
    It 'keeps module sources ASCII and gives non-ASCII scripts a BOM (Windows PowerShell 5.1 reads BOM-less files as ANSI)' {
        $root = Split-Path -Parent (Split-Path -Parent $script:ModuleManifest)
        foreach ($f in Get-ChildItem -LiteralPath (Split-Path -Parent $script:ModuleManifest) -Recurse -File | Where-Object { $_.Extension -in '.psm1', '.psd1' }) {
            $bytes = [IO.File]::ReadAllBytes($f.FullName)
            @($bytes | Where-Object { $_ -gt 127 }).Count | Should -Be 0 -Because $f.FullName
        }
        $scripts = @(Get-ChildItem -LiteralPath $PSScriptRoot -Recurse -File -Filter *.ps1)
        $examples = Join-Path $root 'examples'
        if (Test-Path -LiteralPath $examples) { $scripts += @(Get-ChildItem -LiteralPath $examples -Recurse -File -Filter *.ps1) }
        foreach ($f in $scripts) {
            $bytes = [IO.File]::ReadAllBytes($f.FullName)
            if (@($bytes | Where-Object { $_ -gt 127 }).Count -gt 0) {
                ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) | Should -BeTrue -Because "$($f.FullName) has non-ASCII text"
            }
        }
    }
    It 'rejects connecting without host credentials' {
        { Connect-UI4ME -Endpoint '' -Token '' } | Should -Throw
    }
}

Describe 'UI4ME JSON codec' {
    It 'keeps date-like strings and numeric types' {
        $v = [UI4ME.Json]::Parse('{"d":"2024-01-01T00:00:00Z","i":5,"l":5000000000,"f":1.25,"e":1e3,"u":18446744073709551615}')
        $v.d | Should -BeOfType ([string])
        $v.i | Should -BeOfType ([int])
        $v.l | Should -BeOfType ([long])
        $v.f | Should -Be 1.25
        $v.e | Should -Be 1000
        $v.u | Should -BeOfType ([uint64])
    }
    It 'decodes escapes and surrogate pairs' {
        ([UI4ME.Json]::Parse('"aé😀\n\"\\\/"')) | Should -BeExactly ("aé😀`n" + '"\/')
    }
    It 'preserves keys that differ only by case' {
        $v = [UI4ME.Json]::Parse('{"A":1,"a":2}')
        $v['A'] | Should -Be 1
        $v['a'] | Should -Be 2
    }
    It 'keeps members whose names PowerShell reserves, and the empty name (BIND-PS-01)' {
        $v = [UI4ME.Json]::Parse('{"psbase":1,"":2,"a":3,"PSObject":{"PSTypeNames":4},"psadapted":5,"psextended":6}')
        $v | Should -BeOfType ([System.Collections.IDictionary])
        $v['psbase'] | Should -Be 1
        $v[''] | Should -Be 2
        $v['a'] | Should -Be 3
        $v['PSObject']['PSTypeNames'] | Should -Be 4
        & (Get-Module UI4ME.Core) { param($o) Get-UI4MEField $o 'psextended' } $v | Should -Be 6
        ([UI4ME.Json]::Parse('{"plain":{"b":1}}')).plain.b | Should -Be 1   # ordinary objects stay PSObjects
    }
    It 'rejects invalid input' {
        foreach ($bad in @('{"a":1,"a":2}', '[1,]', '{"a":NaN}', '"\x"', "`"a`nb`"", '01', '1.', '{"a":1} x', '[' * 200 + ']' * 200)) {
            { [UI4ME.Json]::Parse($bad) } | Should -Throw
        }
    }
    It 'serializes PowerShell values without wrapper artifacts' {
        $json = [UI4ME.Json]::Serialize([ordered]@{ a = @(1); b = @(); c = [pscustomobject]@{ x = 'y' }; d = $null; e = [double]0.1; f = ('q"' + [char]1) })
        $json | Should -BeExactly '{"a":[1],"b":[],"c":{"x":"y"},"d":null,"e":0.1,"f":"q\"\u0001"}'
    }
    It 'refuses values the host cannot interpret' {
        { [UI4ME.Json]::Serialize(@{ a = [double]::NaN }) } | Should -Throw
        { [UI4ME.Json]::Serialize(@{ a = { 1 } }) } | Should -Throw
        { [UI4ME.Json]::Serialize(@{ a = (Get-Date) }) } | Should -Throw
        { [UI4ME.Json]::Serialize(@{ 1 = 'x' }) } | Should -Throw
    }
}

Describe 'New-UI4MEProject' {
    It 'generates a runnable pwsh project' {
        $path = Join-Path $TestDrive 'my app ü'
        $item = New-UI4MEProject -Path $path -Name 'Mÿ App' -AppId org.example.Generated
        $item.FullName | Should -Be ([IO.Path]::GetFullPath($path))
        $toml = Get-Content -LiteralPath (Join-Path $path 'app.toml') -Raw -Encoding UTF8
        $toml | Should -Match 'program = "pwsh"'
        $toml | Should -Match 'name = "Mÿ App"'
        $toml | Should -Match '"-File", "\{app_dir\}/app.ps1"'
        $toml | Should -Match 'id = "org.example.Generated"'
        Test-Path -LiteralPath (Join-Path $path 'backend.ps1') | Should -BeFalse
        $backendBytes = [IO.File]::ReadAllBytes((Join-Path $path 'app.ps1'))
        $backendBytes[0..2] | Should -Be @(0xEF, 0xBB, 0xBF)
        $backend = Get-Content -LiteralPath (Join-Path $path 'app.ps1') -Raw -Encoding UTF8
        $backend | Should -Match "Title 'Mÿ App'"
        # Runs directly; both launch routes share the manifest identity.
        $backend | Should -Match "Start-UI4MEScript -Name 'Mÿ App' -AppId 'org.example.Generated'"
        $backend | Should -Not -Match 'Connect-UI4ME'
        $errors = $null
        $null = [Management.Automation.Language.Parser]::ParseInput($backend, [ref]$null, [ref]$errors)
        @($errors).Count | Should -Be 0
        (Get-Content -LiteralPath (Join-Path $path 'index.html') -Raw) | Should -Match 'stylesheet'
        Test-Path -LiteralPath (Join-Path $path 'style.css') | Should -BeTrue
    }
    It 'generates a Windows PowerShell 5.1 project' {
        $path = Join-Path $TestDrive 'ps51'
        $null = New-UI4MEProject -Path $path -WindowsPowerShell
        (Get-Content -LiteralPath (Join-Path $path 'app.toml') -Raw) | Should -Match 'program = "powershell.exe"'
    }
    It 'refuses to overwrite without -Force and validates names' {
        $path = Join-Path $TestDrive 'twice'
        $null = New-UI4MEProject -Path $path
        { New-UI4MEProject -Path $path } | Should -Throw
        { New-UI4MEProject -Path $path -Force } | Should -Not -Throw
        { New-UI4MEProject -Path (Join-Path $TestDrive 'bad1') -Name 'a"b' } | Should -Throw
        { New-UI4MEProject -Path (Join-Path $TestDrive 'bad2') -AppId 'noperiod' } | Should -Throw
    }
}

Describe 'Install-UI4MESkill' {
    BeforeAll { $script:project = Join-Path $TestDrive 'proj é' ; $null = New-Item -ItemType Directory -Path $script:project }
    It 'plans without touching the file system' {
        $plan = @(Install-UI4MESkill -Agents claude, codex -Scope project -ProjectPath $script:project -DryRun)
        $plan.Count | Should -Be 2
        $plan[0].Action | Should -Be 'would-install'
        Test-Path -LiteralPath (Join-Path $script:project '.claude') | Should -BeFalse
    }
    It 'installs for every agent in project scope' {
        $done = @(Install-UI4MESkill -Agents all -Scope project -ProjectPath $script:project)
        $done.Count | Should -Be 6
        foreach ($rel in '.claude/skills', '.agents/skills', '.opencode/skills', '.github/skills', '.cursor/skills', '.gemini/skills') {
            $skill = Join-Path (Join-Path $script:project $rel) 'ui4me-powershell/SKILL.md'
            Test-Path -LiteralPath $skill | Should -BeTrue
            (Get-Content -LiteralPath $skill -Raw -Encoding UTF8) | Should -Match 'name: ui4me-powershell'
        }
        @(Install-UI4MESkill -Action status -Scope project -ProjectPath $script:project | Where-Object State -eq 'installed').Count | Should -Be 6
        # Reinstalling an unmodified install is allowed.
        { Install-UI4MESkill -Agents claude -Scope project -ProjectPath $script:project } | Should -Not -Throw
    }
    It 'refuses to replace or remove modified and unmanaged folders' {
        $skill = Join-Path $script:project '.claude/skills/ui4me-powershell/SKILL.md'
        Add-Content -LiteralPath $skill -Value 'local edit'
        (Install-UI4MESkill -Action status -Agents claude -Scope project -ProjectPath $script:project).State | Should -Be 'modified'
        { Install-UI4MESkill -Agents claude -Scope project -ProjectPath $script:project } | Should -Throw
        { Install-UI4MESkill -Action uninstall -Agents claude -Scope project -ProjectPath $script:project } | Should -Throw
        $unmanaged = Join-Path $script:project '.cursor/skills/ui4me-powershell'
        Remove-Item -LiteralPath (Join-Path $unmanaged '.ui4me-managed.json') -Force
        (Install-UI4MESkill -Action status -Agents cursor -Scope project -ProjectPath $script:project).State | Should -Be 'unmanaged'
        { Install-UI4MESkill -Agents cursor -Scope project -ProjectPath $script:project } | Should -Throw
        $null = Install-UI4MESkill -Agents claude, cursor -Scope project -ProjectPath $script:project -Force
        (Get-Content -LiteralPath $skill -Raw) | Should -Not -Match 'local edit'
    }
    It 'uninstalls cleanly and leaves no temporary folders' {
        $removed = @(Install-UI4MESkill -Action uninstall -Agents all -Scope project -ProjectPath $script:project)
        @($removed | Where-Object Action -eq 'removed').Count | Should -Be 6
        @(Get-ChildItem -LiteralPath $script:project -Recurse -Force -Filter '.ui4me-skill-*').Count | Should -Be 0
        @(Install-UI4MESkill -Action status -Scope project -ProjectPath $script:project | Where-Object State -ne 'missing').Count | Should -Be 0
    }
    It 'validates agents and scope arguments' {
        { Install-UI4MESkill -Agents vim -Scope project -ProjectPath $script:project } | Should -Throw
        { Install-UI4MESkill -Scope project } | Should -Throw
        { Install-UI4MESkill -Scope user -ProjectPath $script:project } | Should -Throw
    }
}

Describe 'Launcher console interrupt guard' {
    It 'installs and removes a Ctrl+C/Ctrl+Break handler on Windows only' {
        $installed = [UI4ME.ConsoleInterrupt]::Install()
        try {
            if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
                $installed | Should -BeTrue
                [UI4ME.ConsoleInterrupt]::Installed | Should -BeTrue
                [UI4ME.ConsoleInterrupt]::Install() | Should -BeTrue   # idempotent
            } else {
                $installed | Should -BeFalse
                [UI4ME.ConsoleInterrupt]::Installed | Should -BeFalse
            }
            [UI4ME.ConsoleInterrupt]::Count | Should -Be 0
        } finally { [UI4ME.ConsoleInterrupt]::Uninstall() }
        [UI4ME.ConsoleInterrupt]::Installed | Should -BeFalse
    }
}

Describe 'Get-UI4MEHostPath' {
    AfterEach { Remove-Item Env:UI4ME_HOST_BINARY -ErrorAction SilentlyContinue }
    It 'runs a hash-verified private copy when the installed host cannot be made executable (BIND-PS-11)' -Skip:$script:DiscoveryIsWindows {
        $source = Join-Path $TestDrive 'installed-host'
        [IO.File]::WriteAllText($source, "#!/bin/sh`necho cached-host-ran`n")
        $saved = $env:XDG_CACHE_HOME
        $env:XDG_CACHE_HOME = Join-Path $TestDrive 'cache'
        try {
            $copy = & (Get-Module UI4ME.Core) { param($s) Copy-UI4MEHostToCache $s } $source
            $copy | Should -BeLike (Join-Path $env:XDG_CACHE_HOME 'ui4me/powershell/*/ui4me-host')
            & $copy | Should -Be 'cached-host-ran'
            # Reused while it matches; replaced when it does not.
            [IO.File]::WriteAllText($copy, "#!/bin/sh`necho tampered`n")
            $again = & (Get-Module UI4ME.Core) { param($s) Copy-UI4MEHostToCache $s } $source
            $again | Should -Be $copy
            & $again | Should -Be 'cached-host-ran'
            @(Get-ChildItem -LiteralPath (Split-Path $copy) -Force).Count | Should -Be 1   # no temporary files left
        } finally { $env:XDG_CACHE_HOME = $saved }
    }
    It 'honors UI4ME_HOST_BINARY and reports a missing override' {
        $fake = Join-Path $TestDrive 'host-bin'
        Set-Content -LiteralPath $fake -Value 'x'
        $env:UI4ME_HOST_BINARY = $fake
        Get-UI4MEHostPath | Should -Be ([IO.Path]::GetFullPath($fake))
        $env:UI4ME_HOST_BINARY = Join-Path $TestDrive 'missing'
        { Get-UI4MEHostPath } | Should -Throw
    }
}
