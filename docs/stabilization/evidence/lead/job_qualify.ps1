$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
Set-Location "$ws\src"
Remove-Item Env:UI4ME_HOST_BINARY -ErrorAction SilentlyContinue; Remove-Item Env:PYTHONPATH -ErrorAction SilentlyContinue
& "$ws\py\cpython-3.13-windows-x86_64-none\python.exe" tools\release\qualify_windows_rc.py --wheel "$ws\rc\dist\ui4me_core-1.0.2-py3-none-win_amd64.whl" --output "$ws\rc\qualification-windows-1.0.2.json" *> "$ws\rc\qualify.log"
"exit=$LASTEXITCODE" | Out-File -Append "$ws\rc\qualify.log"
