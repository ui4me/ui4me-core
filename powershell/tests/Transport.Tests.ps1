# Transport and binding behavior against a local JSON-RPC peer (no GUI).
# Pester 5+. PowerShell 7.2+ (Linux/Windows) or Windows PowerShell 5.1.
BeforeAll {
    . (Join-Path $PSScriptRoot 'TestHelpers.ps1')
    Import-Module $script:ModuleManifest -Force -ErrorAction Stop
}

Describe 'UI4ME.Core transport against a local peer' {
    BeforeAll {
        $script:mock = Start-MockHost
        $script:app = Connect-UI4ME -Endpoint $script:mock.Endpoint -Token 'testing-token'
    }
    AfterAll {
        if ($null -ne $script:app) { Close-UI4ME -Application $script:app }
        Stop-MockHost $script:mock
    }

    It 'authenticates with host.hello' {
        $script:app.Hello.protocol_version | Should -Be 1
        $script:app.Hello.host.name | Should -Be 'mock'
    }

    It 'runs a window and document lifecycle with Unicode text' {
        $script:win = New-UI4MEWindow -Application $script:app -Title 'Tëst 窗口' -MinWidth 200 -MinHeight 100
        $script:win.Id | Should -Be 'w-1'
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p id="x">hi</p>'
        $script:doc.Id | Should -Be 'd-1'
        Set-UI4MEDocumentText -Document $script:doc -Element x -Text "Český 😀 ""quoted"" `t tab \ 2024-01-01T00:00:00"
        Get-UI4MEDocumentText -Document $script:doc -Element x | Should -BeExactly "Český 😀 ""quoted"" `t tab \ 2024-01-01T00:00:00"
    }

    It 'sends typed window fields with host names' {
        $r = Set-UI4MEWindow -Window $script:win -Minimized $true -Title 'T' -Properties @{ maximized = $false }
        $r.echo.minimized | Should -BeTrue
        $r.echo.maximized | Should -BeFalse
        $r.echo.title | Should -Be 'T'
        $r.echo.window_id | Should -Be 'w-1'
        $r = Set-UI4MEWindow -Window $script:win -Properties @{ min_width = $null; min_height = $null }
        $r.echo.PSObject.Properties['min_width'] | Should -Not -BeNullOrEmpty
        $r.echo.min_width | Should -BeNullOrEmpty
    }

    It 'serializes arrays, nested objects and numbers faithfully' {
        $r = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation echo -Parameters @{
            ops = @(@{ op = 'set_text'; element = 'a'; text = '1' }); single = @('one'); empty = @(); n = 2.5; big = 9007199254740993; flag = $false
        }
        $r.command.ops.Count | Should -Be 1
        $r.command.ops[0].element | Should -Be 'a'
        @($r.command.single).Count | Should -Be 1
        @($r.command.empty).Count | Should -Be 0
        $r.command.n | Should -Be 2.5
        $r.command.big | Should -Be 9007199254740993
        $r.command.flag | Should -BeFalse
    }

    It 'delivers events to top-level and function-defined handlers, with nested RPC' {
        $script:seen = New-Object System.Collections.ArrayList
        function Register-Recorder($Document, [string]$Label) {
            $tag = "fn-$Label"
            $sink = $script:seen
            # GetNewClosure in the caller's scope captures $tag and $sink. Inside
            # the closure $script: is the closure's own scope, so use captured locals.
            Register-UI4MEDocumentEvent -Document $Document -Type click -Element save -Callback ({
                param($e)
                [void]$sink.Add("${tag}:$($e.element):$($e.window_id):$($e.document_id)")
            }.GetNewClosure())
        }
        $s1 = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Element save -Callback {
            param($e)
            [void]$script:seen.Add('top')
            Set-UI4MEDocumentText -Document $script:doc -Element x -Text 'event-fired'
        }
        $s2 = Register-Recorder $script:doc 'one'
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'click'; element = 'save' }) }
        Wait-Until { $script:seen.Count -ge 2 } $script:app | Should -BeTrue
        $script:seen[0] | Should -Be 'top'
        $script:seen[1] | Should -Be 'fn-one:save:w-1:d-1'
        Get-UI4MEDocumentText -Document $script:doc -Element x | Should -Be 'event-fired'
        Unregister-UI4MEDocumentEvent -Subscription $s1
        Unregister-UI4MEDocumentEvent -Subscription $s2
    }

    It 'keeps notifications ordered and never nests them inside a running handler' {
        $script:order = New-Object System.Collections.ArrayList
        $subs = @(
            (Register-UI4MEDocumentEvent -Document $script:doc -Type input -Callback {
                param($e)
                [void]$script:order.Add("start:$($e.element)")
                if ($e.element -eq 'a') {
                    # This RPC's reply arrives after events b and c; they must wait.
                    $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'input'; element = 'c' }) }
                }
                [void]$script:order.Add("end:$($e.element)")
            })
        )
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'input'; element = 'a' }, @{ type = 'input'; element = 'b' }) }
        Wait-Until { $script:order.Count -ge 6 } $script:app | Should -BeTrue
        ($script:order -join ',') | Should -Be 'start:a,end:a,start:b,end:b,start:c,end:c'
        foreach ($s in $subs) { Unregister-UI4MEDocumentEvent -Subscription $s }
    }

    It 'routes delegated handlers by event path and ignores stale-document events' {
        $script:hits = 0
        $s = Register-UI4MEDocumentEvent -Document $script:doc -Type click -Element root -Callback { $script:hits++ }
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit -Parameters @{ events = @(@{ type = 'click'; element = 'child' }) }
        $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation emit_stale
        Wait-Until { $script:hits -ge 1 } $script:app | Should -BeTrue
        [void](Wait-UI4ME -Application $script:app -TimeoutMilliseconds 200)
        $script:hits | Should -Be 1
        Unregister-UI4MEDocumentEvent -Subscription $s
    }

    It 'raises typed RPC errors with code, message and data' {
        $caught = $null
        try { $null = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation 'raise_error' } catch { $caught = $_.Exception }
        $caught | Should -BeOfType ([UI4ME.RpcException])
        $caught.Code | Should -Be -32003
        $caught.RpcMessage | Should -Be 'Synthetic policy rejection'
        $caught.RpcData.detail | Should -Be 'mock'
        $caught.Data['RpcCode'] | Should -Be -32003
        { Invoke-UI4MERpc -Application $script:app -Method 'no.such.method' } | Should -Throw -ExceptionType ([UI4ME.RpcException])
    }

    It 'times out, requests cancellation and discards the late reply' {
        $request = Start-UI4MERpc -Application $script:app -Method 'mock.never'
        { Wait-UI4MERpc -Request $request -TimeoutMilliseconds 150 } | Should -Throw -ExceptionType ([System.TimeoutException])
        $request.Cancelled | Should -BeTrue
        $null = Invoke-UI4MERpc -Application $script:app -Method 'mock.late' -Parameters @{ target = $request.Id }
        $stats = Invoke-UI4MERpc -Application $script:app -Method 'mock.stats'
        @($stats.cancels) | Should -Contain $request.Id
        $script:app.Pending.Count | Should -Be 0
        $script:app.Transport.PendingCount | Should -Be 0
    }

    It 'supports explicit asynchronous cancel' {
        $request = Start-UI4MERpc -Application $script:app -Method 'mock.never'
        Stop-UI4MERpc -Request $request
        { Wait-UI4MERpc -Request $request } | Should -Throw
        $stats = Invoke-UI4MERpc -Application $script:app -Method 'mock.stats'
        @($stats.cancels) | Should -Contain $request.Id
    }

    It 'bounds queued notifications at 128 and counts drops instead of failing' {
        $before = $script:app.Transport.DroppedNotifications
        $request = Start-UI4MERpc -Application $script:app -Method 'mock.flood_notifications' -Parameters @{ count = 400 }
        Start-Sleep -Milliseconds 1500   # not pumping: the reader must apply the bound
        $null = Wait-UI4MERpc -Request $request
        while ((Wait-UI4ME -Application $script:app -TimeoutMilliseconds 50) -gt 0) { }
        ($script:app.Transport.DroppedNotifications - $before) | Should -Be 272
        $script:app.Transport.QueuedMessages | Should -Be 0
        $script:app.Transport.IsDisconnected | Should -BeFalse
    }

    It 'rejects host requests beyond the queue bound with -32005 and answers the rest' {
        $request = Start-UI4MERpc -Application $script:app -Method 'mock.flood_requests' -Parameters @{ count = 200 }
        Start-Sleep -Milliseconds 1500
        $null = Wait-UI4MERpc -Request $request
        $codes = @((Invoke-UI4MERpc -Application $script:app -Method 'mock.stats').codes)
        @($codes | Where-Object { $_ -eq -32601 }).Count | Should -Be 128
        @($codes | Where-Object { $_ -eq -32005 }).Count | Should -Be 72
        $script:app.Transport.RejectedRequests | Should -Be 72
        $script:app.Transport.IsDisconnected | Should -BeFalse
    }

    It 'maps, writes and presents RGBA8 frames, then retires the bitmap' {
        $b = New-UI4MELiveBitmap -Document $script:doc -Id 'mock-frame' -Width 8 -Height 8 -Slots 3
        try {
            $rgba = New-Object byte[] 256
            for ($i = 1; $i -le 7; $i++) {
                $rgba[0] = [byte]$i
                $r = Send-UI4MELiveBitmap -Bitmap $b -Pixels $rgba
                $r.presented | Should -BeTrue
                $r.slot | Should -Be ($i % 3)
                $r.first_byte | Should -Be $i
            }
            $b.Sequence | Should -Be 7
            { Send-UI4MELiveBitmap -Bitmap $b -Pixels (New-Object byte[] 255) } | Should -Throw
        } finally { Remove-UI4MELiveBitmap -Bitmap $b }
        $b.Closed | Should -BeTrue
        { Send-UI4MELiveBitmap -Bitmap $b -Pixels $rgba } | Should -Throw
        Remove-UI4MELiveBitmap -Bitmap $b
    }

    It 'binds large frames quickly and accepts unrolled byte arrays' {
        $b = New-UI4MELiveBitmap -Document $script:doc -Id 'large' -Width 1024 -Height 768
        try {
            $frame = New-Object byte[] (1024 * 768 * 4)
            $clock = [Diagnostics.Stopwatch]::StartNew()
            for ($i = 0; $i -lt 10; $i++) { $null = Send-UI4MELiveBitmap -Bitmap $b -Pixels $frame }
            $clock.ElapsedMilliseconds | Should -BeLessThan 3000
            $small = New-UI4MELiveBitmap -Document $script:doc -Id 'unrolled' -Width 2 -Height 1
            function Get-Frame { [byte[]]@(7, 0, 0, 255, 0, 0, 0, 255) }   # PowerShell unrolls this into object[]
            (Send-UI4MELiveBitmap -Bitmap $small -Pixels (Get-Frame)).first_byte | Should -Be 7
            { Send-UI4MELiveBitmap -Bitmap $small -Pixels @(1, 2, 3, 300, 0, 0, 0, 0) } | Should -Throw
            Remove-UI4MELiveBitmap -Bitmap $small
        } finally { Remove-UI4MELiveBitmap -Bitmap $b }
    }

    It 'returns $null from -Try while a present of the same bitmap is in progress' {
        $b = New-UI4MELiveBitmap -Document $script:doc -Id 'busy' -Width 2 -Height 2
        try {
            $b.Busy = $true
            Send-UI4MELiveBitmap -Bitmap $b -Pixels (New-Object byte[] 16) -Try | Should -BeNullOrEmpty
            { Send-UI4MELiveBitmap -Bitmap $b -Pixels (New-Object byte[] 16) } | Should -Throw
            $b.Busy = $false
            (Send-UI4MELiveBitmap -Bitmap $b -Pixels (New-Object byte[] 16) -Try).presented | Should -BeTrue
        } finally { Remove-UI4MELiveBitmap -Bitmap $b }
    }

    It 'invalidates the previous document, its handlers and bitmaps on replacement' {
        $old = $script:doc
        $bitmap = New-UI4MELiveBitmap -Document $old -Id 'old' -Width 4 -Height 4
        $node = Get-UI4MENode -Document $old -Reference 'n:1:5'
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p id="new">New</p>'
        $script:doc.Id | Should -Be 'd-2'
        $old.Retired | Should -BeTrue
        $bitmap.Closed | Should -BeTrue
        { Get-UI4MEDocumentText -Document $old -Element x } | Should -Throw -ExceptionType ([UI4ME.StaleObjectException])
        { Get-UI4MENodeInfo -Node $node } | Should -Throw -ExceptionType ([UI4ME.StaleObjectException])
    }

    It 'routes node operations and refuses nodes from another document' {
        $n = Get-UI4MENode -Document $script:doc -Reference 'n:2:9'
        $r = Invoke-UI4MEDocumentCommand -Document $script:doc -Operation echo -Parameters @{ ops = @(@{ op = 'set_text'; node = $n.Id; text = 't' }) }
        $r.command.ops[0].node | Should -Be 'n:2:9'
        { Set-UI4MEFocus -Document $script:doc } | Should -Throw
    }

    It 'retires a document after a fatal document error and reports it to handlers' {
        $script:errors = New-Object System.Collections.ArrayList
        $doc = $script:doc
        $null = Register-UI4MEDocumentEvent -Document $doc -Type error -Callback { param($e) [void]$script:errors.Add($e) }
        $null = Invoke-UI4MEDocumentCommand -Document $doc -Operation emit_fatal
        Wait-Until { $doc.Retired } $script:app | Should -BeTrue
        $script:errors.Count | Should -Be 1
        $script:errors[0].fatal | Should -BeTrue
        $script:win.Document | Should -BeNullOrEmpty
        $script:doc = Open-UI4MEHtml -Window $script:win -Html '<p>again</p>'
    }

    It 'applies a before-close veto, then allows and retires the window' {
        $script:asked = 0
        Set-UI4MEBeforeClose -Window $script:win -Callback { param($p) $script:asked++; 'noise'; $false }
        Close-UI4MEWindow -Window $script:win
        [void](Wait-UI4ME -Application $script:app -TimeoutMilliseconds 200)
        $script:asked | Should -Be 1
        $script:win.Closed | Should -BeFalse
        Set-UI4MEBeforeClose -Window $script:win -Callback { $true }
        Close-UI4MEWindow -Window $script:win
        Wait-Until { $script:win.Closed } $script:app | Should -BeTrue
        $script:doc.Retired | Should -BeTrue
        { Get-UI4MEWindow -Window $script:win } | Should -Throw -ExceptionType ([UI4ME.StaleObjectException])
    }

    It 'applies the quit policy, then shuts down on app.shutdown' {
        Set-UI4MEBeforeQuit -Application $script:app -Callback { $false }
        (Request-UI4MEQuit -Application $script:app).allowed | Should -BeFalse
        Set-UI4MEBeforeQuit -Application $script:app -Callback { $true }
        (Request-UI4MEQuit -Application $script:app).allowed | Should -BeTrue
        Wait-UI4ME -Application $script:app -UntilShutdown
        $script:app.ShutdownRequested | Should -BeTrue
    }
}

