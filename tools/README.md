# Development and qualification tools

These tools belong to repository development. The installed host has no dependency
on SSH, RDP, Proxmox, Python test tooling, Node or a web server.

## Build and run

Linux prerequisites can be installed with:

```sh
bash tools/bootstrap_linux.sh --install-system
python3 tools/run_example.py
```

On Windows, run `tools/bootstrap_windows.ps1` as the dedicated development user.
Use `-InstallBuildTools` in the administrative provisioning session only if the C++
toolchain is missing. Python 3.11 or newer is required for examples and checks.
Launch the example from the user's interactive desktop:

```powershell
python tools/run_example.py
```

`run_example.py` generates a temporary manifest containing the exact Python
interpreter path, avoiding the difference between `python` and `python3`.
The reference app uses the host's fixed probe surface; it is not an HTML renderer.

Install the executable with:

```sh
python3 tools/install.py
```

To install the host and copy a self-contained reference application outside the
checkout, give a new or empty destination directory:

```sh
python3 tools/install.py --example-dir /permanent/path/ui4me-reference
ui4me-host run --app /permanent/path/ui4me-reference/app.toml
```

The copied application contains the Python client, demo, manifest and PNG icon. Its
manifest uses the current interpreter's absolute path, so it works with Windows
`python.exe` and Linux interpreter installations without editing a command name.

## Register an application in the desktop menu

Keep the application manifest and its backend in their permanent location, then
register a launcher for the current user:

```sh
python3 tools/create_launcher.py install --app /absolute/path/to/app.toml
python3 tools/create_launcher.py uninstall --app-id org.example.MyApplication
```

Use `--host /absolute/path/ui4me-host` if the installed binary is not on PATH.
The manifest's `app.id` becomes the Linux desktop-file ID and the Windows shortcut
AppUserModelID, matching the identity assigned to the native windows. Linux gets
a per-user `.desktop` entry; Windows gets a per-user Start Menu shortcut whose
AppUserModelID is written and read back through the Windows property store.
The command records ownership and refuses to overwrite an unrelated or modified
launcher. Repeating installation updates its own unchanged entry; uninstall
removes that entry and its ownership record, leaving application files intact.

`--icon` can select a launcher icon. Windows shortcuts require an `.ico` file or
an icon resource in an `.exe`; a PNG manifest icon is still available to the
native window but is not a Windows shortcut icon. A Linux executable path
containing `%` is rejected because desktop launcher implementations interpret
that character inconsistently; manifest and other argument paths can contain it.
Registration and launch must be tested under the actual desktop user.

## Focused verification

```sh
python3 tools/verify.py --stage python
python3 tools/verify.py --stage check --stage test
python3 tools/verify.py --stage headless
python3 tools/verify.py --stage faults
```

With no stages, the verifier runs formatting, check, Rust tests, Python tests and
the headless end-to-end reference application and bounded fault fixtures. The
Windows run additionally checks forced host death through its real Job Object;
Linux production forced-host cleanup is verified in the native systemd session.
Each command and its exit status
is saved under `verification/latest/`. `--jobs 2` is the conservative default.
Full verification belongs at integration milestones, not after every small edit.

Headless mode is exclusively a protocol/lifecycle fixture. It does not establish
Wayland, Windows, IME, dialog, accessibility or screen-reader correctness.

On a non-Windows development host, the supplied source-check helper can check
the actual Windows Rust sources without a Windows SDK:

```sh
rustup target add --toolchain 1.99.1 x86_64-pc-windows-msvc
python3 tools/windows_source_check.py
```

It creates a temporary manifest using this repository's sources, dependency
features and lockfile, and omits the Windows resource build script. This verifies
Rust source/type/lint compatibility only. Use the real workspace on Windows to
verify resources, linking and native execution.

To inspect the actual CPU probe without a desktop, generate an image:

```sh
cargo run --locked --example render_probe -- probe.png
```

The image does not provide evidence for native input or operating-system behavior.

## Supplied fault fixtures

The fixtures are deliberately misbehaving test backends. They use the same
transport and launch path as the reference app; there is no mock host.

