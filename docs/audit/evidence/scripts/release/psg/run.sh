#!/bin/sh
set -u
pwsh -NoProfile -c '$PSVersionTable.PSVersion.ToString(); Set-PSRepository PSGallery -InstallationPolicy Trusted; Install-Module UI4ME.Core -RequiredVersion 1.0.1 -Scope AllUsers -Force; (Get-Module -ListAvailable UI4ME.Core).ModuleBase' 2>&1 | tail -3
H=$(find / -path '*UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host' 2>/dev/null | head -1)
echo "host file: $H"; ls -l "$H"
useradd -m auditor
echo "=== non-root user, AllUsers install"
su auditor -s /bin/sh -c "pwsh -NoProfile -c 'Import-Module UI4ME.Core; try { \$p = Get-UI4MEHostPath; & \$p --version } catch { \"ERROR: \" + \$_.Exception.Message }'" 2>&1 | tail -4
echo "=== root user, AllUsers install"
pwsh -NoProfile -c 'Import-Module UI4ME.Core; & (Get-UI4MEHostPath) --version' 2>&1 | tail -2
ls -l "$H"
echo "=== non-root after root chmod"
su auditor -s /bin/sh -c "pwsh -NoProfile -c 'Import-Module UI4ME.Core; try { & (Get-UI4MEHostPath) --version } catch { \"ERROR: \" + \$_.Exception.Message }'" 2>&1 | tail -3
echo "=== CurrentUser install + headless direct start as non-root"
su auditor -s /bin/sh -c "pwsh -NoProfile -c 'Set-PSRepository PSGallery -InstallationPolicy Trusted; Install-Module UI4ME.Core -RequiredVersion 1.0.1 -Scope CurrentUser -Force'" 2>&1 | tail -2
su auditor -s /bin/sh -c "cd /tmp && cp /psg/app.ps1 /tmp/app.ps1 && UI4ME_TEST_MODE=1 timeout 90 pwsh -NoProfile -File /tmp/app.ps1; echo exit=\$?" 2>&1 | tail -5
