#!/bin/bash
# Runs inside mcr.microsoft.com/powershell as root. /src/<variant>/UI4ME.Core is a staged module
# (host without execute permission, as NuGet extraction leaves it).
set -u
variant=$1
dest=/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1
mkdir -p "$dest" && cp -r /src/$variant/UI4ME.Core/. "$dest/" && chown -R root:root /usr/local/share/powershell/Modules
host="$dest/runtimes/linux-x64/ui4me_core/_bin/ui4me-host"
chmod 0644 "$host"; ls -l "$host" | awk '{print "installed:", $1, $3}'
useradd -m appuser 2>/dev/null
cat > /tmp/app.ps1 <<'PS'
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core
$app = Start-UI4MEScript -Name 'AllUsers' -Instance multiple
$window = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $window -Html "<p id='t'>ran as $([Environment]::UserName)</p>"
[Console]::Out.WriteLine("TEXT $(Get-UI4MEDocumentText -Document $doc -Element t)")
Set-UI4MEReady -Application $app
$null = Request-UI4MEQuit -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
PS
chmod 0644 /tmp/app.ps1
echo "--- as appuser: Get-UI4MEHostPath"
su appuser -c "pwsh -NoProfile -c 'Import-Module UI4ME.Core; \$p = Get-UI4MEHostPath; \"path: \$p\"; & \$p --version 2>/dev/null'" 2>&1 | tail -4
echo "--- as appuser: headless Start-UI4MEScript"
su appuser -c "cd /tmp && mkdir -p /tmp/rt-appuser && chmod 700 /tmp/rt-appuser && XDG_RUNTIME_DIR=/tmp/rt-appuser UI4ME_TEST_MODE=1 UI4ME_ALLOW_UNSUPERVISED=1 pwsh -NoProfile -File /tmp/app.ps1; echo exit=\$?" 2>&1 | grep -v "GLIBC_2.39" | tail -4
ls -l "$host" | awk '{print "installed afterwards:", $1, $3}'
echo "--- root runs it once, then appuser"
pwsh -NoProfile -c 'Import-Module UI4ME.Core; $p = Get-UI4MEHostPath; "root path: $p"' 2>&1 | tail -1
ls -l "$host" | awk '{print "installed after root:", $1, $3}'
su appuser -c "XDG_CACHE_HOME=/tmp/empty-cache pwsh -NoProfile -c 'Import-Module UI4ME.Core; \"path: \$(Get-UI4MEHostPath)\"'" 2>&1 | tail -2
