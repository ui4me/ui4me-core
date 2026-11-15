$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$window = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $window -Html '<p>x</p>'
function Update-Status([string]$t) { "Update-Status($t)" }
$script:out = New-Object System.Collections.ArrayList
$out2 = $script:out
$script:counter = 0
function Add-Handler($Document, [string]$Label) {
    $sink = $script:out
    $null = Register-UI4MEDocumentEvent -Document $Document -Type transitionrun -Callback ({
        $a = try { Update-Status $Label } catch { "function call failed: $($_.Exception.Message)" }
        $b = "window-visible=$($null -ne $window)"
        $script:counter++
        [void]$sink.Add("$a | $b | closure script:counter=$script:counter")
    }.GetNewClosure())
}
Add-Handler $doc 'L1'
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 2 }
for ($i = 0; $i -lt 3; $i++) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 100) }
$script:out | ForEach-Object { "  $_" }
"script counter=$script:counter"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
