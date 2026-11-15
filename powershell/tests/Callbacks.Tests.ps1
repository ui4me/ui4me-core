# Callback pitfalls, reserved member names and dialog waits, against a local JSON-RPC peer (no GUI).
# BIND-PS-01, -05, -08, -09. Pester 5+; PowerShell 7.2+ or Windows PowerShell 5.1.
BeforeAll {
    . (Join-Path $PSScriptRoot 'TestHelpers.ps1')
    Import-Module $script:ModuleManifest -Force -ErrorAction Stop
    function Wait-Transport([scriptblock]$Condition, [int]$Seconds = 10) {
        # Waits WITHOUT pumping: only the transport's reader thread runs.
        $end = [DateTime]::UtcNow.AddSeconds($Seconds)
        while (-not (& $Condition) -and [DateTime]::UtcNow -lt $end) { Start-Sleep -Milliseconds 20 }
        return [bool](& $Condition)
    }
    function Clear-Pump($Application) {
        while ((Wait-UI4ME -Application $Application -TimeoutMilliseconds 150) -gt 0) { }
    }
}

Describe 'Callback pitfalls and codec edge cases' {
    BeforeAll {
        $script:mock = Start-MockHost
        $script:app = Connect-UI4ME -Endpoint $script:mock.Endpoint -Token 'testing-token'
        $script:win = New-UI4MEWindow -Application $script:app
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p>x</p>'
    }
    AfterAll {
        Close-UI4ME -Application $script:app
        Stop-MockHost $script:mock
    }

    It 'contains break and continue in event handlers, so the other handlers still run (BIND-PS-08)' {
        $script:order = New-Object System.Collections.ArrayList
        $subs = @(
            (Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback { param($e) [void]$script:order.Add("a-$($e.element)"); if ($e.element -eq 'one') { break } else { continue } }),
            (Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback { param($e) [void]$script:order.Add("b-$($e.element)") })
        )
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'click'; element = 'one' }, @{ type = 'click'; element = 'two' }) }
        Wait-Until { $script:order.Count -ge 4 } $script:app | Should -BeTrue
        ($script:order -join ',') | Should -Be 'a-one,b-one,a-two,b-two'
        foreach ($s in $subs) { Unregister-UI4MEDocumentEvent -Subscription $s }
    }

    It 'does not let break in a deferred notification callback end the RPC wait that pumps it' {
        $script:ticks = 0
        Register-UI4MENotification -Application $script:app -Method 'test.tick' -Callback { $script:ticks++; break }
        $sub = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback {
            # The ticks arrive while this handler runs, so they are deferred.
            $null = Invoke-UI4MERpc -Application $script:app -Method 'mock.flood_notifications' -Parameters @{ count = 3 }
        }
        $r = Invoke-UI4MERpc -Application $script:app -Method 'mock.click_then_reply'
        $r.ok | Should -BeTrue
        Clear-Pump $script:app
        $script:ticks | Should -Be 3
        Register-UI4MENotification -Application $script:app -Method 'test.tick' -Callback $null
        Unregister-UI4MEDocumentEvent -Subscription $sub
    }

    It 'answers an explicit boolean from policies; break, strings and numbers veto (BIND-PS-08/09)' {
        foreach ($case in @(
                @{ Callback = { break }; Allowed = $false },
                @{ Callback = { 'false' }; Allowed = $false },
                @{ Callback = { 'true' }; Allowed = $false },
                @{ Callback = { 1 }; Allowed = $false },
                @{ Callback = { $false; $true }; Allowed = $true })) {
            Set-UI4MEBeforeClose -Window $script:win -Callback $case.Callback
            Close-UI4MEWindow -Window $script:win
            $decisions = @((Invoke-UI4MERpc -Application $script:app -Method 'mock.stats').decisions)
            $decisions[-1] | Should -BeOfType ([bool])
            $decisions[-1] | Should -Be $case.Allowed
            if ($case.Allowed) { Wait-Until { $script:win.Closed } $script:app | Should -BeTrue }
            else { $script:win.Closed | Should -BeFalse }
        }
    }
}

Describe 'Reserved member names and dialog waits' {
    BeforeAll {
        $script:mock = Start-MockHost
        $script:app = Connect-UI4ME -Endpoint $script:mock.Endpoint -Token 'testing-token'
        $script:win = New-UI4MEWindow -Application $script:app
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p>x</p>'
    }
    AfterAll {
        Close-UI4ME -Application $script:app
        Stop-MockHost $script:mock
    }

    It 'reads attributes named psbase, PSObject, pstypenames and the empty name (BIND-PS-01)' {
        $attributes = Get-UI4MEAttributes -Document $script:doc -Element x
        @($attributes.Keys) | Should -Be @('', 'PSObject', 'psbase', 'pstypenames', 'title')
        $attributes['psbase'] | Should -Be '1'
        $attributes['PSObject'] | Should -Be '2'
        $attributes[''] | Should -Be '3'
        $script:app.Transport.IsDisconnected | Should -BeFalse
        Get-UI4MEDocumentText -Document $script:doc -Element x | Should -BeNullOrEmpty   # still connected
    }

    It 'waits for a -NoWait dialog without the 15 second default deadline (BIND-PS-05)' {
        $echo = Start-UI4MERpc -Application $script:app -Method 'mock.echo'
        $echo.DefaultTimeoutMilliseconds | Should -Be 15000
        $null = Wait-UI4MERpc -Request $echo
        # The mock answers the dialog after 16 s and reads nothing meanwhile. Send no other
        # request in that time: the Windows mock pipe is unbuffered, so a write would block
        # until then and the wait below would start after the answer had already arrived.
        $request = Show-UI4MEFileDialog -Window $script:win -Title '16000' -NoWait
        $request.DefaultTimeoutMilliseconds | Should -Be -1
        $started = [DateTime]::UtcNow
        $result = Wait-UI4MERpc -Request $request
        $elapsed = ([DateTime]::UtcNow - $started).TotalMilliseconds
        $elapsed | Should -BeGreaterThan 15500
        @($result.paths) | Should -Be @('/tmp/picked.txt')
        @((Invoke-UI4MERpc -Application $script:app -Method 'mock.stats').cancels) | Should -Not -Contain $request.Id
    }
}
