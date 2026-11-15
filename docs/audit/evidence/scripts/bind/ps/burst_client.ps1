param([int]$N = 1000, [string]$Mode = 'toplevel', [int]$HandlerMs = 0)
$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p>x</p>'
$script:count = 0; $script:clicks = 0; $script:closed = $false
$null = Register-UI4MEDocumentEvent -Document $doc -Type transitionrun -Callback { $script:count++; if ($HandlerMs) { Start-Sleep -Milliseconds $HandlerMs } }
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Callback {
    $script:clicks++
    if ($Mode -eq 'inrpc') {
        # A click handler that issues commands while a burst arrives.
        $null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = $N; close = $true }
        for ($i = 0; $i -lt 20; $i++) { $null = Invoke-UI4MERpc -Application $app -Method 'noop' }
    }
}
Register-UI4MENotification -Application $app -Method window.event -Callback { param($p) if ($p.type -eq 'closed') { $script:closed = $true } }
$sw = [Diagnostics.Stopwatch]::StartNew()
if ($Mode -eq 'toplevel') {
    $null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = $N; click = $true; close = $true }
} else {
    $null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 0; click = $true }
}
$deadline = [DateTime]::UtcNow.AddSeconds(30)
while ([DateTime]::UtcNow -lt $deadline) {
    $h = Wait-UI4ME -Application $app -TimeoutMilliseconds 300
    if ($h -eq 0) { break }
}
"mode=$Mode N=$N handled transitionrun=$($script:count) clicks=$($script:clicks) closedCallback=$($script:closed) window.Closed=$($w.Closed) transportDropped=$($app.Transport.DroppedNotifications) deferredDropped=$($app.DroppedNotifications) elapsed=$($sw.ElapsedMilliseconds)ms rate=$([int]($script:count / [Math]::Max(0.001, ($sw.ElapsedMilliseconds - 300) / 1000.0)))/s"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
