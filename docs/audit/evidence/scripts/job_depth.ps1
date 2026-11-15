Set-Location 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd'
Remove-Item depth-*.txt -ErrorAction SilentlyContinue
foreach ($n in 20, 30, 40, 60, 120, 250) {
  $p = Start-Process -FilePath .\venv\Scripts\python.exe -ArgumentList "depth.py $n" -RedirectStandardOutput "depth-$n.out.txt" -RedirectStandardError "depth-$n.err.txt" -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(30000)) { "TIMEOUT" | Out-File -Append "depth-$n.out.txt"; Get-Process ui4me-host,python -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq (Get-Process -Id $PID).SessionId } | Stop-Process -Force }
  Start-Sleep 2
}
Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Application Error'; StartTime=(Get-Date).AddMinutes(-10)} -ErrorAction SilentlyContinue | Select-Object -First 6 | ForEach-Object { $_.Message.Split("`n")[0..4] -join ' | ' } | Out-File depth-events.txt
