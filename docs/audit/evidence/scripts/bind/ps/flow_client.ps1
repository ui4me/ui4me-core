param([string]$Case = 'break')
$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p>x</p>'
$script:seen = New-Object System.Collections.ArrayList
$null = Register-UI4MEDocumentEvent -Document $doc -Type transitionrun -Callback {
    param($e)
    if ($e.seq -eq 1) {
        if ($Case -eq 'break') { [void]$script:seen.Add("h1-$($e.seq)-break"); break }
        if ($Case -eq 'continue') { [void]$script:seen.Add("h1-$($e.seq)-continue"); continue }
        if ($Case -eq 'exit') { exit 7 }
        if ($Case -eq 'return') { return }
    }
    [void]$script:seen.Add("h1-$($e.seq)")
}
$null = Register-UI4MEDocumentEvent -Document $doc -Type transitionrun -Callback { param($e) [void]$script:seen.Add("h2-$($e.seq)") }
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 3 }
for ($i = 0; $i -lt 5; $i++) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 100) }
"case=$Case seen: $($script:seen -join ' ')"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
