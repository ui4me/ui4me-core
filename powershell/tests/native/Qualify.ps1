# Native PowerShell parity qualification backend. Launched by the real UI4ME
# host (app-pwsh.toml / app-ps51.toml) inside a real desktop session.
#
# Environment:
#   UI4ME_PS_RESULTS  absolute folder for results.jsonl (records + input requests)
#                     and acks.jsonl (written by the platform input driver)
#   UI4ME_PS_MODULE   optional path to the UI4ME.Core.psd1 under test
#   UI4ME_PS_MODE     full (default) | auto (no input driver) | idle | bitmap-kill
#
# Automatic checks need no input. Interactive checks write a request line with
# client-area coordinates (CSS px) and wait, bounded, for the resulting events.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
if ($env:UI4ME_PS_MODULE) { Import-Module $env:UI4ME_PS_MODULE -Force } else { Import-Module UI4ME.Core }
$mode = if ($env:UI4ME_PS_MODE) { $env:UI4ME_PS_MODE } else { 'full' }
$resultsDir = $env:UI4ME_PS_RESULTS
if (-not $resultsDir) { throw 'UI4ME_PS_RESULTS is required' }
$resultsFile = Join-Path $resultsDir 'results.jsonl'
$acksFile = Join-Path $resultsDir 'acks.jsonl'
$utf8 = New-Object Text.UTF8Encoding($false)
$script:requestId = 0
$script:failures = 0

