"""Offline structural validation for Linux UI4ME wheels and source archives."""
from __future__ import annotations
from pathlib import Path
import argparse, hashlib, json, re, struct, tarfile, zipfile


# Local agent instructions, VCS metadata, build/test outputs, caches and
# credentials never belong in a distributed artifact.
LOCAL_ONLY = {'AGENTS.md', 'CLAUDE.md', 'LOCAL_FINISH_PROMPT.md'}
EXCLUDED_ANYWHERE = {'.git', '__pycache__', '.pytest_cache', '.venv'}
EXCLUDED_TOP_LEVEL = {'target', 'verification', 'dist', 'build', '.local', 'evidence'}
SENSITIVE_SUFFIXES = ('.pem', '.key', '.secret', '.pyc', '.pyo', '.log', '.credentials.json', '.local.json')


def check_distributable(names) -> None:
    for name in names:
        parts = Path(name).parts
        # sdist members are prefixed by `ui4me_core-1.0.0/`; wheel members are not.
        rel = parts[1:] if parts and parts[0].startswith('ui4me_core-') else parts
        if rel and rel[-1] in LOCAL_ONLY and len(rel) == 1:
            raise ValueError(f'Local-only development file in artifact: {name}')
        if any(p in EXCLUDED_ANYWHERE for p in rel) or (len(rel) > 1 and rel[0] in EXCLUDED_TOP_LEVEL):
            raise ValueError(f'VCS, build, cache or evidence directory in artifact: {name}')
        if name.lower().endswith(SENSITIVE_SUFFIXES):
            raise ValueError(f'Sensitive, log or cache file in artifact: {name}')


def validate_wheel(path: Path) -> dict:
    with zipfile.ZipFile(path) as z:
        bad=z.testzip()
        if bad: raise ValueError(f'Corrupt member in {path}: {bad}')
        entries=z.namelist()
        binaries=[x for x in entries if x.endswith('/_bin/ui4me-host')]
        if len(binaries)!=1: raise ValueError('Wheel must contain exactly one UI4ME host')
        data=z.read(binaries[0])
        if not data.startswith(b'\x7fELF') or len(data)<1024*1024:
            raise ValueError('Wheel binary is not a compiled Linux ELF host')
        if data[4]!=2 or data[5]!=1 or struct.unpack_from('<H',data,18)[0]!=62:
            raise ValueError('Linux release requires ELF64 little-endian x86_64')
        wheel_files=[x for x in entries if x.endswith('.dist-info/WHEEL')]
        if len(wheel_files)!=1: raise ValueError('Missing wheel metadata')
        wheel_meta=z.read(wheel_files[0]).decode()
        tags=[line[5:].strip() for line in wheel_meta.splitlines() if line.startswith('Tag: ')]
        if not tags or any(t.endswith('-any') for t in tags):
            raise ValueError(f'Platform-specific binary must not use universal tags: {tags}')
        # No CPython extension: one wheel serves every Python 3 on the platform.
        if any(not t.startswith('py3-none-') or 'x86_64' not in t for t in tags):
            raise ValueError(f'Expected py3-none-<linux x86_64 platform> tags: {tags}')
        if any('.data/' in x for x in entries):
            raise ValueError('Package modules must install at the wheel root, not under .data/')
        licenses=z.read('ui4me_core/_bin/THIRD_PARTY_LICENSES.txt').decode() if 'ui4me_core/_bin/THIRD_PARTY_LICENSES.txt' in entries else ''
        if 'stylo' not in licenses or 'MPL-2.0' not in licenses or 'Apache-2.0' not in licenses:
            raise ValueError('Wheel lacks the statically linked dependencies\' license texts')
        for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md', 'FONT-LICENSE.txt', 'MPL-2.0.txt'):
            if not any(x.endswith('.dist-info/licenses/'+name) or (x.endswith('/'+name) and '.dist-info/licenses/' in x) for x in entries):
                raise ValueError(f'Wheel metadata lacks license file {name}')
        check_distributable(entries)
        for needed in (
            'ui4me_core/skills.py', 'ui4me_core/scaffold.py',
            'ui4me_core/_skills/ui4me-python/SKILL.md',
            'ui4me_core/_skills/ui4me-python/references/python-api.md',
        ):
            if needed not in entries:
                raise ValueError(f'Wheel is missing the coding skill or developer CLI: {needed}')
        for needed in ('ui4me_core/application.py','ui4me_core/client.py','ui4me_core/live_bitmap.py','ui4me_core/launcher.py'):
            if needed not in entries: raise ValueError(f'Missing module {needed}')
        return {'filename':path.name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                'bytes':path.stat().st_size,'host_bytes':len(data),'tags':tags,'result':'PASS'}


def validate_sdist(path: Path) -> dict:
    with tarfile.open(path, 'r:*') as t:
        names=t.getnames()
        if not names: raise ValueError('Empty source distribution')
        for n in names:
            if '..' in Path(n).parts or n.startswith('/'):
                raise ValueError('Unsafe sdist member')
        check_distributable(names)
        essentials=['Cargo.lock','Cargo.toml','pyproject.toml','setup.py',
                    'crates/ui4me-host/src/host/bitmap.rs',
                    'patches/stylo/stylo/style/stylist.rs',
                    # Extensionless build inputs (build scripts read them).
                    'crates/ui4me-assets/assets/last-resort/SHA256SUMS',
                    'tools/release/collect_licenses.py',
                    'tools/release/verify_publish_dist.py',
                    'python/ui4me_core/skills.py',
                    'python/ui4me_core/scaffold.py',
                    'skills/ui4me-python/SKILL.md',
                    'skills/ui4me-python/references/python-api.md',
                    'docs/guides/quickstart.md',
                    'docs/images/desktop-workspace.png']
        for part in essentials:
            if not any(n.endswith('/'+part) for n in names):
                raise ValueError(f'Sdist is missing {part}')
        return {'filename':path.name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                'bytes':path.stat().st_size,'files':len(names),'result':'PASS'}


def main() -> None:
    ap=argparse.ArgumentParser()
    ap.add_argument('dist_dir', type=Path, nargs='?',default=Path('dist'))
    args=ap.parse_args()
    sdists=sorted(args.dist_dir.glob('ui4me_core-1.0.0.tar.gz'))
    wheels=sorted(args.dist_dir.glob('ui4me_core-1.0.0-*.whl'))
    if not sdists or not wheels:
        raise SystemExit('Both source distribution and a Linux wheel must exist')
    reports=[validate_sdist(x) for x in sdists]+[validate_wheel(x) for x in wheels]
    print(json.dumps({'version':'1.0.0','artifacts':reports,'status':'PASS'},indent=2))


if __name__ == '__main__':main()
