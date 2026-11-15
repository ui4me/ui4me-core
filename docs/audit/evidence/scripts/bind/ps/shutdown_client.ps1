$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p>x</p>'
$script:closedSeen = $false
Register-UI4MENotification -Application $app -Method window.event -Callback { param($p) if ($p.type -eq 'closed') { $script:closedSeen = $true } }
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Callback {
    $null = Invoke-UI4MERpc -Application $app -Method 'test.close_and_shutdown'
    $null = Invoke-UI4MERpc -Application $app -Method 'test.slow' -Parameters @{ delay = 0.2 }
}
Set-UI4MEReady -Application $app
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 0; click = $true }
Wait-UI4ME -Application $app -UntilShutdown
"after UntilShutdown: closed-callback-ran=$($script:closedSeen) window.Closed=$($w.Closed) deferred-left=$($app.Deferred.Count)"
Close-UI4ME -Application $app
