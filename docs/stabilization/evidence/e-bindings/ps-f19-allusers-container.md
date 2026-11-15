# F-19 (BIND-PS-11, RELEASE-06): AllUsers install as root, run as an unprivileged user

Container: mcr.microsoft.com/powershell:latest (Ubuntu 22.04, PowerShell 7.4.2), names ui4me-e-f19-*, removed (--rm).
The staged module (host 0644, as NuGet extraction leaves it) is copied to /usr/local/share/powershell/Modules as root.
Script: scratch f19/run.sh (in this file's history: see the commit message). Host: target/debug/ui4me-host of this branch.

```text
===== baseline
installed: -rw-r--r-- root
--- as appuser: Get-UI4MEHostPath
chmod: changing permissions of '/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host': Operation not permitted
Exception: The native host is not executable and could not be made executable:
/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_co
re/_bin/ui4me-host
--- as appuser: headless Start-UI4MEScript
Exception: The native host is not executable and could not be made executable:
/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_co
re/_bin/ui4me-host
exit=1
installed afterwards: -rw-r--r-- root
--- root runs it once, then appuser
root path: /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
installed after root: -rwxr--r-- root
/usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_co
re/_bin/ui4me-host
===== fixed
installed: -rw-r--r-- root
--- as appuser: Get-UI4MEHostPath
path: /home/appuser/.cache/ui4me/powershell/1.0.1/91c0ef3ed8b6319c/ui4me-host
ui4me-host 1.0.1
--- as appuser: headless Start-UI4MEScript
TEXT ran as appuser
exit=0
installed afterwards: -rw-r--r-- root
--- root runs it once, then appuser
root path: /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
installed after root: -rwxr-xr-x root
path: /usr/local/share/powershell/Modules/UI4ME.Core/1.0.1/runtimes/linux-x64/ui4me_core/_bin/ui4me-host
```
