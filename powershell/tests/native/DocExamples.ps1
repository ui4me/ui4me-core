# Runs the code examples from docs/guides/powershell.md, docs/reference/powershell-api.md,
# skills/ui4me-powershell/SKILL.md and README.md against the real host (app-docs.toml),
# with real input from the platform driver. Same JSONL protocol as Qualify.ps1.
$ErrorActionPreference = 'Stop'
if ($env:UI4ME_PS_MODULE) { Import-Module $env:UI4ME_PS_MODULE -Force } else { Import-Module UI4ME.Core }
$resultsFile = Join-Path $env:UI4ME_PS_RESULTS 'results.jsonl'
$acksFile = Join-Path $env:UI4ME_PS_RESULTS 'acks.jsonl'
$utf8 = New-Object Text.UTF8Encoding($false)
$script:requestId = 0
function Write-Record([hashtable]$Record) { $Record.t = [DateTime]::UtcNow.ToString('o'); [IO.File]::AppendAllText($resultsFile, ([UI4ME.Json]::Serialize($Record) + "`n"), $utf8) }
function Check([string]$Name, [scriptblock]$Body) {
    try { $d = & $Body; Write-Record @{ kind = 'check'; name = $Name; status = 'pass'; ms = 0; detail = $d } }
    catch { Write-Record @{ kind = 'check'; name = $Name; status = 'fail'; ms = 0; error = "$($_.Exception.GetType().FullName): $($_.Exception.Message)"; at = "$($_.InvocationInfo.ScriptLineNumber)" } }
}
function Assert-True($c, $m) { if (-not $c) { throw "Assertion failed: $m" } }
function Pump-Until([scriptblock]$Condition, [int]$Milliseconds = 10000) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Condition)) { if ($clock.ElapsedMilliseconds -ge $Milliseconds) { return $false }; [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 25) }
    return $true
}
function Click([string]$Element) {
    $r = (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_rect -Parameters @{ element = $Element }).rect
    $script:requestId++
    Write-Record @{ kind = 'request'; id = $script:requestId; action = 'click'; button = 'left'; x = $r.x + $r.width / 2; y = $r.y + $r.height / 2 }
    $id = $script:requestId
    $ok = Pump-Until { (Test-Path -LiteralPath $acksFile) -and ([IO.File]::ReadAllText($acksFile) -match ('"ack":\s*' + $id + '[,}]')) } 30000
    if (-not $ok) { throw "no ack for click on $Element" }
    [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 200)
}

