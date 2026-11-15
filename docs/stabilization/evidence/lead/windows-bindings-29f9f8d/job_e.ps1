$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
$out = "$ws\results\e-bind"; New-Item -ItemType Directory -Force $out | Out-Null
$py = "$ws\py\cpython-3.13-windows-x86_64-none\python.exe"
$env:UI4ME_HOST_BINARY = "$ws\target\release\ui4me-host.exe"; $env:PYTHONPATH = "$ws\src\python"
Set-Location "$ws\src"
& $py -m unittest tools.test_binding_robustness tools.test_application tools.test_reference_client tools.test_bootstrap tools.test_launcher *> "$out\py313.txt"; "py313 exit=$LASTEXITCODE" | Out-File -Append "$out\summary.txt"
$py310 = Get-ChildItem "$ws\py" -Directory -Filter 'cpython-3.10*' | Select-Object -First 1
if ($py310) { & "$($py310.FullName)\python.exe" -m unittest tools.test_binding_robustness tools.test_application tools.test_reference_client tools.test_bootstrap tools.test_launcher *> "$out\py310.txt"; "py310 exit=$LASTEXITCODE" | Out-File -Append "$out\summary.txt" }
& $py -c "from ui4me_core import launcher; from pathlib import Path; import glob; print('BIND04 host', launcher._is_native_host(Path(r'$ws\target\release\ui4me-host.exe'))); [print('BIND04 shim', p, launcher._is_native_host(Path(p))) for p in glob.glob(r'F:\agent-workspaces\ui4me-core\audit-ac27d3dd\venv\Scripts\*.exe')]" *>> "$out\summary.txt"
$mod = "$ws\src\powershell\UI4ME.Core\UI4ME.Core.psd1"
$ps51 = "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe"; $pw = "$ws\pwsh7\pwsh.exe"
foreach ($c in @(@('ps51',$ps51,0),@('pwsh7',$pw,0),@('ps51-slow',$ps51,3),@('pwsh7-slow',$pw,3))) {
  & $c[1] -NoProfile -ExecutionPolicy Bypass -File "$ws\probes\burst3.ps1" -N 100 -Rounds 6 -Module $mod -SleepMs $c[2] *> "$out\burst-$($c[0]).txt"
  "burst $($c[0]) exit=$LASTEXITCODE" | Out-File -Append "$out\summary.txt"
}
'DONE' | Out-File -Append "$out\summary.txt"
