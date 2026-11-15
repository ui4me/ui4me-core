#!/bin/sh
set -u
useradd -m auditor
su auditor -s /bin/sh -c "pwsh -NoProfile -c 'Set-PSRepository PSGallery -InstallationPolicy Trusted; Install-Module UI4ME.Core -RequiredVersion 1.0.1 -Scope CurrentUser -Force; (Get-Module -ListAvailable UI4ME.Core).ModuleBase'" 2>&1 | tail -2
su auditor -s /bin/sh -c "cd /tmp && cp /psg/app.ps1 /tmp/app.ps1 && UI4ME_TEST_MODE=1 timeout 90 pwsh -NoProfile -File /tmp/app.ps1; echo exit=\$?" 2>&1 | tail -5
