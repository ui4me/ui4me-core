# Development and qualification tools

These tools belong to repository development. The installed host has no dependency
on SSH, RDP, Proxmox, Python test tooling, Node or a web server.

## Build and run

Linux prerequisites can be installed with:

```sh
bash tools/bootstrap_linux.sh --install-system
python3 tools/run_renderer.py
```

On Windows, run `tools/bootstrap_windows.ps1` as the dedicated development user.
Use `-InstallBuildTools` in the administrative provisioning session only if the C++
toolchain is missing. Python 3.11 or newer is required for examples and checks.
Launch the example from the user's interactive desktop:

```powershell
python tools/run_renderer.py
```

`run_renderer.py` generates a temporary manifest containing the exact Python
interpreter path, avoiding the difference between `python` and `python3`. It runs
the real local HTML/CSS/Python application with fonts, PNG/SVG, form controls,
live DOM updates, native picker, HTML dialog/popover and a second document.

`run_example.py` retains the v0.1 fixed host probe for regression testing. Its
hotkeys/fault helpers remain useful; the product example is now the renderer app.
The source Python facade is `examples/python/application.py`. Its full protocol
contract is in [PROTOCOL_DOCUMENTS.md](../docs/PROTOCOL_DOCUMENTS.md).

Install the executable with:

```sh
python3 tools/install.py
```

To install the host and copy a self-contained reference application outside the
checkout, give a new or empty destination directory:

```sh
python3 tools/install.py --example-dir /permanent/path/ui4me-renderer-reference
ui4me-host run --app /permanent/path/ui4me-renderer-reference/app.toml
```

The copied application contains the Python client/facade/backend, complete local
HTML/CSS/assets/font license, manifest and PNG icon. Its
manifest uses the current interpreter's absolute path, so it works with Windows
`python.exe` and Linux interpreter installations without editing a command name.
Keep that interpreter installed at the selected location. Use `--probe` with a
different destination to install the retained v0.1 probe instead. Existing
nonempty application directories are never overwritten by the copy helper.

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
a per-user `.desktop` entry; Windows gets a per-user Start Menu shortcut named
after `app.name` (for example `UI4ME Desktop Workspace.lnk`, because the Start Menu
shows and searches shortcut file names) whose AppUserModelID is written and read
back through the Windows property store. The ownership record remembers the
shortcut path, so `uninstall --app-id` and a renamed application are handled.
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
python3 tools/verify.py --stage renderer
python3 tools/verify.py --stage faults
```

With no stages, the verifier runs formatting, check, Rust tests, Python tests and
the headless probe and real document application plus bounded fault fixtures. Run
`cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings` at the same
integration milestone. The
Windows run additionally checks forced host death through its real Job Object;
Linux production forced-host cleanup is verified in the native systemd session.
Each command and its exit status
is saved under `verification/latest/`. `--jobs 2` is the conservative default.
Full verification belongs at integration milestones, not after every small edit.

Use `python3 tools/verify.py --stage fmt` for formatting. It names the five owned
workspace packages explicitly. `cargo fmt --all` also walks local dependency
patch workspaces and can ask for their omitted upstream developer generators;
those vendored trees are not the project's formatting target.

The renderer headless fixture exercises the real document engine and its IPC
contract; the probe headless fixture exercises host lifecycle. Neither establishes
Wayland, Windows, IME, dialog, native accessibility or screen-reader correctness.
`--stage renderer-native` creates the new native renderer ledger and runs its
smoke subset; `--stage browser` runs the optional controlled browser reference.
Unobserved native scenarios and unavailable oracle prerequisites remain BLOCKED.

On a non-Windows development host, the supplied source-check helper can check
the actual Windows Rust sources without a Windows SDK:

```sh
rustup target add --toolchain 1.99.1 x86_64-pc-windows-msvc
python3 tools/windows_source_check.py
```

It mirrors the entire supplied workspace, dependency features, lockfile and local
patches into `.local/windows-source-workspace`, and disables only the host's
Windows resource build script. The stable source path allows Cargo to reuse
dependency fingerprints across retries. A process lock prevents simultaneous
mirror updates; generated directory ownership is checked before replacing it.
Build outputs, Git metadata, lab credentials and verification records are excluded.
This verifies
Rust source/type/lint compatibility only. Use the real workspace on Windows to
verify resources, linking and native execution.

To inspect the actual CPU probe without a desktop, generate an image:

```sh
cargo run --locked --example render_probe -- probe.png
```

The image does not provide evidence for native input or operating-system behavior.

Render the actual HTML/CSS engine with the standalone CPU diagnostic command:

```sh
target/debug/ui4me-host render --html examples/python/ui/index.html \
  --output verification/workspace.png --width 1080 --height 760 --scale 1
