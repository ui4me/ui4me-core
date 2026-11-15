$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p>x</p>'
$status = 'script-level'
$script:out = New-Object System.Collections.ArrayList
$null = Register-UI4MEDocumentEvent -Document $doc -Type transitionrun -Callback { [void]$script:out.Add("status=$status"); $counter++ }
$counter = 0
function Invoke-Work {
    $status = 'function-local'
    $null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 1 }
    $null = Invoke-UI4MERpc -Application $app -Method 'test.slow' -Parameters @{ delay = 0.3 }
}
Invoke-Work
for ($i = 0; $i -lt 3; $i++) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 100) }
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 1 }
for ($i = 0; $i -lt 3; $i++) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 100) }
"observations: $($script:out -join '; ')  counter-after=$counter"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
