param([string]$Tag = 'none')
$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Start-UI4MEScript -Name 'forever' -Instance multiple
"backend started tag=$Tag pid=$PID"
$w = New-UI4MEWindow -Application $app -Title t
Set-UI4MEReady -Application $app
Register-UI4MENotification -Application $app -Method 'app.shutdown' -Callback { [Console]::Out.WriteLine("backend saw app.shutdown") }
try { Wait-UI4ME -Application $app -UntilShutdown; "backend: orderly shutdown" } finally { "backend finally"; Close-UI4ME -Application $app }
