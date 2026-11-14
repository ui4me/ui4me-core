# Projects, running and troubleshooting

## Layout

```text
my-app/
  app.ps1         # Start-UI4MEScript near the top; UTF-8 with BOM
  index.html      # structure; ids for everything PowerShell touches
  style.css       # linked from index.html
  assets/         # images, fonts (local only)
  app.toml        # optional manifest for Start-UI4MEHost -Manifest / ui4me-host run --app
```

`New-UI4MEProject -Path DIR [-Name NAME] [-AppId ID] [-WindowsPowerShell] [-Force]` creates
exactly this (it refuses to overwrite these files without `-Force`). Its `app.ps1` uses
`Start-UI4MEScript -Name NAME -AppId ID`; `app.toml` has the same app ID and runs `app.ps1`
with `pwsh`, or `powershell.exe` with `-WindowsPowerShell`. The module itself is the only
dependency (`Install-Module UI4ME.Core`); there is no per-project dependency file.

Resolve files from the script, not the working directory: `Join-Path $PSScriptRoot 'index.html'`.

## Running

| Command | When |
| --- | --- |
| `pwsh -File ./app.ps1` | PowerShell 7.2+ on Linux or Windows, with `UI4ME.Core` installed. |
| `powershell.exe -NoProfile -File .\app.ps1` | Windows PowerShell 5.1. |
| `./app.ps1` | From an already open prompt (arguments only pass through with `-ArgumentList`). |
| `Start-UI4MEHost -Manifest ./app.toml` | Explicit manifest launch (packaging, desktop launchers, custom timeouts/env); `ui4me-host run --app app.toml` is the same. |

Useful environment variables: `$env:UI4ME_VERBOSE = '1'` (all host logs),
`$env:UI4ME_TEST_MODE = '1'` (headless host for tests; no window, app must quit itself),
`$env:UI4ME_HOST_BINARY` (development host build).

## Structuring code

- Keep the top of the script side-effect free; everything above `Start-UI4MEScript` runs twice.
- Register all callbacks before `Wait-UI4ME -UntilShutdown`; it blocks until shutdown.
- Group per-screen setup in functions (`Show-Home`), each loading HTML and registering its
  callbacks on the returned document. Callbacks that use the function's own variables need
  `.GetNewClosure()`.
- Keep application state in a script-level hashtable (`$state = @{ dirty = $false }`); the
  page only displays it. Policy and event callbacks share one runspace, so no lock is needed.
- Background work: `Start-Job` (or `Start-ThreadJob`) without UI4ME commands inside; poll it
  with `Receive-Job` from a `Wait-UI4ME -TimeoutMilliseconds 100` loop.
- For headless smoke tests, add `$null = Request-UI4MEQuit -Application $app` before
  `Wait-UI4ME -UntilShutdown` behind an `if ($env:UI4ME_TEST_MODE -eq '1')` check.

## Common mistakes

| Symptom | Cause / fix |
| --- | --- |
| Something happens twice at startup (two log lines, file written twice) | It runs above `Start-UI4MEScript`. Move it below. |
| `Start-UI4MEScript must be called from a .ps1 script file` | Pasted into a prompt or run from a scriptblock. Save to a `.ps1` file. |
| `UI4ME_ENDPOINT and UI4ME_TOKEN come from the native host` | `Connect-UI4ME` used with `pwsh -File app.ps1`. Use `Start-UI4MEScript`. |
| `The native UI4ME host is missing` | Module not installed for this platform, or `UI4ME_HOST_BINARY` points nowhere. Check `Get-UI4MEHostPath`. |
| `An incompatible UI4ME helper is already loaded` | The session loaded an older module version. Start a new PowerShell process. |
| `UI4ME.RpcException` -32602 when loading HTML | The page has `<script>`, `on...=` attributes, `contenteditable`, a color/date input or duplicate `id`s. Remove them. |
| `UI4ME.RpcException`: text exceeds 16 KiB | Split long text, or render it as a list of elements. |
| Clicking a link does nothing | Links never navigate; handle `link_activated`. |
| `UI4ME.StaleObjectException` | Using a document/node after `Open-UI4MEFile`/`Open-UI4MEHtml` or after its window closed. Use the returned document / `$window.Document`. |
| Button does nothing | Wrong `-Element` id, callback registered on an old document, `Wait-UI4ME` never reached, or JS/`onclick` used. |
| Counter or flag never changes | Assigned `$name = ...` inside a callback; use `$script:name` or a hashtable property. |
| Callback in a function fails with a null variable | Missing `.GetNewClosure()` in that function. |
| Close/quit is always refused | The policy's last output is not `$true` (stray output); discard other output with `$null =`. |
| Form clears/reloads on submit | Missing `Set-UI4MEPreventDefault -EventType submit` before input. |
| Relative image/CSS missing with `Open-UI4MEHtml` | Pass `-AssetRoot $PSScriptRoot`. Network URLs never load. |
| `UI4ME.RpcException` -32001 on `Set-UI4MEWindow -X -Y` | Wayland cannot position windows; make it optional. |
| UI freezes for later events | A callback is doing slow work; move it to a job and poll it. |
| Garbled non-ASCII text on Windows PowerShell 5.1 | Save the `.ps1` as UTF-8 with BOM. |
| `Read-Host` hangs/fails | The backend has no stdin; use a form in the window. |
| Nothing visible over SSH | The host needs the user's desktop session (Wayland on Linux; an interactive session on Windows). |

## Packaging and distribution

The `UI4ME.Core` module contains the native hosts for Linux x64 and Windows x64; it does not
bundle your app. Ship your project folder, declare the module requirement in the script
(`Import-Module UI4ME.Core -MinimumVersion 1.0.1`, or `#Requires -Modules UI4ME.Core`), and
run it with `pwsh -File app.ps1`, or install a desktop launcher that runs
`Start-UI4MEHost -Manifest /path/to/app.toml` with a manifest that names the right shell
(`pwsh` or `powershell.exe`). Use a fixed reverse-DNS `-AppId` so single-instance behavior
and per-app settings stay stable.

## Agent skill installation

`Install-UI4MESkill -Agents all` (or `claude,codex,opencode,copilot,cursor,gemini`) copies
this skill into each agent's per-user folder; `-Scope project -ProjectPath PATH` installs
into a repository; `-Action status`, `-Action uninstall`, `-DryRun`, `-Force` are available.
It never touches unrelated skills or agent settings.

More: https://github.com/ui4me/ui4me-core/blob/main/docs/guides/troubleshooting.md and https://github.com/ui4me/ui4me-core/blob/main/docs/reference/host-cli.md
