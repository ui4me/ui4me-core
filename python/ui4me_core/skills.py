"""Install the bundled UI4ME Python coding skill into user or project agent folders.

No network access, elevation, or shell commands are required.  Only the
ui4me-python skill is managed; existing unrelated skills are never touched.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.resources
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from typing import Iterable

SKILL_NAME = 'ui4me-python'
MARKER = '.ui4me-managed.json'
AGENTS = ('claude', 'codex', 'opencode', 'copilot', 'cursor', 'gemini')
USER_PATHS = {
    'claude': '.claude/skills',
    'codex': '.agents/skills',
    'opencode': '.config/opencode/skills',
    'copilot': '.copilot/skills',
    'cursor': '.cursor/skills',
    'gemini': '.gemini/skills',
}
PROJECT_PATHS = {
    'claude': '.claude/skills',
    'codex': '.agents/skills',
    'opencode': '.opencode/skills',
    'copilot': '.github/skills',
    'cursor': '.cursor/skills',
    'gemini': '.gemini/skills',
}


def skill_source() -> Path:
    """Prefer the embedded wheel payload; allow running from a source tree."""
    embedded = importlib.resources.files('ui4me_core').joinpath('_skills', SKILL_NAME)
    candidate = Path(os.fspath(embedded))
    if (candidate / 'SKILL.md').is_file():
        return candidate
    checkout = Path(__file__).resolve().parents[2] / 'skills' / SKILL_NAME
    if (checkout / 'SKILL.md').is_file():
        return checkout
    raise FileNotFoundError('UI4ME coding skill is missing from this distribution')


def files_hash(root: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for p in sorted(root.rglob('*')):
        if p.is_symlink():
            raise ValueError(f'Symlink inside installed skill: {p}')
        if p.is_file() and p.name != MARKER:
            result[p.relative_to(root).as_posix()] = hashlib.sha256(p.read_bytes()).hexdigest()
    if 'SKILL.md' not in result:
        raise ValueError('Skill must contain SKILL.md')
    return result


def marker_data(path: Path) -> dict | None:
    marker = path / MARKER
    if path.is_symlink() or marker.is_symlink():
        raise ValueError('Refusing to manage a symlinked skill directory')
    if not marker.is_file():
        return None
    try:
        data = json.loads(marker.read_text(encoding='utf-8'))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f'Invalid ownership marker: {marker}') from exc
    if data.get('name') != SKILL_NAME or not isinstance(data.get('files'), dict):
        raise ValueError(f'Unexpected ownership marker: {marker}')
    return data


def skill_status(path: Path) -> str:
    if not path.exists() and not path.is_symlink():
        return 'missing'
    if path.is_symlink() or not path.is_dir():
        return 'unmanaged'
    marker = marker_data(path)
    if marker is None:
        return 'unmanaged'
    return 'installed' if files_hash(path) == marker['files'] else 'modified'


def _safe_parent(target: Path) -> None:
    """Refuse symlinked pre-existing directories (including ancestors)."""
    for ancestor in [target, *target.parents]:
        if ancestor.is_symlink():
            raise ValueError(f'Refusing symlinked destination path: {ancestor}')


def install_skill(source: Path, target: Path, *, force: bool = False) -> str:
    _safe_parent(target)
    if target.exists():
        state = skill_status(target)
        if state == 'unmanaged' and not force:
            raise FileExistsError(f'Unmanaged skill at {target}; use --force to replace it')
        if state == 'modified' and not force:
            raise FileExistsError(f'Modified skill at {target}; use --force to replace it')
        if not target.is_dir():
            raise FileExistsError(f'Non-directory exists at {target}')
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.ui4me-skill-', dir=target.parent) as tmp:
        staged = Path(tmp) / SKILL_NAME
        shutil.copytree(source, staged, symlinks=False)
        manifest = files_hash(staged)
        (staged / MARKER).write_text(json.dumps({'name': SKILL_NAME, 'files': manifest}, sort_keys=True, indent=2) + '\n', encoding='utf-8')
        if not target.exists():
            staged.rename(target)
            return 'installed'
        # The rename pair is guarded; restore the original if the swap fails.
        backup = Path(tmp) / 'original'
        target.rename(backup)
        try:
            staged.rename(target)
        except BaseException:
            backup.rename(target)
            raise
        return 'updated'


def uninstall_skill(target: Path, *, force: bool = False) -> str:
    _safe_parent(target)
    state = skill_status(target)
    if state == 'missing':
        return 'missing'
    if state in ('modified', 'unmanaged') and not force:
        raise FileExistsError(f'Will not delete {state} directory {target} without --force')
    if target.is_symlink() or not target.is_dir():
        raise ValueError(f'Refusing non-directory target: {target}')
    shutil.rmtree(target)
    return 'removed'


def agent_names(text: str) -> tuple[str, ...]:
    if text == 'all':
        return AGENTS
    values = tuple(dict.fromkeys(v.strip().lower() for v in text.split(',')))
    if not values or any(x not in AGENTS for x in values):
        raise ValueError(f'Agents must be all or a comma-separated subset of: {", ".join(AGENTS)}')
    return values


def locations(names: Iterable[str], scope: str, *, project: Path | None = None, home: Path | None = None) -> dict[str, Path]:
    home = home or Path.home()
    if scope == 'user':
        base, mapping = home, USER_PATHS
    elif scope == 'project':
        if project is None:
            raise ValueError('Project scope requires --project PATH')
        base, mapping = project.resolve(strict=True), PROJECT_PATHS
        if not base.is_dir():
            raise ValueError('Project path is not a directory')
    else:
        raise ValueError('scope must be user or project')
    return {name: base / mapping[name] / SKILL_NAME for name in names}


def _safe_console() -> None:
    """Never fail after doing the work just because the console code page
    (e.g. cp1252 on a redirected Windows stdout) cannot show a path."""
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(errors='backslashreplace')
        except (AttributeError, ValueError):
            pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('install', 'status', 'uninstall'))
    parser.add_argument('--agents', default='all', help='all or comma-separated: claude,codex,opencode,copilot,cursor,gemini')
    parser.add_argument('--scope', choices=('user', 'project'), default='user', help='user = global per-user skills')
    parser.add_argument('--project', type=Path, help='Project directory for --scope project')
    parser.add_argument('--force', action='store_true', help='Explicitly replace/remove a modified or unmanaged skill')
    parser.add_argument('--dry-run', action='store_true', help='Print intended operations without modifying the filesystem')
    args = parser.parse_args(argv)
    _safe_console()
    if args.scope == 'project' and args.project is None:
        parser.error('--scope project requires --project PATH')
    if args.scope == 'user' and args.project is not None:
        parser.error('--project is only supported with --scope project')
    try:
        dests = locations(agent_names(args.agents), args.scope, project=args.project)
        source = skill_source() if args.command == 'install' else None
        errors = 0
        for name, target in dests.items():
            try:
                if args.dry_run:
                    result = 'would ' + args.command + ' (current: ' + skill_status(target) + ')'
                elif args.command == 'status':
                    result = skill_status(target)
                elif args.command == 'install':
                    result = install_skill(source, target, force=args.force)
                else:
                    result = uninstall_skill(target, force=args.force)
                print(f'{name}: {result} — {target}')
            except (OSError, ValueError) as exc:
                errors += 1
                print(f'{name}: ERROR — {exc}')
        return 1 if errors else 0
    except (OSError, ValueError) as exc:
        parser.exit(2, f'ERROR: {exc}\n')


if __name__ == '__main__':
    raise SystemExit(main())
