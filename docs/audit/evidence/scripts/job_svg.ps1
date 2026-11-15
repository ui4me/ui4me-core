Set-Location 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd'
Remove-Item svg-*.txt -ErrorAction SilentlyContinue
foreach ($n in 500, 700) {
  $p = Start-Process -FilePath .\venv\Scripts\python.exe -ArgumentList "svgdepth.py $n" -RedirectStandardOutput "svg-$n.out.txt" -RedirectStandardError "svg-$n.err.txt" -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(30000)) { "TIMEOUT" | Out-File -Append "svg-$n.out.txt"; Get-Process ui4me-host,python -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq (Get-Process -Id $PID).SessionId } | Stop-Process -Force }
  Start-Sleep 2
}
