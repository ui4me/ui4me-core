Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
& (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
foreach ($n in 'psobject','PSBase','psadapted','psextended','pstypenames','psbase2','Count','Length','GetType','ToString','Equals','') {
  try { $v = [UI4ME.Json]::Parse('{"' + $n + '":1}'); "ok   '$n' -> props: $(@($v.PSObject.Properties.Name) -join ',')" } catch { "FAIL '$n' -> $($_.Exception.InnerException.GetType().FullName): $($_.Exception.InnerException.Message)" }
}
