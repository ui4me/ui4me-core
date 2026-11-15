#!/bin/sh
# Clean-machine check of a PowerShell AllUsers install (RELEASE-06 / F-19, T12).
#
# Run as root inside a disposable Linux container with pwsh 7.4+ (for example
# mcr.microsoft.com/powershell). MODULE_DIR is an unpacked UI4ME.Core module
# folder that contains runtimes/linux-x64/ui4me_core/_bin/ui4me-host (the staged
# release module, or the source module plus a built host).
#
# It packs the folder into a .nupkg in a container-local file repository
# (Publish-PSResource to that folder; PSResourceGet 1.0 has no
# Compress-PSResource, and nothing leaves the container), installs it with
# Install-PSResource -Scope AllUsers as root (nupkg extraction drops Unix
# modes, exactly as a Gallery install does), then runs a headless
# Start-UI4MEScript app as a separate non-root user. Exit 0 only if that user
# can start the bundled host. Never publishes; touches nothing outside the
# container.
set -eu
if [ "$(id -u)" != 0 ]; then echo 'smoke_psmodule_allusers.sh must run as root in a container' >&2; exit 2; fi
MODULE=${1:?usage: smoke_psmodule_allusers.sh MODULE_DIR}
[ -f "$MODULE/UI4ME.Core.psd1" ] || { echo "No UI4ME.Core.psd1 in $MODULE" >&2; exit 2; }
WORK=$(mktemp -d /tmp/ui4me-allusers.XXXXXX)
export WORK
cp -R "$MODULE" "$WORK/UI4ME.Core"
pwsh -NoProfile -NonInteractive -Command '
  $ErrorActionPreference = "Stop"
  $PSVersionTable.PSVersion.ToString()
  $repo = Join-Path $env:WORK repo
  New-Item -ItemType Directory -Force $repo | Out-Null
  Register-PSResourceRepository -Name ui4me-allusers-smoke -Uri $repo -Trusted
  # Local folder repository inside this container only.
  Publish-PSResource -Path (Join-Path $env:WORK UI4ME.Core) -Repository ui4me-allusers-smoke -SkipDependenciesCheck
  Get-ChildItem $repo -Filter *.nupkg | ForEach-Object { "Packed: " + $_.Name }
  Install-PSResource -Name UI4ME.Core -Repository ui4me-allusers-smoke -Scope AllUsers -TrustRepository -Quiet
  $installed = Get-Module -ListAvailable UI4ME.Core | Select-Object -First 1
  "Installed (AllUsers): " + $installed.ModuleBase
'
HOST_FILE=$(pwsh -NoProfile -NonInteractive -Command '(Get-Module -ListAvailable UI4ME.Core | Select-Object -First 1).ModuleBase' | tail -n 1)/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
ls -l "$HOST_FILE"
id ui4me-allusers >/dev/null 2>&1 || useradd -m ui4me-allusers
APP_DIR=$(mktemp -d /tmp/ui4me-allusers-app.XXXXXX)
cat > "$APP_DIR/app.ps1" <<'PS1'
$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core
$app = Start-UI4MEScript -Name 'AllUsers clean check' -Instance multiple
$window = New-UI4MEWindow -Application $app -Title 'allusers' -Width 300 -Height 200
Set-UI4MEReady -Application $app
[Console]::Out.WriteLine('PS-APP-CONNECTED')
$null = Request-UI4MEQuit -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
PS1
chmod 755 "$APP_DIR"
chmod 644 "$APP_DIR/app.ps1"
set +e
OUTPUT=$(su ui4me-allusers -s /bin/sh -c "cd /tmp && env -u UI4ME_HOST_BINARY UI4ME_TEST_MODE=1 timeout 120 pwsh -NoProfile -File '$APP_DIR/app.ps1'" 2>&1)
STATUS=$?
set -e
printf '%s\n' "$OUTPUT" | tail -n 20
ls -l "$HOST_FILE"
if [ "$STATUS" -ne 0 ] || ! printf '%s\n' "$OUTPUT" | grep -q 'PS-APP-CONNECTED'; then
  echo "FAIL: a non-root user cannot use the AllUsers install (exit $STATUS)" >&2
  exit 1
fi
echo 'PASS: AllUsers install as root, headless direct start as a non-root user'
