Set-Location 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd'
Remove-Item psv-*.txt, pyw-*.txt -ErrorAction SilentlyContinue
foreach ($n in 10, 20) {
  $p = Start-Process -FilePath powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"`$PSVersionTable.PSVersion.ToString() + ' ' + `$PSVersionTable.PSEdition | Out-File psv-$n.ver.txt; & .\burst.ps1 -N $n -Rounds 6`"" -RedirectStandardOutput "psv-$n.out.txt" -RedirectStandardError "psv-$n.err.txt" -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(40000)) { "TIMEOUT" | Out-File -Append "psv-$n.out.txt"; Get-Process ui4me-host -ErrorAction SilentlyContinue | Stop-Process -Force }
  Start-Sleep 2
}
# pythonw launch: does the host get a console?
$pyw = (Resolve-Path .\venv\Scripts\pythonw.exe).Path
$p = Start-Process -FilePath $pyw -ArgumentList "churn.py 30" -PassThru
Start-Sleep 6
$hostp = Get-CimInstance Win32_Process -Filter "Name='ui4me-host.exe'"
$out = foreach ($h in $hostp) {
  $kids = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($h.ProcessId)" | ForEach-Object { $_.Name }
  $gp = Get-Process -Id $h.ProcessId
  "host pid=$($h.ProcessId) parent=$($h.ParentProcessId) children=[$($kids -join ',')] mainWindowTitle='$($gp.MainWindowTitle)'"
}
$cons = Get-Process conhost -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq (Get-Process -Id $PID).SessionId } | ForEach-Object { "conhost pid=$($_.Id) title='$($_.MainWindowTitle)' handle=$($_.MainWindowHandle)" }
($out + $cons) | Out-File pyw-result.txt
Get-Process ui4me-host, pythonw -ErrorAction SilentlyContinue | Stop-Process -Force
