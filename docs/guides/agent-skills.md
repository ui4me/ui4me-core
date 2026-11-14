# Teach your coding agent UI4ME

UI4ME ships two self-contained Agent Skills: one for Python and another for PowerShell. They explain the event model, native host boundary, supported HTML/CSS, APIs, and platform restrictions. Installing them helps an agent avoid generating browser JavaScript, Qt code or unsupported UI calls.

## Python agents

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

```powershell
Install-UI4MESkill -Agents all -Scope user -DryRun
Install-UI4MESkill -Agents all -Scope user
Install-UI4MESkill -Agents claude,codex -Scope project -ProjectPath .
Install-UI4MESkill -Action status -Agents all
Install-UI4MESkill -Action uninstall -Agents claude -Scope user
```

## Supported agent locations

| Agent | Per-user Python skill destination (relative to home) | Project destination |
| --- | --- | --- |
| Claude Code | `.claude/skills/ui4me-python/` | `.claude/skills/ui4me-python/` |
| Codex | `.agents/skills/ui4me-python/` | `.agents/skills/ui4me-python/` |
| OpenCode | `.config/opencode/skills/ui4me-python/` | `.opencode/skills/ui4me-python/` |
| GitHub Copilot | `.copilot/skills/ui4me-python/` | `.github/skills/ui4me-python/` |
| Cursor | `.cursor/skills/ui4me-python/` | `.cursor/skills/ui4me-python/` |
| Gemini CLI | `.gemini/skills/ui4me-python/` | `.gemini/skills/ui4me-python/` |

PowerShell uses the same destination conventions but the folder name `ui4me-powershell`. The installers create only their own managed skill folders. They refuse to silently replace unrelated or user-modified skills; `-Force` is explicit and should be used only after inspection.

## What to ask an agent

> Build a small native UI4ME Core application with a local HTML/CSS interface and Python (or PowerShell) logic. Use the installed UI4ME skill, no JavaScript or browser. Make the application executable with `python app.py` or `pwsh -File app.ps1`. Use named callbacks and explain how to run it.

For the source skill materials, see [Python skill](https://github.com/ui4me/ui4me-core/tree/main/skills/ui4me-python) and [PowerShell skill](https://github.com/ui4me/ui4me-core/tree/main/skills/ui4me-powershell).
