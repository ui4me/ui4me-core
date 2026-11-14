"""Lightweight source integrity check before and after local qualification."""
from __future__ import annotations
from pathlib import Path
import hashlib, json, tomllib

ROOT=Path(__file__).resolve().parents[2]
versions={}
versions['rust']=tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
versions['python']=tomllib.loads((ROOT/'pyproject.toml').read_text())['project']['version']
lock=tomllib.loads((ROOT/'Cargo.lock').read_text())['package']
for item in lock:
    if item['name'] in ('ui4me-host','ui4me-document','ui4me-renderer','ui4me-assets','ui4me-engine-types'):
        versions[item['name']]=item['version']
import re
def declared(path, pattern):
    match=re.search(pattern,(ROOT/path).read_text(encoding='utf-8'),re.M)
    return match.group(1) if match else 'missing'
versions['python-module']=declared('python/ui4me_core/__init__.py',r'^__version__ = "([^"]+)"')
versions['python-client']=declared('python/ui4me_core/client.py',r'^CLIENT_VERSION = "([^"]+)"')
versions['powershell-manifest']=declared('powershell/UI4ME.Core/UI4ME.Core.psd1',r"ModuleVersion = '([^']+)'")
versions['powershell-module']=declared('powershell/UI4ME.Core/UI4ME.Core.psm1',r"^\$script:UI4MEVersion = '([^']+)'")
if any(v!='1.0.1' for v in versions.values()):
    raise SystemExit(f'Inconsistent final version: {versions}')
required=['Cargo.lock','pyproject.toml','setup.py','MANIFEST.in','LICENSE',
          'third_party/README.md','crates/ui4me-host/src/host/bitmap.rs',
          'tools/release/build_linux.sh','tools/release/verify_linux_artifact.py',
          'python/ui4me_core/launcher.py','python/ui4me_core/live_bitmap.py',
          'examples/python/ui/live_bitmap.html','python/ui4me_core/_bootstrap.py',
          'tools/release/SOURCE_FILES.sha256','docs/release/RELEASE_NOTES_1.0.1.md',
          'tools/release/collect_licenses.py','tools/release/smoke_installed_wheel.py']
missing=[f for f in required if not (ROOT/f).is_file()]
if missing:raise SystemExit(f'Missing source files: {missing}')
for doc in (ROOT/'pyproject.toml',ROOT/'setup.py'):
    if b'--force' in doc.read_bytes():raise SystemExit('Unexpected force flag')
print(json.dumps({'result':'PASS','versions':versions,'source_lock_sha256':hashlib.sha256((ROOT/'Cargo.lock').read_bytes()).hexdigest(),'required_files':len(required)},indent=2))
