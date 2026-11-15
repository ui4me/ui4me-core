param([int]$N = 20, [int]$Rounds = 6)
$ErrorActionPreference = 'Stop'
Import-Module 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd\psmod\UI4ME.Core'
$app = Start-UI4MEScript -Name 'PS burst' -AppId 'org.ui4me.audit.PsBurst' -Instance multiple -ArgumentList @("-N", "$N", "-Rounds", "$Rounds")
$css = 'button { transition: background-color 120ms, border-color 120ms, color 120ms; background:#eee; border:1px solid #999; color:#111 } @media (prefers-color-scheme: dark) { button { background:#333; border-color:#777; color:#eee } }'
$buttons = (0..($N-1) | ForEach-Object { "<button id=`"b$_`">B$_</button>" }) -join ''
$window = New-UI4MEWindow -Application $app -Title 'burst' -Width 800 -Height 600
$doc = Open-UI4MEHtml -Window $window -Html "<style>$css</style><div id=root>$buttons</div><p id=s>x</p>"
$script:count = 0
foreach ($t in 'transitionrun','transitionstart','transitionend') { $null = Register-UI4MEDocumentEvent -Document $doc -Type $t -Callback { $script:count++ } }
Set-UI4MEReady -Application $app
try {
  $null = Wait-UI4ME -Application $app -TimeoutMilliseconds 1000
  for ($r = 0; $r -lt $Rounds; $r++) {
    $scheme = if ($r % 2 -eq 0) { 'dark' } else { 'light' }
    $null = Set-UI4MEEnvironment -Document $doc -ColorScheme $scheme
    $end = (Get-Date).AddMilliseconds(400)
    while ((Get-Date) -lt $end) { $null = Wait-UI4ME -Application $app -TimeoutMilliseconds 50 }
    $null = Get-UI4MEDocumentText -Document $doc -Element s
    [Console]::Out.WriteLine("ROUND_OK $r events $script:count")
  }
  [Console]::Out.WriteLine('PROBE_PASS')
} catch { [Console]::Out.WriteLine("PROBE_FAIL $($_.Exception.GetType().Name) $($_.Exception.Message)") }
try { $null = Request-UI4MEQuit -Application $app; Wait-UI4ME -Application $app -UntilShutdown } catch {}
Close-UI4ME -Application $app
