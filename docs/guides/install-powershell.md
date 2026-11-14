# Install UI4ME Core for PowerShell

The PowerShell module **UI4ME.Core** contains the native window host for Linux and Windows.
Nothing else is needed at run time: no Python, Rust, browser, JavaScript runtime or web
server.

## Supported systems

| Shell | Operating system |
| --- | --- |
| PowerShell 7.2 or later (`pwsh`) | Linux x86-64 (glibc 2.34+, Wayland desktop session), Windows x64 |
| Windows PowerShell 5.1 (`powershell.exe`) | Windows x64 |

ARM64 and macOS are not supported in 1.0.1, and X11-only Linux sessions are not qualified. See
[platforms](platforms.md) for desktop behavior and the qualified environments.

## Install

In PowerShell, for your account only (no administrator rights needed):

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
```

- With PSResourceGet (PowerShell 7.4+, or installed separately) you can use
  `Install-PSResource UI4ME.Core -Scope CurrentUser` instead.
- On Windows PowerShell 5.1 the first `Install-Module` may ask to install the NuGet provider
  and to trust the PowerShell Gallery; answer **Yes**.

## Check the installation

```powershell
Import-Module UI4ME.Core
Get-Module UI4ME.Core | Select-Object Name, Version, Path
Get-UI4MEHostPath
& (Get-UI4MEHostPath) --version
```

`Get-UI4MEHostPath` prints the bundled native host. On Linux it also restores the execute
permission that package extraction drops; if that fails (for example in a read-only,
all-users module folder) run `chmod +x` on the printed path.

Then run the [quickstart](quickstart-powershell.md):

```powershell
New-UI4MEProject -Path ./hello -Name 'Hello' -AppId org.example.Hello
pwsh -File ./hello/app.ps1
```

On Windows PowerShell 5.1 run `powershell.exe -NoProfile -File .\hello\app.ps1` instead.

## Update and uninstall

```powershell
Update-Module UI4ME.Core
Uninstall-Module UI4ME.Core -AllVersions
```

Start a new PowerShell session after an update: the module compiles a small C# helper into
the session on first use (about 0.1-0.5 s), and a session that already loaded an older
helper reports "An incompatible UI4ME helper is already loaded".

`Start-UI4MEScript` (direct `pwsh -File ./app.ps1` start) needs UI4ME.Core 1.0.1 or later.
Scripts that need it can say so with `Import-Module UI4ME.Core -MinimumVersion 1.0.1`.

## Execution policy

The script you start yourself follows your execution policy as usual. If Windows refuses to
run it, either start it with `-ExecutionPolicy Bypass` or allow your own scripts once:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

The backend process that UI4ME starts for your app (and the backend in a generated
`app.toml`) is run with `-ExecutionPolicy Bypass` for that process only. An organization's
Group Policy still takes precedence.

## Coding agents

The module bundles a skill that teaches coding agents (Claude Code, Codex, OpenCode, GitHub
Copilot, Cursor, Gemini CLI) to write UI4ME apps:

```powershell
Install-UI4MESkill -Agents all -Scope user
```

See [agent skills](agent-skills.md).

## Working from a source checkout

Without the Gallery package, import the module from the repository and point
`UI4ME_HOST_BINARY` at a native host you built (see [development](development.md)):

```powershell
Import-Module ./powershell/UI4ME.Core/UI4ME.Core.psd1
$env:UI4ME_HOST_BINARY = (Resolve-Path ./target/release/ui4me-host).Path   # ui4me-host.exe on Windows
```

Problems? See [troubleshooting](troubleshooting.md).
