$ErrorActionPreference = 'Continue'
$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
$out = "$ws\results\pester"; New-Item -ItemType Directory -Force $out | Out-Null
$hostexe = "$ws\target\release\ui4me-host.exe"
$runner = "$ws\src\tools\release\run_pester.ps1"
$pwsh = "$ws\pwsh7\pwsh.exe"
if ($pwsh) {
  & $pwsh -NoProfile -ExecutionPolicy Bypass -File $runner -HostBinary $hostexe *> "$out\pwsh7.txt"; "pwsh7 exit=$LASTEXITCODE" | Out-File -Append "$out\summary.txt"
} else { 'pwsh7 MISSING' | Out-File -Append "$out\summary.txt" }
& "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File $runner -HostBinary $hostexe *> "$out\ps51.txt"; "ps51 exit=$LASTEXITCODE" | Out-File -Append "$out\summary.txt"
'DONE' | Out-File -Append "$out\summary.txt"
