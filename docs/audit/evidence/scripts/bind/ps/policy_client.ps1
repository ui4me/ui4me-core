param([string]$Case = 'busy')
$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
switch ($Case) {
  'busy'      { Set-UI4MEBeforeClose -Window $w -Callback { $true } }
  'nooutput'  { Set-UI4MEBeforeClose -Window $w -Callback { $null = 1 } }
  'stringfalse' { Set-UI4MEBeforeClose -Window $w -Callback { 'false' } }
  'listadd'   { $script:l = New-Object System.Collections.ArrayList; Set-UI4MEBeforeClose -Window $w -Callback { $false; $script:l.Add('closing') } }
  'throws'    { Set-UI4MEBeforeClose -Window $w -Callback { throw 'boom' } }
  'break'     { Set-UI4MEBeforeClose -Window $w -Callback { if ($true) { break }; $true } }
}
$null = Invoke-UI4MERpc -Application $app -Method 'test.before_close'
if ($Case -eq 'busy') { Start-Sleep -Seconds 3 }  # long synchronous work with no pumping
for ($i = 0; $i -lt 5; $i++) { [void](Wait-UI4ME -Application $app -TimeoutMilliseconds 100) }
$null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown'
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