```sh
python3 tools/run_faults.py veto-close --headless
python3 tools/run_faults.py veto-quit --headless
python3 tools/run_faults.py freeze-close --headless --duration 3
python3 tools/run_faults.py freeze-quit --headless --duration 3
python3 tools/run_faults.py crash-after-ready --headless --duration 1
python3 tools/run_faults.py log-flood --headless
python3 tools/run_faults.py log-flood --headless --unread-host-output
python3 tools/run_faults.py managed-tree --headless
```

Run the same fixtures without `--headless` on the actual target desktop to observe
native behavior. A blocked decision is not approval: the host must keep the
affected windows alive and remain locally responsive. `freeze-close` and
`freeze-quit` release their callback with an explicit veto after `--duration`
seconds, then complete normally. To leave the decision pending while inspecting
painting, native text input and accessibility, use:

```sh
python3 tools/run_faults.py freeze-quit --duration 0
```

End that scenario with the host's explicit native **Ctrl+Shift+Q** emergency quit.
The runner leaves this scenario BLOCKED for native observation; record the actual
result with the acceptance tool. After a native `crash-after-ready` fixture,
inspect the visible failure surface and its working Quit action before ending it.

For process containment, use the production native host with its normal strong
containment, then deliberately kill only the host launched by this runner:

```sh
python3 tools/run_faults.py managed-tree --kill-host --timeout 30
```

The fixture supplies a child and grandchild, each with PID and OS creation stamp.
It obtains the actual host PID from authenticated `host.diagnostics` and records
its OS creation stamp and containment mode. The driver kills that host process,
not the Linux systemd bootstrap launcher, and checks both descendants after host
exit. Each run has a unique application ID. A survivor fails the check; the runner
then attempts to clean up only the matching fixture processes. Headless fallback
or `--allow-unsupervised` is not evidence of strong Linux containment. Do not use
the weakened mode for the production forced-host-death acceptance criterion.

Each invocation writes its own directory under `verification/faults/`, including
the exact command, backend events, bounded flood output, process identities and
report. `--output` must name an empty directory. A fixture PASS describes only the
listed control/process checks; native painting, IME and failure UI still require
desktop evidence. The log-flood fixture emits approximately 4 MiB on each of
stdout and stderr while requesting host services, then exits normally.
`--unread-host-output` deliberately fills the host's downstream output pipe and
does not drain it until the host exits. The same control calls and orderly exit
must still complete; a logging sink must not block the GUI or shutdown path.

## Native acceptance evidence

In the actual KDE native Wayland or Windows RDP session:

```sh
python3 tools/native_acceptance.py --automated --output verification/native
```

The automated subset checks window creation, host communication, clipboard
self-round-trip and quit. It deliberately leaves the 15 real desktop checks
BLOCKED until observations exist. It never converts compilation or the automated
subset into full qualification.

Record a completed native observation by referencing an existing evidence file:

```sh
python3 tools/native_acceptance.py --record N05 --status PASS --evidence verification/dialogs-observation.json --note "All three dialog kinds, cancellation, parent close and quit races passed"
```

A referenced file must contain the real commands, actions, observations and
outcome. A filename alone is not proof. A record made in the wrong desktop session
is invalid. Review `acceptance.json` and the evidence before declaring release
readiness. Every mandatory check needs evidence on both platforms. Use separate
output directories for KDE and Windows; the report binds observations to the
machine and OS and records evidence file hashes.

For IME, use an actual configured input method. Injecting a completed Unicode
string through RDP does not exercise preedit, caret or surrounding-text behavior.
For clipboard, test with another Windows/Linux application in the target desktop;
RDP clipboard redirection is a separate path. Record redirection settings.

For responsiveness, observe host-local text/caret/diagnostics or input processing
while the backend is blocked. Merely moving a Wayland window is not sufficient.

The reference app opens file, save and folder dialogs with F3/F4/F5. F7 requests
protocol cancellation of outstanding dialogs; use the second app window if the
dialog's modal parent cannot receive keyboard input. Also test closing the parent
and quitting the application while each dialog is open. Verify the native dialog
actually closes and the original request completes or is cancelled without a late
result reaching a reused window.

For external application survival, launch the demo with an explicit harmless test
file or URL and press F8 to invoke the host's desktop broker:

