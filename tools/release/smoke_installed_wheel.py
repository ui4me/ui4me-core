"""Verify the installed wheel's native executable without opening a desktop."""
from __future__ import annotations
from importlib import metadata
import subprocess
import importlib.resources
from ui4me_core import scaffold, skills
import ui4me_core
from ui4me_core.launcher import host_binary

assert metadata.version('ui4me-core')=='1.0.0'
assert ui4me_core.__version__=='1.0.0'
binary=host_binary()
assert binary.is_file() and binary.stat().st_size>1024*1024
value=subprocess.check_output([str(binary),'--version'],timeout=15,text=True)
assert '1.0.0' in value,(value,binary)
skill=importlib.resources.files('ui4me_core').joinpath('_skills','ui4me-python','SKILL.md')
assert skill.is_file(), 'Installed wheel lacks UI4ME coding-agent skill'
assert 'name: ui4me-python' in skill.read_text(encoding='utf-8')
assert 'index.html' in scaffold.templates('Smoke Demo', '/venv/bin/python')
assert skills.SKILL_NAME == 'ui4me-python'
print('PASS installed Python wheel, native host, project generator and coding-agent skill:',binary,value.strip())
