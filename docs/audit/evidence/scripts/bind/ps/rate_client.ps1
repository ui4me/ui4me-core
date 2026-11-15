$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p>x</p>'
$script:sw = [Diagnostics.Stopwatch]::new(); $script:n = 0; $script:last = 0
$null = Register-UI4MEDocumentEvent -Document $doc -Type transitionrun -Callback { if ($script:n -eq 0) { $script:sw.Start() }; $script:n++; $script:last = $script:sw.Elapsed.TotalMilliseconds }
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 120 }
Start-Sleep -Milliseconds 300   # let the whole burst arrive in the queue
while ((Wait-UI4ME -Application $app -TimeoutMilliseconds 200) -gt 0) { }
"events=$($script:n) handler-span=$([math]::Round($script:last,1))ms per-event=$([math]::Round($script:last/[math]::Max(1,$script:n-1),3))ms"
# unsubscribed events: cost of an event nobody listens to
$script:sw2 = [Diagnostics.Stopwatch]::StartNew()
$null = Invoke-UI4MERpc -Application $app -Method 'test.burst' -Parameters @{ n = 120; type = 'pointermove' }
Start-Sleep -Milliseconds 300
$t0 = $script:sw2.Elapsed.TotalMilliseconds; $h = 0
while (($k = Wait-UI4ME -Application $app -TimeoutMilliseconds 0) -gt 0) { $h += $k }
"unsubscribed events handled=$h in $([math]::Round($script:sw2.Elapsed.TotalMilliseconds - $t0,1))ms"
# RPC throughput
$sw3 = [Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt 500; $i++) { Set-UI4MEDocumentText -Document $doc -Element p -Text "t$i" }
"500 Set-UI4MEDocumentText in $($sw3.ElapsedMilliseconds)ms => $([int](500000/$sw3.ElapsedMilliseconds))/s"
$sw4 = [Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt 500; $i++) { $null = $app.Transport.Send(@{ jsonrpc='2.0'; method='noop.notify'; params=@{} }) }
"500 raw Transport.Send in $($sw4.ElapsedMilliseconds)ms"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
