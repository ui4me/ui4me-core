$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
$out = "$ws\results\ctrlbreak-e"; New-Item -ItemType Directory -Force $out | Out-Null
& "$ws\py\cpython-3.13-windows-x86_64-none\python.exe" "$ws\probes\ctrlbreak_win.py" $out *> "$out\summary.txt"
'DONE' | Out-File -Append "$out\summary.txt"
