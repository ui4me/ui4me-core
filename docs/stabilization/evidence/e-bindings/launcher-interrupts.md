# Launcher interrupt handling (lead request; BIND-10 / F-25, BIND-PS-07)

Linux, UI4ME_TEST_MODE=1 (headless). 'launcher' = SIGINT to the launcher only; 'group' = SIGINT to the launcher's process group (a terminal Ctrl+C); 'term' = SIGTERM to the launcher.
Hosts: 1.0.1-era host built from this branch (target/debug) and workstream G's host (g-lifecycle target/debug, orderly SIGINT/SIGTERM handling). Probe app prints SHUTDOWN-HOOK from on_shutdown and RUN-RETURNED after run().

Python launcher at 9827c3d (kills the host on interrupt), G host:
```text
mode=launcher exit=130 after=0.17s stdout=[] defect_msg=False
mode=group exit=130 after=0.17s stdout=[] defect_msg=False
mode=term exit=143 after=0.17s stdout=[] defect_msg=False
```
Python launcher on this commit:
```text
host: ui4me-host
mode=launcher exit=130 after=0.03s stdout=[] defect_msg=False
mode=group exit=130 after=0.02s stdout=[] defect_msg=False
mode=term exit=143 after=0.02s stdout=[] defect_msg=False
host: ui4me-host-g
mode=launcher exit=130 after=0.15s stdout=['SHUTDOWN-HOOK', 'RUN-RETURNED'] defect_msg=False
mode=group exit=130 after=0.14s stdout=['SHUTDOWN-HOOK', 'RUN-RETURNED'] defect_msg=False
mode=term exit=143 after=0.15s stdout=['SHUTDOWN-HOOK', 'RUN-RETURNED'] defect_msg=False
```
PowerShell launcher on this commit (probe prints UNTIL-SHUTDOWN-RETURNED after Wait-UI4ME -UntilShutdown); before the 'exit' fix, 'group' with the old host exited 0 (pipeline stop swallowed 'exit 130'):
```text
host: ui4me-host
mode=launcher exit=130 after=0.19s stdout=[]
mode=group exit=130 after=0.03s stdout=[]
host: ui4me-host-g
mode=launcher exit=130 after=0.22s stdout=['UNTIL-SHUTDOWN-RETURNED']
mode=group exit=130 after=0.28s stdout=['UNTIL-SHUTDOWN-RETURNED']
```
Run: python3 scripts/interrupt_drive.py <host> launcher|group|term ; python3 scripts/interrupt_drive_ps.py <host> launcher|group <abs path to UI4ME.Core.psd1>
