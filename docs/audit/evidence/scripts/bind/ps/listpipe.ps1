$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$null = New-UI4MEWindow -Application $app
"pipeline count of Get-UI4MEWindowList: $((Get-UI4MEWindowList -Application $app | Measure-Object).Count)  (assigned count: $((Get-UI4MEWindowList -Application $app).Count))"
$r = Invoke-UI4MERpc -Application $app -Method 'test.array1'
"Invoke-UI4MERpc single-element array result type: $(if ($null -eq $r) {'null'} else {$r.GetType().Name})"
$r = Invoke-UI4MERpc -Application $app -Method 'test.array0'
"Invoke-UI4MERpc empty array result: $(if ($null -eq $r) {'$null'} else {$r.GetType().Name})"
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