```sh
python3 tools/run_example.py --external-target /absolute/path/to/test-document.txt
```

Record which native application opened it, then exit or deliberately terminate
the host and verify that external application remains alive. Do this in the
dedicated test desktop so an existing user's unsaved application is not disturbed.
The command does not open anything until the F8 action is invoked.

## Windows laboratory

Administrative access is `administrator@<windows-vm>`. The Windows Server 2022 VM has
Desktop Experience. The dedicated test user uses a workspace on `F:` and an RDP
desktop session. Headless builds and tests can run over SSH; native GUI checks
must execute in the visible interactive session of the dedicated user.

`windows/provision_user.ps1` reads credentials and a public key as one JSON object
on stdin. It creates a normal user, grants Remote Desktop access and workspace
access, and installs the supplied SSH public key. It never prints the password.
Keep that JSON record on the Linux workstation, outside this repository. Reuse
an existing verified record; do not reset an existing user's password by default.

`windows/start_interactive.ps1` can be triggered through administrative SSH to
run an existing PowerShell script using the dedicated user's interactive token.
The user must already be logged in through RDP. For example:

```powershell
powershell.exe -NoProfile -File tools/windows/start_interactive.ps1 -UserName Ui4meDev -ScriptPath F:\ui4me\run-native.ps1
```

The job script should run native checks, save output and return a nonzero exit code
on failure. This helper is not an RDP client and does not manufacture a desktop
session. The GUI process, automation process and screenshot process must share
the intended session ID.

`windows/capture_desktop.ps1` saves a desktop screenshot, target-process UI
Automation tree and session information. It can invoke one named accessible
control using `-InvokeName`. Run it inside the target RDP session with `-STA`.
This is an observation aid; review results rather than treating a tree dump as
complete accessibility proof.

## Proxmox recovery

The user has authorized management of the `<windows-vm>` VM through `root@<proxmox-host>`.
The helper discovers and verifies its VM ID from the Proxmox inventory. It does
not guess IDs or restart unrelated VMs or the hypervisor.

A private local configuration file can contain:

```json
{
  "admin_ssh": "administrator@<windows-vm>",
  "proxmox_ssh": "root@<proxmox-host>",
  "vm_name": "<windows-vm>"
}
```

```sh
python3 tools/windows_lab.py --config /absolute/private/windows-lab.json status
python3 tools/windows_lab.py --config /absolute/private/windows-lab.json recover
```

`recover` starts a stopped guest. Restarting a running but unreachable guest
requires `--restart-running`; a forced stop after unsuccessful orderly shutdown
also requires `--force`. The local finishing agent may use these options under
the user's existing recovery authorization after diagnosing the actual failure.
An SSH interruption alone is not proof that Windows is hung: inspect the desktop
and Proxmox state first.

After recovery, reconnect RDP, verify the correct session and rerun interrupted
checks. Preserve completed output on the Linux workstation between milestones.
Do not mark interrupted work PASS or repeatedly overload the VM with parallel
full builds.

## Preparation-time checks

The supplied Python suite was run on the preparation Linux environment on
2026-11-12: **17 tests passed; one real Windows named-pipe test was skipped**
because it requires Windows. The process-identity test uses real child/grandchild
processes and verifies that cleanup rejects a mismatched creation stamp.

Linux launcher generation passed `desktop-file-validate` and an actual GIO launch
of an isolated command fixture, including quoted paths, spaces, dollar signs,
literal percent characters in arguments and non-BMP Unicode. Owned-entry update,
uninstall and refusal to replace an unrelated launcher also passed. This checks
launcher mechanics; it does not establish KDE taskbar grouping.

All five PowerShell scripts passed the PowerShell parser, and the shortcut
property-store helper's C# compiled. Those are syntax/compilation checks, not
Windows execution evidence. `bootstrap_linux.sh` passed `bash -n`; Python source
files passed bytecode compilation.

The preparation environment rejects creation of pathname Unix sockets with
`EPERM` and has no interactive Wayland or Windows desktop. Native acceptance
therefore remains BLOCKED. The supplied socket-pair protocol tests can run in
that environment, but real host/listener integration and desktop qualification
must be performed by the local finishing agent on the target systems.
