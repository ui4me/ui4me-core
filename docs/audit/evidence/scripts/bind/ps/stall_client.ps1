$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$null = Invoke-UI4MERpc -Application $app -Method 'test.stall'
$big = 'x' * 1500000
$sw = [Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt 3; $i++) {
  $t = $sw.ElapsedMilliseconds
  try { $null = Invoke-UI4MERpc -Application $app -Method 'noop' -Parameters @{ big = $big } -TimeoutMilliseconds 1000; "call $i returned after $($sw.ElapsedMilliseconds - $t) ms (timeout was 1000)" } catch { "call $i threw after $($sw.ElapsedMilliseconds - $t) ms: $($_.Exception.GetType().Name)" }
}
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
