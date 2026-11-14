# Teach your coding agent UI4ME

UI4ME ships two self-contained Agent Skills: one for Python and another for PowerShell. They explain the event model, native host boundary, supported HTML/CSS, APIs, and platform restrictions. Installing them helps an agent avoid generating browser JavaScript, Qt code or unsupported UI calls.

## Python agents

The `ui4me-skill` command comes with the `ui4me-core` package and installs the `ui4me-python` skill.

Preview changes:

```bash
ui4me-skill install --agents all --dry-run
```

Install globally for your user:

```bash
ui4me-skill install --agents all
# Equivalent module form:
python -m ui4me_core.skills install --agents all
```

Install for selected agents:

```bash
ui4me-skill install --agents claude,codex,opencode
```

Project-local installation:

```bash
ui4me-skill install --scope project --project . --agents claude,codex
```

Check or remove skills:

```bash
ui4me-skill status --agents all
ui4me-skill uninstall --agents claude
```

## PowerShell agents

The `Install-UI4MESkill` command comes with the `UI4ME.Core` module and installs the `ui4me-powershell` skill.

Preview changes:

```powershell
Install-UI4MESkill -Agents all -Scope user -DryRun
```

Install globally for your user:

```powershell
Install-UI4MESkill -Agents all -Scope user
# Equivalent explicit form:
Install-UI4MESkill -Action install -Agents all -Scope user
```

Install for selected agents:

```powershell
Install-UI4MESkill -Agents claude,codex,opencode
```

Project-local installation:

```powershell
Install-UI4MESkill -Scope project -ProjectPath . -Agents claude,codex
```

Check or remove skills:

```powershell
Install-UI4MESkill -Action status -Agents all
Install-UI4MESkill -Action uninstall -Agents claude
```

| Python option | PowerShell parameter |
| --- | --- |
| `install` / `status` / `uninstall` | `-Action install` (default) / `status` / `uninstall` |
| `--agents all` or `claude,codex,...` | `-Agents all` or `claude,codex,...` |
| `--scope user` (default) / `project` | `-Scope user` (default) / `project` |
| `--project PATH` | `-ProjectPath PATH` |
| `--dry-run` | `-DryRun` |
| `--force` | `-Force` |

## Supported agent locations

| Agent | Per-user destination (relative to home) | Project destination |
| --- | --- | --- |
| Claude Code | `.claude/skills/<skill>/` | `.claude/skills/<skill>/` |
| Codex | `.agents/skills/<skill>/` | `.agents/skills/<skill>/` |
| OpenCode | `.config/opencode/skills/<skill>/` | `.opencode/skills/<skill>/` |
| GitHub Copilot | `.copilot/skills/<skill>/` | `.github/skills/<skill>/` |
| Cursor | `.cursor/skills/<skill>/` | `.cursor/skills/<skill>/` |
| Gemini CLI | `.gemini/skills/<skill>/` | `.gemini/skills/<skill>/` |

`<skill>` is `ui4me-python` for the Python installer and `ui4me-powershell` for the PowerShell one, so both skills can be installed side by side. The installers create only their own managed skill folders. They refuse to silently replace unrelated or user-modified skills; `--force` / `-Force` is explicit and should be used only after inspection.

## What to ask an agent

> Build a small native UI4ME Core application with a local HTML/CSS interface and Python (or PowerShell) logic. Use the installed UI4ME skill, no JavaScript or browser. Make the application executable with `python app.py` or `pwsh -File app.ps1`. Use named callbacks and explain how to run it.

For the source skill materials, see [Python skill](https://github.com/ui4me/ui4me-core/tree/main/skills/ui4me-python) and [PowerShell skill](https://github.com/ui4me/ui4me-core/tree/main/skills/ui4me-powershell).
