$ErrorActionPreference = 'Continue'
Set-Location 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd'
$env:SLOW = '0'
Remove-Item thresh-*.txt -ErrorAction SilentlyContinue
foreach ($n in 10, 20, 40, 60, 80) {
  $p = Start-Process -FilePath .\venv\Scripts\python.exe -ArgumentList "burst2.py $n 6 env" -RedirectStandardOutput "thresh-$n.out.txt" -RedirectStandardError "thresh-$n.err.txt" -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(30000)) { "TIMEOUT" | Out-File -Append thresh-$n.out.txt; Get-Process ui4me-host,python -ErrorAction SilentlyContinue | Stop-Process -Force }
  Start-Sleep 2
}
