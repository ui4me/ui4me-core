$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$req = Show-UI4MEFileDialog -Window $w -Kind OpenFiles -NoWait
$sw = [Diagnostics.Stopwatch]::StartNew()
try { $r = Wait-UI4MERpc -Request $req; "result: $r" } catch { "Wait-UI4MERpc threw after $($sw.ElapsedMilliseconds) ms: $($_.Exception.GetType().Name): $($_.Exception.Message)" }
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