```

On Windows use `target\debug\ui4me-host.exe`; an installed `ui4me-host` works too.
Width and height are CSS pixels; physical output dimensions include `--scale`.
Local resources settle within the command's bounded deadline. The command reports
diagnostics and output dimensions as JSON. It is not a printing/PDF application API.

## Controlled browser reference

Install optional QA dependencies in a separate Python environment and use a local
Chromium executable. None is a runtime or packaging dependency:

```sh
python3 -m pip install -r tools/requirements-browser.txt
python3 -m playwright install chromium
python3 tools/browser_reference.py --output verification/browser-first-run
```

`--chromium /path/to/chromium` reuses an existing compatible installation. The
four fixtures cover explicit box geometry, flex/grid, a pinned local font and
basic static SVG. Every run records versions, source/binary/fixture hashes,
predeclared tolerances, metrics and renderer/browser/difference PNGs. Use a new
or empty output directory. Missing optional prerequisites yield BLOCKED; an
executed comparison mismatch yields FAIL. Review the images, preserve fixed
tolerances and investigate differences. Passing four fixtures is not a claim of
full-browser compatibility. See R12 in the renderer acceptance guide.

## Supplied fault fixtures

The fixtures are deliberately misbehaving test backends. They use the same
transport and launch path as the reference app; there is no mock host.

The renderer additionally has direct native seam diagnostics:

```sh
python3 tools/run_renderer.py --updates 0.25
python3 tools/run_renderer.py --hold-callback
python3 tools/run_renderer.py --crash-after-ready 3
```

The update run continues document mutations while a window is covered/minimized.
The blocked-callback run waits after Add row is clicked; observe host-local
editing/painting/accessibility and finish with native Ctrl+Shift+Q. Repeat it with
ordinary native window close; the independent lifecycle request pool and shutdown
path must finish even while the application notification worker is blocked. The crash run
lets the host present its backend-failure UI. These flags do not synthesize a
PASS; record actual desktop observations in R09.

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

## Resource measurements

```sh
python3 tools/measure_resources.py --renderer --host ~/.cargo/bin/ui4me-host \
  --cycles 400 --dom-cycles 800 --output verification/renderer-measurements.json