Describe 'UI4ME.Core failure handling' {
    It 'refuses a wrong token' {
        $mock = Start-MockHost
        try {
            { Connect-UI4ME -Endpoint $mock.Endpoint -Token 'wrong' } | Should -Throw -ExceptionType ([UI4ME.RpcException])
        } finally { Stop-MockHost $mock }
    }

    It 'surfaces an unexpected host disconnect' {
        $mock = Start-MockHost
        $app = Connect-UI4ME -Endpoint $mock.Endpoint -Token 'testing-token'
        try {
            $null = Invoke-UI4MERpc -Application $app -Method 'mock.disconnect'
            { Wait-UI4ME -Application $app -UntilShutdown } | Should -Throw -ExceptionType ([UI4ME.DisconnectedException])
            $app.Disconnected | Should -BeTrue
            { Invoke-UI4MERpc -Application $app -Method 'host.capabilities' } | Should -Throw -ExceptionType ([UI4ME.DisconnectedException])
        } finally { Close-UI4ME -Application $app; Stop-MockHost $mock }
    }

    It 'treats a protocol violation as fatal to the connection' {
        $mock = Start-MockHost
        $app = Connect-UI4ME -Endpoint $mock.Endpoint -Token 'testing-token'
        try {
            $null = Invoke-UI4MERpc -Application $app -Method 'mock.garbage'
            { Wait-UI4ME -Application $app -UntilShutdown } | Should -Throw -ExceptionType ([UI4ME.DisconnectedException])
            $app.Transport.Failure | Should -Match 'request ID'
        } finally { Close-UI4ME -Application $app; Stop-MockHost $mock }
    }

    It 'fails fast when no host is listening' {
        $endpoint = if ($script:TestIsWindows) { '\\.\pipe\ui4me-absent-' + [guid]::NewGuid().ToString('N') } else { Join-Path ([IO.Path]::GetTempPath()) 'ui4me-absent.sock' }
        { Connect-UI4ME -Endpoint $endpoint -Token 'x' -ConnectTimeoutMilliseconds 300 } | Should -Throw
    }
}
