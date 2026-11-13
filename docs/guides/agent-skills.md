# Coding-agent skill for UI4ME Python

This repository ships a `SKILL.md` and self-contained reference files so a coding agent can build correct UI4ME apps without guessing APIs or importing a browser framework. It covers local HTML/CSS, Python event callbacks, generated pages, document replacement, native dialogs, resource bundles, live bitmaps, and the host/process model.

## Install for your user (all projects)

Linux:

```bash
python3 -m ui4me_core.skills install --agents all
```

Windows PowerShell:

```powershell
py -m ui4me_core.skills install --agents all
```

Use the interpreter/virtual environment where `ui4me-core` is installed. If you only use Claude Code and Codex, choose `--agents claude,codex`. This installs only **one skill named `ui4me-python`** into each selected agent's user directory; it does not install dependencies, modify agent settings, or require admin/root access.

| Agent | User-level skill folder (Linux and Windows home equivalent) |
| --- | --- |
| Claude Code | `~/.claude/skills/ui4me-python/` |
| Codex | `~/.agents/skills/ui4me-python/` |
| OpenCode | `~/.config/opencode/skills/ui4me-python/` |
| GitHub Copilot | `~/.copilot/skills/ui4me-python/` |
| Cursor | `~/.cursor/skills/ui4me-python/` |
| Gemini CLI | `~/.gemini/skills/ui4me-python/` |

Skill-folder recognition depends on the installed agent version. **There is no universally recognized global skills directory.** Installer paths are explicit and inspectable.

## Inspect, update and remove

```bash
ui4me-skill install --agents all --dry-run
ui4me-skill status --agents all
ui4me-skill install --agents claude,codex
ui4me-skill uninstall --agents codex
```

Existing unrelated/unmanaged skill folders are refused. An installed skill edited by a user is not overwritten or deleted unless `--force` is explicit. The installer keeps ownership hashes in `.ui4me-managed.json` and rejects symlinked destination paths.

For one repository instead of your whole user profile:

```bash
ui4me-skill install --scope project --project /path/to/your-project --agents claude,codex
```

Use the appropriate absolute Windows path after `--project` in PowerShell. The skill source is included in both this Git repository and the PyPI binary wheels.

## How to ask an agent

> Build a native desktop task tracker in Python using the UI4ME Core skill. Use local index.html/style.css, a typed app.toml manifest, Python click/change callbacks, native file pickers and atomic document updates. No JavaScript or web server. Show me the exact host launch command.

The agent should use the built-in `ui4me-new` scaffold or follow the skill examples. Verify that the application is started with `ui4me-host run --app ...`, not `python app.py` directly.

[Skill source](../../skills/ui4me-python/SKILL.md) · [Reference API](../reference/python-api.md).