$app = Connect-UI4ME
try {
    # --- guide/reference "How an app is structured" ---------------------------------
    $window = New-UI4MEWindow -Application $app -Title "UI4ME PS docs $PID" -Width 840 -Height 600
    $html = @'
<!doctype html>
<html lang="en"><head><meta charset="utf-8"></head>
<body>
  <input id="name"><button id="greet">Greet</button>
  <p id="result" aria-live="polite"></p>
  <button id="save">Save</button><p id="status"></p>
  <button id="inc">+</button><span id="count">0</span>
  <ul id="list"></ul>
  <form id="form"><input name="title" value="Draft"><button id="submit">Submit</button></form>
  <img id="preview" src="ui4me-bitmap://preview" width="320" height="200" alt="Preview">
  <img id="chart" src="ui4me-res://generated/chart.png" width="1" height="1" alt="Chart">
  <div style="height:1200px"></div><p id="row-40">far</p>
</body></html>
'@
    $doc = Open-UI4MEHtml -Window $window -Html $html -AssetRoot $PSScriptRoot

    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element greet -Callback {
        $name = (Get-UI4MEValue -Document $doc -Element name).Trim()
        Set-UI4MEDocumentText -Document $doc -Element result -Text "Hello, $name!"
    }
    # README snippet
    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element save -Callback {
        Set-UI4MEDocumentText -Document $doc -Element status -Text 'Saved'
    }
    # Guide: closure in a function
    function Add-Counter($Document, [string]$Button, [string]$Output) {
        $state = @{ n = 0 }
        $null = Register-UI4MEDocumentEvent -Document $Document -Type click -Element $Button -Callback ({
            $state.n++
            Set-UI4MEDocumentText -Document $Document -Element $Output -Text $state.n
        }.GetNewClosure())
    }
    Add-Counter $doc 'inc' 'count'
    # Reference: closure capturing a list
    $script:log = New-Object System.Collections.ArrayList
    function Add-SaveHandler($Document, [string]$Label) {
        $log = $script:log
        Register-UI4MEDocumentEvent -Document $Document -Type click -Element save -Callback ({
            param($e)
            [void]$log.Add("$Label clicked")
        }.GetNewClosure())
    }
    $null = Add-SaveHandler $doc 'docs'
    Set-UI4MEReady -Application $app
    Write-Record @{ kind = 'ready'; title = "UI4ME PS docs $PID" }

    Check 'guide: greet handler reads value and sets text' {
        Set-UI4MEDocumentValue -Document $doc -Element name -Value '  Grace '
        Click greet
        Assert-True (Pump-Until { (Get-UI4MEDocumentText -Document $doc -Element result) -eq 'Hello, Grace!' } 5000) 'greeting'
        Get-UI4MEDocumentText -Document $doc -Element result
    }
    Check 'README + reference: save handler and closure list' {
        Click save
        Assert-True (Pump-Until { (Get-UI4MEDocumentText -Document $doc -Element status) -eq 'Saved' -and $script:log.Count -eq 1 } 5000) 'saved and logged'
        $script:log[0]
    }
    Check 'guide: Add-Counter closure keeps its own state' {
        Click inc; Click inc
        Assert-True (Pump-Until { (Get-UI4MEDocumentText -Document $doc -Element count) -eq '2' } 5000) "count is $(Get-UI4MEDocumentText -Document $doc -Element count)"
        2
    }
    Check 'guide + skill: atomic batches' {
        Invoke-UI4MEBatch -Document $doc -Operations @(
            @{ op = 'set_text'; element = 'count'; text = '42' },
            @{ op = 'add_class'; element = 'count'; name = 'updated' }
        )
        Invoke-UI4MEBatch -Document $doc -Operations @(
            @{ op = 'set_text'; element = 'count'; text = '3' },
            @{ op = 'add_class'; element = 'count'; name = 'changed' },
            @{ op = 'set_attribute'; element = 'save'; name = 'disabled'; value = '' }
        )
        (Invoke-UI4MEDocumentCommand -Document $doc -Operation get_attributes -Parameters @{ element = 'count' })
    }
    Check 'skill: list from data with HtmlEncode' {
        $rows = @([pscustomobject]@{ Name = 'Tom & Jerry' }, [pscustomobject]@{ Name = '<b>bold</b>' })
        $items = $rows | ForEach-Object { '<li>' + [Net.WebUtility]::HtmlEncode($_.Name) + '</li>' }
        Set-UI4MEDocumentHtml -Document $doc -Element list -Html ($items -join '')
        $text = Get-UI4MEDocumentText -Document $doc -Element list
        Assert-True ($text -eq 'Tom & Jerry<b>bold</b>') "text '$text'"
        $text
    }
    Check 'skill: prevent submit, submit event, form data entries' {
        Set-UI4MEPreventDefault -Document $doc -Element form -EventType submit
        $script:entries = $null
        $null = Register-UI4MEDocumentEvent -Document $doc -Type submit -Element form -Callback {
            $script:entries = (Get-UI4MEFormData -Document $doc -Form form).entries
        }
        Click submit
        Assert-True (Pump-Until { $null -ne $script:entries } 5000) 'submit handler ran'
        Assert-True ($script:entries[0].name -eq 'title' -and $script:entries[0].value -eq 'Draft') 'entries'
        $script:entries
    }
    Check 'skill: window state and guide: observed maximize toggle' {
        $null = Set-UI4MEWindow -Window $window -Title 'Edited' -Maximized $true
        Assert-True (Pump-Until { (Get-UI4MEWindow -Window $window).observed.maximized } 5000) 'maximized'
        $state = Get-UI4MEWindow -Window $window
        $null = Set-UI4MEWindow -Window $window -Maximized (-not $state.observed.maximized)
        Assert-True (Pump-Until { -not (Get-UI4MEWindow -Window $window).observed.maximized } 5000) 'restored'
        $null = Set-UI4MEWindow -Window $window -Title "UI4ME PS docs $PID"
        (Get-UI4MEWindow -Window $window).title
    }
    Check 'skill: raw command scroll_into_view' {
        $null = Invoke-UI4MEDocumentCommand -Document $doc -Operation scroll_into_view -Parameters @{ element = 'row-40' }
        $scroll = Get-UI4MEScroll -Document $doc
        $scroll
    }
    Check 'guide: generated resources' {
        $pngBytes = [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==')
        $null = Register-UI4MEResources -Document $doc -Name generated -Entries @{
            'chart.png' = @{ Bytes = $pngBytes; Mime = 'image/png' }
        }
        Get-UI4MEResourceStats -Document $doc
    }
    Check 'guide + skill: live pixels snippet' {
        $bitmap = New-UI4MELiveBitmap -Document $doc -Id preview -Width 320 -Height 200
        $frame = New-Object byte[] (320 * 200 * 4)
        $r = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame
        Remove-UI4MELiveBitmap -Bitmap $bitmap
        $r
    }
    Check 'reference + skill: error handling snippet' {
        $out = $null
        try { Set-UI4MEDocumentText -Document $doc -Element missing -Text x }
        catch [UI4ME.RpcException] { $out = "Host refused: $($_.Exception.Code) $($_.Exception.RpcMessage)" }
        Assert-True ($out -match '^Host refused: -32\d{3} ') "got '$out'"
        $out
    }
    Check 'guide: clipboard snippet' {
        # The click checks above gave this window input, so Wayland allows the write.
        Set-UI4MEClipboard -Application $app -Text 'copied'
        Assert-True ((Get-UI4MEClipboard -Application $app) -eq 'copied') 'clipboard'
        'copied'
    }
    Check 'guide: unsaved-changes close policy' {
        $script:unsaved = $true
        Set-UI4MEBeforeClose -Window $window -Callback {
            -not $script:unsaved      # $true closes; $false keeps the window open
        }
        Close-UI4MEWindow -Window $window
        $null = Pump-Until { $false } 800
        Assert-True (-not $window.Closed) 'kept open while unsaved'
        $script:unsaved = $false
        'vetoed while unsaved'
    }
    Write-Record @{ kind = 'done' }
    Close-UI4MEWindow -Window $window
    Wait-UI4ME -Application $app -UntilShutdown
} catch {
    Write-Record @{ kind = 'fatal'; error = "$($_.Exception.GetType().FullName): $($_.Exception.Message)"; at = "$($_.InvocationInfo.ScriptLineNumber)" }
    throw
} finally {
    Close-UI4ME -Application $app
    Write-Record @{ kind = 'exit' }
}
