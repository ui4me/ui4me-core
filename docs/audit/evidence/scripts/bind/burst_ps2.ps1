param([int]$N = 20, [int]$Rounds = 6, [int]$Sub = 1)
$ErrorActionPreference = 'Stop'
Import-Module '<scratch>/exp/BIND/burst/psmod/UI4ME.Core/UI4ME.Core.psd1'
$app = Start-UI4MEScript -Name 'PS burst BIND' -AppId 'org.ui4me.audit.BindPsBurst' -Instance multiple -ArgumentList @("-N", "$N", "-Rounds", "$Rounds", "-Sub", "$Sub")
$css = 'button { transition: background-color 120ms, border-color 120ms, color 120ms; background:#eee; border:1px solid #999; color:#111 } @media (prefers-color-scheme: dark) { button { background:#333; border-color:#777; color:#eee } }'
$buttons = (0..($N-1) | ForEach-Object { "<button id=`"b$_`">B$_</button>" }) -join ''
$window = New-UI4MEWindow -Application $app -Title 'burst' -Width 800 -Height 600
$doc = Open-UI4MEHtml -Window $window -Html "<style>$css</style><div id=root>$buttons</div><p id=s>x</p>"
$script:count = 0
if ($Sub) { foreach ($t in 'transitionrun','transitionstart','transitionend') { $null = Register-UI4MEDocumentEvent -Document $doc -Type $t -Callback { $script:count++ } } }
Set-UI4MEReady -Application $app
try {
  $null = Wait-UI4ME -Application $app -TimeoutMilliseconds 1000
  for ($r = 0; $r -lt $Rounds; $r++) {
    $scheme = if ($r % 2 -eq 0) { 'dark' } else { 'light' }
    $null = Set-UI4MEEnvironment -Document $doc -ColorScheme $scheme
    $end = (Get-Date).AddMilliseconds(500)
    while ((Get-Date) -lt $end) { $null = Wait-UI4ME -Application $app -TimeoutMilliseconds 50 }
    $null = Get-UI4MEDocumentText -Document $doc -Element s
    $arr = @($app.Transport.AuditArrivals.ToArray()); while ($app.Transport.AuditArrivals.Count) { $x = 0L; $null = $app.Transport.AuditArrivals.TryDequeue([ref]$x) }; $msg = "frames=$($arr.Count)"; if ($arr.Count -gt 1) { $f = [Diagnostics.Stopwatch]::Frequency; $t0 = $arr[0]; $c1 = @($arr | Where-Object { ($_ - $t0) / $f -lt 0.05 }); $ms = 1000.0 * ($c1[-1] - $t0) / $f; $msg += (" cluster1={0} in {1:N2}ms => reader {2:N0} frames/s" -f $c1.Count, $ms, (($c1.Count - 1) / [Math]::Max($ms / 1000.0, 1e-9))) }; [Console]::Out.WriteLine("ROUND_OK $r callbacks=$script:count dropped=$($app.Transport.DroppedNotifications) $msg")
  }
  [Console]::Out.WriteLine('PROBE_PASS')
} catch { [Console]::Out.WriteLine("PROBE_FAIL $($_.Exception.GetType().Name) $($_.Exception.Message)") }
try { $null = Request-UI4MEQuit -Application $app; Wait-UI4ME -Application $app -UntilShutdown } catch {}
Close-UI4ME -Application $app