```

Run it in the target desktop session. It launches the host with itself as the
backend and reports startup-to-ready and first frame separately, idle CPU
(Linux: per-thread schedstat runtime; Windows: process times), host+backend
memory (Linux PSS/RSS/shared/private; Windows private bytes and working set)
and memory across repeated window create/close cycles. `--renderer` loads real
documents with local resources, disables animation for idle measurements and
adds independent DOM insert/update/remove churn. Without that flag it measures
the retained probe. First native-window-frame and document-ready times have
different meanings and are labeled separately. With `--output`, raw logs/events
are retained in a sibling generated directory. These are measurements, not
arbitrary hard-coded performance promises.

## Native acceptance evidence

In the actual KDE native Wayland or Windows RDP session:

```sh
python3 tools/native_acceptance.py --automated --output verification/native
python3 tools/renderer_acceptance.py --automated --output verification/renderer-native
```

The automated subset checks window creation, host communication, clipboard
self-round-trip and quit. It deliberately leaves the 15 real desktop checks
BLOCKED until observations exist. It never converts compilation or the automated
subset into full qualification. The clipboard check restores only the previous
*plain text*: a non-text clipboard (an image copied elsewhere) is replaced. Save
it first if it matters.

The renderer ledger adds R01–R12 from
[RENDERER_ACCEPTANCE.md](../docs/RENDERER_ACCEPTANCE.md), separately from N01–N15.
Both new ledgers bind evidence to the current executable source/asset/dependency
hash and refuse a changed source or different check set in the same output.
Use a new milestone directory after an executable change. Historical v0.1
reports remain under `docs/verification/` and are not reused as current PASSes.

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

For this authorized lab, administrative access is `administrator@<windows-vm>`; the
hypervisor is `root@<proxmox-host>`, used only for necessary recovery of the verified
`<windows-vm>` guest. The Windows Server 2022 VM has
Desktop Experience. The dedicated test user uses a workspace on `F:` and an RDP
desktop session. Prefer the existing `ui4me_test` normal user and private record.
Headless builds and tests can run over SSH; native GUI checks
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
on failure. Add `-Hidden` for observation jobs (UI Automation queries,
screenshots): the script then runs under `conhost.exe --headless` and does not
take the foreground or keyboard focus from the application being observed.
Completion is detected from the new task's result code rather than
`LastRunTime`, which was observed stale on Windows Server 2022. This helper is not an RDP client and does not manufacture a desktop
session. The GUI process, automation process and screenshot process must share
the intended session ID.

`windows/capture_desktop.ps1` saves a desktop screenshot, target-process UI
Automation tree and session information. It can invoke one named accessible
control using `-InvokeName`. Run it inside the target RDP session with `-STA`.
This is an observation aid; review results rather than treating a tree dump as
complete accessibility proof.

## Proxmox recovery

The user has authorized recovery of the `<windows-vm>` VM through `root@<proxmox-host>`.
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

Use [STATUS.md](../docs/STATUS.md) and its linked Core 1.0 preparation reports for
the checks actually completed on this source. The retained v0.1/v0.2 reports
describe their original revisions and environments. They do not qualify this
completion candidate. A Windows source check is not
Windows execution, a socket-pair test is not the real named-pipe test, and a
headless PNG is not native presentation/input evidence.

## Core 1.0 completion qualification

Start with `python3 tools/verify_package.py` before changing the delivered source.
Follow [LOCAL_FINISH_PROMPT.md](../LOCAL_FINISH_PROMPT.md) for the complete
Linux/Python qualification, commit and push, Windows qualification, commit and
push, then final common-source stabilization. Preserve the existing Git history.

The added C01–C12 ledger complements the retained N01–N15 and R01–R12 ledgers:

```sh
python3 tools/verify.py --stage completion-native
python3 tools/completion_acceptance.py --help
```

Read the concrete procedures in
[CORE_1.0_ACCEPTANCE.md](../docs/CORE_1.0_ACCEPTANCE.md). An unobserved desktop
behavior stays BLOCKED; creating a ledger is not a completed test. The renderer
backend now also includes `completion_checks.py`, which exercises the new Python
facade through real IPC during the renderer verifier stage.

`tools/windows/uia_probe_matrix.py` runs the independent Windows UI Automation
diagnostic with per-operation logs and hard process deadlines. Use `--pid` and
optionally `--title` to identify the installed target. Choose exactly one of
`--automation-id`, `--control-name`, or `--first-edit`; the default reference
selector is AutomationId `name`. Run `--apartment MTA` for the required client
and STA separately for diagnosis. Restore the target selection between cases.
The tool changes selection only in `select` mode and focus only with `--focus`;
its watchdog terminates its own probe process, never the target application.

`measure_resources.py` now defaults to the real system motion preference.
`--reduced-motion reduce` and `--reduced-motion no-preference` provide explicit
comparison cases. The normal reference page is static; animation is enabled
deliberately by adding its documented demonstration class.
