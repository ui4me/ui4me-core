param([string]$Module)
$ErrorActionPreference = 'Stop'
Import-Module $Module
$app = Start-UI4MEScript -Name 'sig' -Instance multiple
$window = New-UI4MEWindow -Application $app
$null = Open-UI4MEHtml -Window $window -Html '<p>x</p>'
Set-UI4MEReady -Application $app
[Console]::Out.WriteLine('READY')
Wait-UI4ME -Application $app -UntilShutdown
[Console]::Out.WriteLine('UNTIL-SHUTDOWN-RETURNED')
Close-UI4ME -Application $app
