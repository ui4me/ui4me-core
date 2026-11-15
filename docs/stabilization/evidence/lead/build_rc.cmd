@echo off
set "WS=F:\agent-workspaces\ui4me-core\stab-374b06f6"
cd /d %WS%\src
git pull -q ..\integ.bundle stabilization/integration || exit /b 1
git log --oneline -1
git status --short
set "PY=%WS%\py\cpython-3.13-windows-x86_64-none"
if not exist %WS%\rcbuild\Scripts\python.exe "%PY%\python.exe" -m venv %WS%\rcbuild
set "PATH=%WS%\rcbuild\Scripts;%PY%;%PATH%"
set "PYTHON3=%WS%\rcbuild\Scripts\python.exe"
set CARGO_BUILD_JOBS=4
set "CARGO_TARGET_DIR="
if exist %WS%\rc\dist rmdir /s /q %WS%\rc\dist
powershell -NoProfile -ExecutionPolicy Bypass -File tools\release\build_windows.ps1 -Python %WS%\rcbuild\Scripts\python.exe -OutDir %WS%\rc\dist > %WS%\rc-build.log 2>&1
echo build exit=%ERRORLEVEL%
dir /b %WS%\rc\dist
