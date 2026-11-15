# Delivery of events and lifecycle notifications under bursts, against a local JSON-RPC peer (no GUI).
# T16 (PowerShell side), BIND-PS-02/03/04. Pester 5+; PowerShell 7.2+ or Windows PowerShell 5.1.
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

Describe 'Control notifications under event bursts (T16)' {
    BeforeEach {
        $script:mock = Start-MockHost
        $script:app = Connect-UI4ME -Endpoint $script:mock.Endpoint -Token 'testing-token'
        $script:win = New-UI4MEWindow -Application $script:app
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p>x</p>'
    }
    AfterEach {
        Close-UI4ME -Application $script:app
        Stop-MockHost $script:mock
    }

    It 'retires the window when a close lands in a 1,000-event burst, and drops no control notification' {
        $script:moves = 0
        $script:life = New-Object System.Collections.ArrayList
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type pointermove -Callback { $script:moves++ }
        Register-UI4MENotification -Application $script:app -Method window.event -Callback { param($p) [void]$script:life.Add($p.type) }
        $null = Invoke-UI4MERpc -Application $script:app -Method mock.configure -Parameters @{ close_burst = 1000 }
        $request = Start-UI4MERpc -Application $script:app -Method window.request_close -Parameters @{ window_id = $script:win.Id }
        Start-Sleep -Milliseconds 1500   # not pumping: 1,000 events, close_requested and before_close queue up
        $null = Wait-UI4MERpc -Request $request
        # The closed event arrives inside the burst; it is applied before anything is pumped.
        Wait-Transport { $script:app.Transport.IsWindowClosed($script:win.Id) } | Should -BeTrue
        { Get-UI4MEWindow -Window $script:win } | Should -Throw -ExceptionType ([UI4ME.StaleObjectException])
        $script:win.Closed | Should -BeTrue
        $script:doc.Retired | Should -BeTrue
        Clear-Pump $script:app
        ($script:life -join ',') | Should -Be 'close_requested,closed'
        $t = $script:app.Transport
        $t.DroppedControl | Should -Be 0
        $t.DroppedNotifications | Should -BeGreaterThan 0
        ($t.DroppedCoalescible + $t.DroppedDiscrete) | Should -Be $t.DroppedNotifications
        $script:app.DroppedNotifications | Should -Be 0
        $t.IsDisconnected | Should -BeFalse
    }

    It 'delivers a click that follows a 300-event burst' {
        $script:clicks = New-Object System.Collections.ArrayList
        $script:runs = 0
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type transitionrun -Callback { $script:runs++ }
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback { param($e) [void]$script:clicks.Add($e.element) }
        $request = Start-UI4MERpc -Application $script:app -Method mock.burst -Parameters @{ count = 300; type = 'transitionrun'; click = $true }
        Wait-Transport { $script:app.Transport.DroppedNotifications -ge 300 - 128 } | Should -BeTrue
        Start-Sleep -Milliseconds 200
        $null = Wait-UI4MERpc -Request $request
        Clear-Pump $script:app
        @($script:clicks) | Should -Be @('after-burst')
        $script:app.Transport.DroppedDiscrete | Should -Be 0
        $script:runs | Should -BeLessOrEqual 128
    }

    It 'never lets unsubscribed events take admission slots' {
        $script:clicks = 0
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback { $script:clicks++ }
        $request = Start-UI4MERpc -Application $script:app -Method mock.burst -Parameters @{ count = 1000; type = 'transitionrun'; click = $true }
        Wait-Transport { $script:app.Transport.FilteredNotifications -ge 1000 } | Should -BeTrue
        $null = Wait-UI4MERpc -Request $request
        Clear-Pump $script:app
        $script:clicks | Should -Be 1
        $script:app.Transport.DroppedNotifications | Should -Be 0
    }

    It 'keeps the closed event that arrives while a callback runs, beyond the deferral cap (BIND-PS-03)' {
        $script:closedSeen = $false
        $script:closedDuringClick = $null
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type transitionrun -Callback { }
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback {
            $null = Invoke-UI4MERpc -Application $script:app -Method mock.burst -Parameters @{ count = 300; type = 'transitionrun'; close = $true }
            for ($i = 0; $i -lt 20; $i++) { $null = Invoke-UI4MERpc -Application $script:app -Method mock.echo }
            $script:closedDuringClick = $script:win.Closed
        }
        Register-UI4MENotification -Application $script:app -Method window.event -Callback { param($p) if ($p.type -eq 'closed') { $script:closedSeen = $true } }
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'click'; element = 'go' }) }
        Wait-Until { $script:closedSeen } $script:app | Should -BeTrue
        $script:closedDuringClick | Should -BeTrue   # bookkeeping did not wait for the click handler
        $script:win.Closed | Should -BeTrue
        $script:app.DroppedNotifications | Should -BeGreaterThan 0   # transitionrun beyond the cap
    }

    It 'delivers deferred final events before Wait-UI4ME -UntilShutdown returns (BIND-PS-04)' {
        $script:closedSeen = $false
        $null = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Callback {
            $null = Invoke-UI4MERpc -Application $script:app -Method mock.burst -Parameters @{ count = 0; type = 'x'; close = $true; shutdown = $true }
            for ($i = 0; $i -lt 3; $i++) { $null = Invoke-UI4MERpc -Application $script:app -Method mock.echo }
        }
        Register-UI4MENotification -Application $script:app -Method window.event -Callback { param($p) if ($p.type -eq 'closed') { $script:closedSeen = $true } }
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'click'; element = 'go' }) }
        Wait-UI4ME -Application $script:app -UntilShutdown
        $script:app.ShutdownRequested | Should -BeTrue
        $script:closedSeen | Should -BeTrue
        $script:app.Deferred.Count | Should -Be 0
    }
}
