$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core -RequiredVersion 1.0.1
$app = Start-UI4MEScript -Name 'PSG clean check' -Instance multiple
$window = New-UI4MEWindow -Application $app -Title 'x' -Width 300 -Height 200
Set-UI4MEReady -Application $app
[Console]::Out.WriteLine('PS-APP-CONNECTED')
$null = Request-UI4MEQuit -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