function Write-Record([hashtable]$Record) {
    $Record.t = [DateTime]::UtcNow.ToString('o')
    [IO.File]::AppendAllText($resultsFile, ([UI4ME.Json]::Serialize($Record) + "`n"), $utf8)
}
function Check([string]$Name, [scriptblock]$Body) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    try {
        $detail = & $Body
        Write-Record @{ kind = 'check'; name = $Name; status = 'pass'; ms = $clock.ElapsedMilliseconds; detail = $detail }
    } catch {
        $script:failures++
        Write-Record @{ kind = 'check'; name = $Name; status = 'fail'; ms = $clock.ElapsedMilliseconds
            error = "$($_.Exception.GetType().FullName): $($_.Exception.Message)"; at = "$($_.InvocationInfo.ScriptLineNumber)" }
    }
}
function Assert-True($Condition, [string]$Message) { if (-not $Condition) { throw "Assertion failed: $Message" } }
function Pump-Until([scriptblock]$Condition, [int]$Milliseconds = 10000) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Condition)) {
        if ($clock.ElapsedMilliseconds -ge $Milliseconds) { return $false }
        [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 25)
    }
    return $true
}
function Request-Input([hashtable]$Action) {
    $script:requestId++
    $Action.kind = 'request'; $Action.id = $script:requestId
    Write-Record $Action
    return $script:requestId
}
function Wait-Ack([int]$Id, [int]$Milliseconds = 30000) {
    $found = $null
    $ok = Pump-Until {
        if (Test-Path -LiteralPath $acksFile) {
            foreach ($line in [IO.File]::ReadAllLines($acksFile, $utf8)) {
                if (-not $line) { continue }
                $a = [UI4ME.Json]::Parse($line)
                if ($a.ack -eq $Id) { $script:lastAck = $a; return $true }
            }
        }
        return $false
    } $Milliseconds
    if (-not $ok) { throw "Input driver did not acknowledge request $Id" }
    return $script:lastAck
}
function Get-Rect([string]$Element) {
    $r = (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_rect -Parameters @{ element = $Element }).rect
    Assert-True ($null -ne $r) "$Element has a layout box"
    return $r
}
function Get-Center([string]$Element) {
    $r = Get-Rect $Element
    return @{ x = [double]$r.x + [double]$r.width / 2; y = [double]$r.y + [double]$r.height / 2 }
}
function Get-Png1x1 {
    # A valid 1x1 opaque PNG.
    return [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==')
}

$started = [Diagnostics.Stopwatch]::StartNew()
$app = Connect-UI4ME
Write-Record @{ kind = 'env'; mode = $mode; ps_version = $PSVersionTable.PSVersion.ToString(); edition = $PSVersionTable.PSEdition
    os = [Environment]::OSVersion.VersionString; clr = [Environment]::Version.ToString(); pid = $PID
    module = (Get-Module UI4ME.Core).Path; module_version = (Get-Module UI4ME.Core).Version.ToString()
    host_path = $(try { Get-UI4MEHostPath } catch { '' }); connect_ms = $started.ElapsedMilliseconds; hello = $app.Hello }

$win = $null; $doc = $null
try {
    # ---------------------------------------------------------- startup ---
    $assets = Join-Path ([IO.Path]::GetTempPath()) ('ui4me ps qualification ünï ' + [guid]::NewGuid().ToString('N').Substring(0, 6))
    $null = New-Item -ItemType Directory -Path $assets
    $htmlPath = Join-Path $assets 'qualifiçation 文件.html'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'index.html') -Destination $htmlPath
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'style.css') -Destination (Join-Path $assets 'style.css')
    $win = New-UI4MEWindow -Application $app -Title "UI4ME PS qualification $PID" -Width 900 -Height 640 -MinWidth 400 -MinHeight 300
    $doc = Open-UI4MEFile -Window $win -Path $htmlPath
    $null = Register-UI4MEResources -Document $doc -Name generated -Entries @{ 'dot.png' = @{ Bytes = (Get-Png1x1); Mime = 'image/png' } }
    Set-UI4MEReady -Application $app
    Write-Record @{ kind = 'ready'; ms = $started.ElapsedMilliseconds; window = $win.Id; title = "UI4ME PS qualification $PID" }

    if ($mode -eq 'idle') {
        $before = $app.Transport.ReceivedMessages
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $script:idleTypes = @{}
        Register-UI4MENotification -Application $app -Method 'document.event' -Callback { param($p) $k = [string]$p.event.type; $script:idleTypes[$k] = 1 + [int]$script:idleTypes[$k] }
        Wait-UI4ME -Application $app -UntilShutdown
        Write-Record @{ kind = 'idle'; seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 1); messages = $app.Transport.ReceivedMessages - $before; event_types = $script:idleTypes }
        return
    }
    if ($mode -eq 'bitmap-kill') {
        $b = New-UI4MELiveBitmap -Document $doc -Id frames -Width 320 -Height 200
        $frame = New-Object byte[] (320 * 200 * 4)
        Write-Record @{ kind = 'bitmap-streaming'; pid = $PID }
        $n = 0
        while ($true) { $frame[0] = [byte]($n % 256); $null = Send-UI4MELiveBitmap -Bitmap $b -Pixels $frame; $n++ }
    }

    if ($mode -eq 'winextras') {
        # Windows desktop integrations exercised from a PowerShell backend.
        $script:imeEvents = New-Object System.Collections.ArrayList
        $imeSink = $script:imeEvents
        foreach ($t in @('compositionstart', 'compositionend', 'input')) {
            $null = Register-UI4MEDocumentEvent -Document $doc -Type $t -Element name -Callback ({ param($e) [void]$imeSink.Add($e) }.GetNewClosure())
        }
        Check 'UI Automation (native COM client) reads text set by PowerShell' {
            Set-UI4MEDocumentValue -Document $doc -Element name -Value 'UIA from PowerShell ü'
            $a = Wait-Ack (Request-Input @{ action = 'uia'; automation_id = 'name' }) 60000
            Assert-True ([string]$a.log -match "DocumentRange.GetText 'UIA from PowerShell ü'") "probe log: $($a.log)"
            @{ probe = $a.log }
        }
        Check 'Microsoft Pinyin composition reaches PowerShell callbacks' {
            Set-UI4MEDocumentValue -Document $doc -Element name -Value ''
            $r = (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_rect -Parameters @{ element = 'name' }).rect
            $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $r.x + 20; y = $r.y + $r.height / 2 })
            $a = Wait-Ack (Request-Input @{ action = 'ime-pinyin'; text = 'nihao' }) 60000
            Assert-True (Pump-Until { (Get-UI4MEValue -Document $doc -Element name) -eq '你好' } 10000) "value is '$(Get-UI4MEValue -Document $doc -Element name)'"
            $ends = @($script:imeEvents | Where-Object { $_.type -eq 'compositionend' })
            Assert-True ($ends.Count -ge 1) "compositionend events: $(@($script:imeEvents | ForEach-Object type) -join ',')"
            @{ driver = $a; compositionend = $ends[-1]; events = @($script:imeEvents | ForEach-Object type) }
        }
        Write-Record @{ kind = 'done'; failures = $script:failures }
        Request-UI4MEQuit -Application $app | Out-Null
        Wait-UI4ME -Application $app -UntilShutdown
        return
    }

    # ------------------------------------------------- automatic checks ---
    Check 'host.capabilities, preferences, window list' {
        $caps = Get-UI4MECapabilities -Application $app
        $prefs = Get-UI4MEPreferences -Application $app
        $list = Get-UI4MEWindowList -Application $app
        Assert-True (@($list).Count -eq 1) 'one window listed'
        Assert-True ($list[0].window_id -eq $win.Id) 'window id round-trips'
        @{ capabilities = $caps; preferences = $prefs; window = (Get-UI4MEWindow -Window $win) }
    }
    Check 'file document with spaces and non-ASCII path, relative stylesheet' {
        $bg = (Get-UI4MEComputedStyle -Document $doc -Element target -Properties width).properties
        $width = (Get-Rect 'target').width
        Assert-True ([Math]::Abs([double]$width - 140) -lt 1) "stylesheet applied (width $width)"
        @{ path = $htmlPath; computed = $bg }
    }
    Check 'Unicode text round-trip and escaping' {
        $text = 'Český 😀 <b>not bold</b> & "quotes" 2024-01-01T00:00:00Z'
        Set-UI4MEDocumentText -Document $doc -Element status -Text $text
        Assert-True ((Get-UI4MEDocumentText -Document $doc -Element status) -ceq $text) 'text equal'
        $html = (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_html -Parameters @{ element = 'status' }).html
        Assert-True ($html -match '&lt;b&gt;') 'text not parsed as HTML'
        @{ html = $html }
    }
    Check 'atomic batch commits all or nothing' {
        Invoke-UI4MEBatch -Document $doc -Operations @(
            @{ op = 'set_text'; element = 'heading'; text = 'Batch A' },
            @{ op = 'set_attribute'; element = 'other'; name = 'data-x'; value = '1' })
        $rejected = $false
        try {
            Invoke-UI4MEBatch -Document $doc -Operations @(
                @{ op = 'set_text'; element = 'heading'; text = 'Batch B' },
                @{ op = 'set_text'; element = 'does-not-exist'; text = 'x' })
        } catch [UI4ME.RpcException] { $rejected = $true; $code = $_.Exception.Code }
        Assert-True $rejected 'invalid batch rejected'
        Assert-True ((Get-UI4MEDocumentText -Document $doc -Element heading) -eq 'Batch A') 'rejected batch rolled back'
        @{ rejected_code = $code }
    }
    Check 'classes, attributes, HTML insertion, removal and query' {
        Set-UI4MEClass -Document $doc -Element list -Name highlighted
        Set-UI4MEDocumentAttribute -Document $doc -Element list -Name 'data-state' -Value 'ok'
        Set-UI4MEDocumentHtml -Document $doc -Element list -Html '<li id="added">four</li>' -Position beforeend
        $q = Find-UI4MEElement -Document $doc -Selector '#list li'
        Remove-UI4MEElement -Document $doc -Element added
        $q2 = Find-UI4MEElement -Document $doc -Selector '#list li'
        Set-UI4MEClass -Document $doc -Element list -Name highlighted -Remove
        $attr = (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_attribute -Parameters @{ element = 'list'; name = 'data-state' }).value
        Assert-True ($attr -eq 'ok') 'attribute set'
        @{ before = $q; after = $q2 }
    }
    Check 'opaque nodes: query, inspect, children, node batch, move' {
        $nodes = Get-UI4MENodes -Document $doc -Selector '#list li'
        $refs = @($nodes.nodes)
        Assert-True ($refs.Count -ge 3) 'three anonymous list items'
        $first = Get-UI4MENode -Document $doc -Reference ([string]$(if ($refs[0] -is [string]) { $refs[0] } else { $refs[0].node }))
        $info = Get-UI4MENodeInfo -Node $first
        Invoke-UI4MENodeOperation -Node $first -Operation set_text -Parameters @{ text = 'uno' }
        $list = Get-UI4MENode -Document $doc -Element list
        $children = Get-UI4MENodeChildren -Node $list
        @{ nodes = $nodes; info = $info; children = $children }
    }
    Check 'stylesheet add, replace, list, remove' {
        $sheet = Add-UI4MEStylesheet -Document $doc -Css '#other { width: 200px; }'
        $id = [long]$sheet.sheet
        Assert-True ([Math]::Abs((Get-Rect 'other').width - 200) -lt 1) 'added sheet applied'
        $null = Set-UI4MEStylesheet -Document $doc -Sheet $id -Css '#other { width: 260px; }'
        Assert-True ([Math]::Abs((Get-Rect 'other').width - 260) -lt 1) 'replaced sheet applied'
        $sheets = Get-UI4MEStylesheets -Document $doc
        Set-UI4MEStylesheet -Document $doc -Sheet $id -Remove
        Assert-True ([Math]::Abs((Get-Rect 'other').width - 140) -lt 1) 'removed sheet no longer applies'
        @{ sheet = $sheet; sheets = $sheets }
    }
    Check 'forms, values, checked, select, selection, form data' {
        Set-UI4MEDocumentValue -Document $doc -Element name -Value 'Žluťoučký kůň'
        Set-UI4MEChecked -Document $doc -Element agree -Checked $true
        Set-UI4MESelectedValues -Document $doc -Element choice -Values @('b')
        $null = Set-UI4MESelection -Document $doc -Element name -Start 1 -End 4
        $sel = Get-UI4MESelection -Document $doc -Element name
        $data = Get-UI4MEFormData -Document $doc -Form form
        $options = Get-UI4MESelectOptions -Document $doc -Element choice
        Assert-True ((Get-UI4MEValue -Document $doc -Element name) -ceq 'Žluťoučký kůň') 'value'
        Assert-True (Get-UI4MEChecked -Document $doc -Element agree) 'checked'
        Assert-True ($options.Count -eq 3) 'three options'
        $null = Reset-UI4MEForm -Document $doc -Form form
        Assert-True ((Get-UI4MEValue -Document $doc -Element name) -eq 'initial') 'reset restores default'
        @{ selection = $sel; form_data = $data; options = $options }
    }
    Check 'environment, scroll, focus, diagnostics, resource stats' {
        $null = Set-UI4MEEnvironment -Document $doc -ColorScheme dark
        $envr = Get-UI4MEEnvironment -Document $doc
        $null = Set-UI4MEEnvironment -Document $doc -ColorScheme light
        Set-UI4MEFocus -Document $doc -Element name
        $focused = Get-UI4MEFocusedElement -Document $doc
        Set-UI4MEFocus -Document $doc -Blur
        @{ environment = $envr; focused = $focused; scroll = (Get-UI4MEScroll -Document $doc)
           diagnostics = (Get-UI4MEDiagnostics -Document $doc); resources = (Get-UI4MEResourceStats -Document $doc) }
    }
    Check 'generated resource bundle renders and unregisters' {
        $stats = Get-UI4MEResourceStats -Document $doc
        $logo = Get-Rect 'logo'
        Assert-True ($logo.width -eq 8) 'resource image laid out'
        $null = Unregister-UI4MEResources -Document $doc -Name generated
        $null = Register-UI4MEResources -Document $doc -Name generated -Entries @{ 'dot.png' = @{ Bytes = (Get-Png1x1); Mime = 'image/png' } }
        @{ stats = $stats }
    }
    Check 'system clipboard round-trip' {
        # Wayland only lets a client set the selection after its seat received input.
        $c = Get-Center heading
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $c.x; y = $c.y })
        [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 200)
        $text = "ui4me-ps-clip $PID 😀 ü"
        Set-UI4MEClipboard -Application $app -Text $text
        $read = Get-UI4MEClipboard -Application $app
        Assert-True ($read -ceq $text) "clipboard read back '$read'"
        $outside = Wait-Ack (Request-Input @{ action = 'clipboard-read' })
        Assert-True ($outside.text -ceq $text) "another application reads '$($outside.text)'"
        $external = "external → PowerShell $PID ✓"
        $null = Wait-Ack (Request-Input @{ action = 'clipboard-write'; text = $external })
        $back = Get-UI4MEClipboard -Application $app
        Assert-True ($back -ceq $external) "PowerShell reads external text '$back'"
        @{ text = $read; external_reader = $outside.text; external_writer = $back }
    }
    Check 'typed RPC errors and external-open policy' {
        $codes = @{}
        try { $null = Invoke-UI4MEDocumentCommand -Document $doc -Operation no_such_operation } catch [UI4ME.RpcException] { $codes.unknown_op = $_.Exception.Code }
        try { Set-UI4MEDocumentText -Document $doc -Element missing-element -Text x } catch [UI4ME.RpcException] { $codes.missing_element = $_.Exception.Code }
        try { $null = Open-UI4MEExternal -Application $app -Target 'javascript:alert(1)' } catch [UI4ME.RpcException] { $codes.scheme_refused = $_.Exception.Code }
        try { $null = Invoke-UI4MERpc -Application $app -Method 'desktop.open_external' -Parameters @{ target = 'relative/file.txt' } } catch [UI4ME.RpcException] { $codes.relative_refused = $_.Exception.Code }
        $expected = 4
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            # Host policy (Windows only): executables, scripts and shortcuts are never launched.
            $script = Join-Path $assets 'run me.cmd'
            [IO.File]::WriteAllText($script, "echo no`r`n")
            try { $null = Open-UI4MEExternal -Application $app -Target $script } catch [UI4ME.RpcException] { $codes.executable_refused = $_.Exception.Code }
            $expected = 5
        }
        Assert-True ($codes.Count -eq $expected) "all refused: $($codes.Keys -join ',')"
        $codes
    }
    Check 'live bitmaps: geometry, 1000+ frames per size, bounded memory, cleanup' {
        $report = @{}
        $before = [GC]::GetTotalMemory($true)
        foreach ($size in @(@(64, 64), @(320, 200), @(1024, 768))) {
            $w = $size[0]; $h = $size[1]
            $b = New-UI4MELiveBitmap -Document $doc -Id frames -Width $w -Height $h -Slots 2
            try {
            $frames = @((New-Object byte[] ($w * $h * 4)), (New-Object byte[] ($w * $h * 4)))
            for ($i = 3; $i -lt $frames[0].Length; $i += 4) { $frames[0][$i] = 255; $frames[1][$i] = 128 }
            $frames[0][0] = 255; $frames[1][1] = 255
            $count = 1000
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $sequences = New-Object System.Collections.Generic.List[long]
            for ($n = 0; $n -lt $count; $n++) {
                $r = Send-UI4MELiveBitmap -Bitmap $b -Pixels $frames[$n % 2]
                if ($n -lt 3 -or $n -ge $count - 2) { $sequences.Add([long]$b.Sequence) }
                if (($n % 100) -eq 0) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 0) }
            }
            $elapsed = $clock.Elapsed.TotalSeconds
            Assert-True ($b.Sequence -eq $count) 'sequence counts every frame'
            Assert-True ($b.Stride -eq $w * 4) 'tight stride'
            Assert-True ($count / $elapsed -ge 30) "at least 30 frames/s at ${w}x${h} (got $([Math]::Round($count / $elapsed, 1)))"
            $report["${w}x${h}"] = @{ frames = $count; seconds = [Math]::Round($elapsed, 3); fps = [Math]::Round($count / $elapsed, 1)
                last_result = $r; process_working_set_mb = [Math]::Round((Get-Process -Id $PID).WorkingSet64 / 1MB, 1); sequences = $sequences.ToArray() }
            } finally { Remove-UI4MELiveBitmap -Bitmap $b -Quiet }
            Assert-True $b.Closed 'closed'
            $stale = $false
            try { $null = Send-UI4MELiveBitmap -Bitmap $b -Pixels $frames[0] } catch { $stale = $true }
            Assert-True $stale 'stale handle rejected'
        }
        [GC]::Collect()
        $report.managed_heap_growth_kb = [Math]::Round(([GC]::GetTotalMemory($true) - $before) / 1KB, 1)
        $report
    }
    Check 'bitmap files are removed after close (temp/runtime dir)' {
        $b = New-UI4MELiveBitmap -Document $doc -Id frames -Width 16 -Height 16 -Slots 3
        $created = Invoke-UI4MERpc -Application $app -Method 'host.diagnostics'
        $null = Send-UI4MELiveBitmap -Bitmap $b -Pixels (New-Object byte[] 1024)
        Remove-UI4MELiveBitmap -Bitmap $b
        $dirs = @([IO.Path]::GetTempPath())
        if ($env:XDG_RUNTIME_DIR) { $dirs += $env:XDG_RUNTIME_DIR }
        $left = @()
        foreach ($d in $dirs) { $left += @(Get-ChildItem -LiteralPath $d -Force -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer -and $_.Name -like 'ui4me-bitmap-*' -and $_.LastWriteTime -gt (Get-Date).AddMinutes(-5) } | ForEach-Object FullName) }
        Assert-True ($left.Count -eq 0) "no bitmap files left: $($left -join ', ')"
        @{ scanned = $dirs }
    }
    Check 'multiple windows, close veto then allow, closed event retires handles' {
        $second = New-UI4MEWindow -Application $app -Title 'Second window' -Width 420 -Height 300
        $secondDoc = Open-UI4MEHtml -Window $second -Html '<p id="p">second</p>' -AssetRoot $assets
        $script:asked = 0
        Set-UI4MEBeforeClose -Window $second -Callback { $script:asked++; $script:asked -gt 1 }
        Close-UI4MEWindow -Window $second
        Assert-True (Pump-Until { $script:asked -ge 1 } 5000) 'veto callback ran'
        Start-Sleep -Milliseconds 300; [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 50)
        Assert-True (-not $second.Closed) 'vetoed window still open'
        Close-UI4MEWindow -Window $second
        Assert-True (Pump-Until { $second.Closed } 5000) 'allowed close retires the window'
        Assert-True $secondDoc.Retired 'its document is retired'
        $stale = $false
        try { Get-UI4MEDocumentText -Document $secondDoc -Element p } catch [UI4ME.StaleObjectException] { $stale = $true }
        Assert-True $stale 'stale document rejected locally'
        @{ asked = $script:asked; windows = (Get-UI4MEWindowList -Application $app) }
    }
    Check 'document replacement makes old handles stale; host rejects stale ids' {
        $old = $doc
        $oldNode = Get-UI4MENode -Document $old -Element heading
        $script:doc = Open-UI4MEFile -Window $win -Path $htmlPath
        $doc = $script:doc
        $null = Register-UI4MEResources -Document $doc -Name generated -Entries @{ 'dot.png' = @{ Bytes = (Get-Png1x1); Mime = 'image/png' } }
        $local = $false
        try { $null = Get-UI4MENodeInfo -Node $oldNode } catch [UI4ME.StaleObjectException] { $local = $true }
        $hostCode = $null
        try { $null = Invoke-UI4MERpc -Application $app -Method 'document.command' -Parameters @{ window_id = $win.Id; document_id = $old.Id; command = @{ op = 'get_text'; element = 'heading' } } }
        catch [UI4ME.RpcException] { $hostCode = $_.Exception.Code }
        Assert-True $local 'local stale detection'
        Assert-True ($null -ne $hostCode) 'host refuses stale document id'
        $hostNode = $null
        try { $null = Invoke-UI4MEDocumentCommand -Document $doc -Operation get_node -Parameters @{ node = $oldNode.Id } } catch [UI4ME.RpcException] { $hostNode = $_.Exception.Code }
        Assert-True ($null -ne $hostNode) 'host refuses node from previous generation'
        @{ host_code = $hostCode; host_node_code = $hostNode; new_document = $doc.Id }
    }
    Check 'timeouts and cancellation leave the transport healthy' {
        $request = Start-UI4MERpc -Application $app -Method 'host.capabilities'
        $result = Wait-UI4MERpc -Request $request -TimeoutMilliseconds 5000
        Assert-True ($null -ne $result) 'reply'
        Assert-True ($app.Transport.PendingCount -eq 0 -and $app.Pending.Count -eq 0) 'nothing pending'
        @{ dropped_notifications = $app.Transport.DroppedNotifications; deferred_dropped = $app.DroppedNotifications }
    }

    if ($mode -eq 'auto') {
        Write-Record @{ kind = 'done'; failures = $script:failures }
        Request-UI4MEQuit -Application $app | Out-Null
        Wait-UI4ME -Application $app -UntilShutdown
        return
    }

    # ----------------------------------------------- interactive checks ---
    $script:events = New-Object System.Collections.ArrayList
    $sink = $script:events
    foreach ($type in @('click', 'auxclick', 'contextmenu', 'pointerdown', 'input', 'change', 'keydown', 'focus', 'submit', 'compositionend')) {
        $t = $type
        $null = Register-UI4MEDocumentEvent -Document $doc -Type $t -Callback ({ param($e) [void]$sink.Add($e) }.GetNewClosure())
    }
    function Wait-Event([string]$Type, [string]$Element, [int]$Milliseconds = 10000, [scriptblock]$Where = { $true }) {
        $ok = Pump-Until { foreach ($e in $script:events) { if ($e.type -eq $Type -and (-not $Element -or $e.element -eq $Element) -and (& $Where $e)) { $script:hit = $e; return $true } }; return $false } $Milliseconds
        if (-not $ok) { throw "No $Type event on '$Element' (seen: $((@($script:events) | ForEach-Object { "$($_.type):$($_.element)" }) -join ' '))" }
        return $script:hit
    }

    function Restore-QualificationWindow {
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            $null = Set-UI4MEWindow -Window $win -Minimized $false -Focus $true
        } else {
            # Wayland: a client cannot unminimize or focus itself; the user (driver) activates it.
            $null = Wait-Ack (Request-Input @{ action = 'activate' })
        }
    }
    Check 'real pointer: left click, right click (contextmenu), middle click (auxclick)' {
        $script:events.Clear()
        $c = Get-Center target
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $c.x; y = $c.y })
        $click = Wait-Event click target
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'right'; x = $c.x; y = $c.y })
        $menu = Wait-Event contextmenu target
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'middle'; x = $c.x; y = $c.y })
        $aux = Wait-Event auxclick target -Where { param($e) $e.button -eq 1 }
        @{ click = $click; contextmenu = $menu; auxclick = $aux }
    }
    Check 'real keyboard into a focused input; input/change events and value' {
        $script:events.Clear()
        Set-UI4MEDocumentValue -Document $doc -Element name -Value ''
        $c = Get-Center name
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $c.x; y = $c.y })
        $null = Wait-Ack (Request-Input @{ action = 'type'; text = 'Hello PS' })
        $null = Wait-Event input name
        $ok = Pump-Until { (Get-UI4MEValue -Document $doc -Element name) -eq 'Hello PS' } 5000
        Assert-True $ok "typed value is '$(Get-UI4MEValue -Document $doc -Element name)'"
        $null = Wait-Ack (Request-Input @{ action = 'key'; key = 'Tab' })
        $change = Wait-Event change name
        @{ inputs = @($script:events | Where-Object { $_.type -eq 'input' }).Count; change = $change }
    }
    Check 'prevent-default registered before input stops native form submission' {
        $script:events.Clear()
        Set-UI4MEPreventDefault -Document $doc -Element form -EventType submit
        $c = Get-Center submit
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $c.x; y = $c.y })
        $submit = Wait-Event submit form
        @{ submit = $submit }
    }
    Check 'callback-issued RPC while further input arrives keeps order' {
        $script:events.Clear()
        $script:order = New-Object System.Collections.ArrayList
        $orderSink = $script:order
        $s1 = Register-UI4MEDocumentEvent -Document $doc -Type click -Element target -Callback ({
            [void]$orderSink.Add('target-start')
            Set-UI4MEDocumentText -Document $doc -Element status -Text 'slow handler'
            Start-Sleep -Milliseconds 400
            $null = Get-UI4MEDocumentText -Document $doc -Element status
            [void]$orderSink.Add('target-end')
        }.GetNewClosure())
        $s2 = Register-UI4MEDocumentEvent -Document $doc -Type click -Element other -Callback ({ [void]$orderSink.Add('other') }.GetNewClosure())
        $a = Get-Center target; $b = Get-Center other
        $null = Wait-Ack (Request-Input @{ action = 'clicks'; points = @(@{ x = $a.x; y = $a.y }, @{ x = $b.x; y = $b.y }) })
        Assert-True (Pump-Until { $script:order.Count -ge 3 } 8000) "order events: $($script:order -join ',')"
        Unregister-UI4MEDocumentEvent -Subscription $s1; Unregister-UI4MEDocumentEvent -Subscription $s2
        Assert-True (($script:order -join ',') -eq 'target-start,target-end,other') "order: $($script:order -join ',')"
        @{ order = $script:order.ToArray() }
    }
    Check 'native save dialog: path with spaces and non-ASCII name' {
        $dir = Join-Path $assets 'save here'
        $null = New-Item -ItemType Directory -Path $dir -Force
        $request = Show-UI4MEFileDialog -Window $win -Kind SaveFile -Title 'PS save test' -Directory $dir -FileName 'zápis ü.txt' -NoWait
        $null = Wait-Ack (Request-Input @{ action = 'dialog-accept'; title = 'PS save test' }) 60000
        $result = Wait-UI4MERpc -Request $request -TimeoutMilliseconds 30000
        @{ result = $result; expected_directory = $dir }
    }
    Check 'native open dialog cancelled from PowerShell (Stop-UI4MERpc)' {
        $request = Show-UI4MEFileDialog -Window $win -Kind OpenFiles -Title 'PS cancel test' -Directory $assets -NoWait
        $seen = Wait-Ack (Request-Input @{ action = 'dialog-observe'; title = 'PS cancel test' }) 60000
        Stop-UI4MERpc -Request $request
        $gone = Wait-Ack (Request-Input @{ action = 'dialog-gone'; title = 'PS cancel test' }) 60000
        Assert-True ([bool]$gone.gone) 'native dialog closed after cancellation'
        @{ observed = $seen; gone = $gone }
    }
    Check 'native maximize/minimize via window state, restore' {
        $null = Set-UI4MEWindow -Window $win -Maximized $true
        Assert-True (Pump-Until { [bool](Get-UI4MEWindow -Window $win).observed.maximized } 5000) 'maximized'
        $null = Set-UI4MEWindow -Window $win -Maximized $false
        Assert-True (Pump-Until { -not [bool](Get-UI4MEWindow -Window $win).observed.maximized } 5000) 'restored'
        $null = Set-UI4MEWindow -Window $win -Minimized $true
        $min = Wait-Ack (Request-Input @{ action = 'observe-window'; expect = 'minimized' }) 15000
        Restore-QualificationWindow
        $restored = Wait-Ack (Request-Input @{ action = 'observe-window'; expect = 'normal' }) 15000
        @{ minimized = $min; restored = $restored }
    }

    # --- frameless chrome: switch decorations off and test drag and resize ---
    Check 'frameless: titlebar drag moves the window' {
        $null = Set-UI4MEWindow -Window $win -Decorations $false
        Start-Sleep -Milliseconds 500; [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 50)
        $c = Get-Center caption
        $a = Wait-Ack (Request-Input @{ action = 'drag'; x = $c.x; y = $c.y; dx = 120; dy = 70; measure = 'move' }) 20000
        Assert-True ([bool]$a.moved) "window moved: $([UI4ME.Json]::Serialize($a))"
        $a
    }
    Check 'frameless: eight resize edges resize the window' {
        $outcomes = @{}
        foreach ($edge in @('n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw')) {
            # Start every edge from the same size (and, where the platform allows, the same
            # place) so that growing never reaches a screen edge and triggers snapping.
            $reset = @{ Window = $win; Width = 720; Height = 520 }
            if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) { $reset.X = 300; $reset.Y = 200 }
            $null = Set-UI4MEWindow @reset
            $null = Pump-Until { $false } 600
            $state = Get-UI4MEWindow -Window $win
            $wv = [double]$state.width; $hv = [double]$state.height
            $x = switch -Wildcard ($edge) { '*e' { $wv - 3 } '*w' { 3 } default { $wv / 2 } }
            $y = switch -Wildcard ($edge) { 'n*' { 3 } 's*' { $hv - 3 } default { $hv / 2 } }
            $dx = switch -Wildcard ($edge) { '*e' { 60 } '*w' { -60 } default { 0 } }
            $dy = switch -Wildcard ($edge) { 'n*' { -50 } 's*' { 50 } default { 0 } }
            $a = Wait-Ack (Request-Input @{ action = 'drag'; x = $x; y = $y; dx = $dx; dy = $dy; measure = 'resize'; edge = $edge }) 20000
            $outcomes[$edge] = $a
            Assert-True ([bool]$a.resized) "edge $edge resized: $([UI4ME.Json]::Serialize($a))"
        }
        $outcomes
    }
    Check 'frameless: HTML maximize/minimize/close buttons drive native state' {
        $script:chrome = New-Object System.Collections.ArrayList
        $chromeSink = $script:chrome
        $s = @(
            (Register-UI4MEDocumentEvent -Document $doc -Type click -Element maximize -Callback ({
                $st = Get-UI4MEWindow -Window $win
                $null = Set-UI4MEWindow -Window $win -Maximized (-not [bool]$st.observed.maximized); [void]$chromeSink.Add('maximize') }.GetNewClosure())),
            (Register-UI4MEDocumentEvent -Document $doc -Type click -Element minimize -Callback ({
                $null = Set-UI4MEWindow -Window $win -Minimized $true; [void]$chromeSink.Add('minimize') }.GetNewClosure()))
        )
        $m = Get-Center maximize
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $m.x; y = $m.y })
        Assert-True (Pump-Until { [bool](Get-UI4MEWindow -Window $win).observed.maximized } 5000) 'maximized by HTML button'
        $m = Get-Center maximize
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $m.x; y = $m.y })
        Assert-True (Pump-Until { -not [bool](Get-UI4MEWindow -Window $win).observed.maximized } 5000) 'restored by HTML button'
        $m = Get-Center minimize
        $null = Wait-Ack (Request-Input @{ action = 'click'; button = 'left'; x = $m.x; y = $m.y })
        $min = Wait-Ack (Request-Input @{ action = 'observe-window'; expect = 'minimized' }) 15000
        Restore-QualificationWindow
        $null = Wait-Ack (Request-Input @{ action = 'observe-window'; expect = 'normal' }) 15000
        foreach ($x in $s) { Unregister-UI4MEDocumentEvent -Subscription $x }
        @{ clicks = $script:chrome.ToArray(); minimized = $min }
    }

    # --- native close: veto once, then allow; the host shuts the app down ---
    $script:closeAsks = 0
    Set-UI4MEBeforeClose -Window $win -Callback { $script:closeAsks++; Write-Record @{ kind = 'before_close'; n = $script:closeAsks }; $script:closeAsks -ge 2 }
    Check 'native close request is vetoed by PowerShell policy' {
        $null = Wait-Ack (Request-Input @{ action = 'native-close' })
        Assert-True (Pump-Until { $script:closeAsks -ge 1 } 10000) 'before_close callback ran'
        Start-Sleep -Milliseconds 500; [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 50)
        Assert-True (-not $win.Closed) 'window survived veto'
        @{ asks = $script:closeAsks }
    }
    Write-Record @{ kind = 'done'; failures = $script:failures }
    $null = Request-Input @{ action = 'native-close'; final = $true }
    Wait-UI4ME -Application $app -UntilShutdown
    Write-Record @{ kind = 'shutdown'; asks = $script:closeAsks; window_closed = $win.Closed }
} catch {
    Write-Record @{ kind = 'fatal'; error = "$($_.Exception.GetType().FullName): $($_.Exception.Message)"; at = "$($_.InvocationInfo.ScriptLineNumber)" }
    throw
} finally {
    Close-UI4ME -Application $app
    if ($assets -and (Test-Path -LiteralPath $assets)) { Remove-Item -LiteralPath $assets -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Record @{ kind = 'exit'; ms = $started.ElapsedMilliseconds }
}
