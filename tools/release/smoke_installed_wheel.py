"""Verify the installed wheel's native executable without opening a desktop."""
from __future__ import annotations
from importlib import metadata
import subprocess
import ui4me_core
from ui4me_core.launcher import host_binary

assert metadata.version('ui4me-core')=='1.0.0'
assert ui4me_core.__version__=='1.0.0'
binary=host_binary()
assert binary.is_file() and binary.stat().st_size>1024*1024
value=subprocess.check_output([str(binary),'--version'],timeout=15,text=True)
assert '1.0.0' in value,(value,binary)
print('PASS installed Python wheel and native host:',binary,value.strip())
