"""Verify the installed wheel's native executable and direct startup without opening a desktop."""
from __future__ import annotations
from importlib import metadata
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import importlib.resources
from ui4me_core import scaffold, skills
import ui4me_core
from ui4me_core.launcher import host_binary

assert metadata.version('ui4me-core')=='1.0.1'
assert ui4me_core.__version__=='1.0.1'
binary=host_binary()
assert binary.is_file() and binary.stat().st_size>1024*1024
value=subprocess.check_output([str(binary),'--version'],timeout=15,text=True)
assert '1.0.1' in value,(value,binary)
skill=importlib.resources.files('ui4me_core').joinpath('_skills','ui4me-python','SKILL.md')
assert skill.is_file(), 'Installed wheel lacks UI4ME coding-agent skill'
assert 'name: ui4me-python' in skill.read_text(encoding='utf-8')
assert 'index.html' in scaffold.templates('Smoke Demo', '/venv/bin/python')
assert skills.SKILL_NAME == 'ui4me-python'
# `python app.py` through the installed package and host, in the host's
# headless test mode (no desktop in a build container).
APP = """import sys, threading
from ui4me_core import Application
def main():
    with Application.start(name='Installed wheel smoke', instance='multiple') as app:
        window = app.open_html("<p id='t'>direct start</p>")
        print('SMOKE', window.document.get_text('t'), sys.argv[1:])
        threading.Timer(0.2, app.request_quit).start()
        app.run()
if __name__ == '__main__':
    main()
"""
with tempfile.TemporaryDirectory(prefix='ui4me wheel smoke ') as folder:
    script = Path(folder) / 'app.py'
    script.write_text(APP, encoding='utf-8')
    env = {k: v for k, v in os.environ.items() if not k.startswith('UI4ME_') and k != 'PYTHONPATH'}
    env['UI4ME_TEST_MODE'] = '1'
    result = subprocess.run([sys.executable, str(script), 'arg one'], cwd=folder, env=env,
                            capture_output=True, text=True, encoding='utf-8', timeout=120)
    assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
    assert "SMOKE direct start ['arg one']" in result.stdout, (result.stdout, result.stderr)
print('PASS installed Python wheel, native host, direct start, project generator and coding-agent skill:',binary,value.strip())
