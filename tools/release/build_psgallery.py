"""Stage a cross-platform UI4ME.Core PSGallery module from qualified 1.0.0 wheels.

No recompilation, no publication. The packaged native hosts are the exact
Linux/Windows host binaries taken from the same published Python release.
Requires Python 3.10+ only *at build time*; the final module has no Python
runtime dependency. Run verify_psgallery.py before Publish-PSResource.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MOD = ROOT / 'powershell' / 'UI4ME.Core'
VERSION = '1.0.0'
HOSTS = {
    'linux': ('linux-x64', 'ui4me_core/_bin/ui4me-host', b'\x7fELF', 'manylinux_2_34_x86_64'),
    'windows': ('win-x64', 'ui4me_core/_bin/ui4me-host.exe', b'MZ', 'win_amd64'),
}

def digest(path: Path) -> str:
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):
            h.update(b)
    return h.hexdigest()

def allowed(name: str) -> bool:
    if name.endswith('/') or name.startswith('/'):
        return False
    parts=PurePosixPath(name).parts
    return not ('..' in parts or '\\' in name or ':' in name)

def copy_wheel(wheel: Path, dest: Path, platform: str) -> dict:
    target,host,magic,suffix=HOSTS[platform]
    if not wheel.is_file() or not wheel.name.startswith('ui4me_core-'+VERSION+'-') or suffix not in wheel.name:
        raise ValueError(f'Expected a qualified UI4ME {VERSION} {platform} wheel: {wheel}')
    entries={}
    with zipfile.ZipFile(wheel) as z:
        bad=z.testzip()
        if bad: raise ValueError(f'Corrupt wheel member: {bad}')
        names=z.namelist()
        if names.count(host)!=1: raise ValueError(f'Expected unique host {host} in wheel')
        metadata=[n for n in names if n.endswith('.dist-info/WHEEL')]
        if len(metadata)!=1 or 'Tag: py3-none-' + suffix not in z.read(metadata[0]).decode():
            raise ValueError(f'Unexpected wheel tag: {metadata}')
        exe=z.read(host)
        if len(exe)<1024*1024 or not exe.startswith(magic):
            raise ValueError(f'Invalid or uncompiled {platform} host')
        base=dest/'runtimes'/target
        for name in names:
            if not allowed(name):
                if name.endswith('/'):continue
                raise ValueError(f'Unsafe wheel entry: {name}')
            # Preserve the exact relative paths of any native loader dependencies
            # (including auditwheel's ui4me_core.libs/), as used in the wheel.
            if name.startswith('ui4me_core/_bin/') or name.startswith('ui4me_core.libs/'):
                out=base.joinpath(*PurePosixPath(name).parts)
                out.parent.mkdir(parents=True,exist_ok=True)
                out.write_bytes(z.read(name))
                if name==host and platform=='linux':out.chmod(out.stat().st_mode|stat.S_IXUSR|stat.S_IXGRP|stat.S_IXOTH)
                entries[name]=hashlib.sha256(out.read_bytes()).hexdigest()
            elif '.dist-info/licenses/' in name:
                # Preserve third-party license notices; exact duplicate files
                # across wheels must have the same contents.
                rel=name.split('.dist-info/licenses/',1)[1]
                out=dest/'licenses'/rel
                data=z.read(name)
                if out.exists() and out.read_bytes()!=data:
                    raise ValueError(f'Conflicting license files: {rel}')
                out.parent.mkdir(parents=True,exist_ok=True)
                out.write_bytes(data)
    if host not in entries:raise AssertionError('Host was not copied')
    return {'filename':wheel.name,'wheel_sha256':digest(wheel),'host_sha256':entries[host],
            'host_bytes':len(exe),'copied_native_files':entries,'tag':suffix}

def build(linux: Path, windows: Path, output: Path, source_identity: str='') -> Path:
    if output.exists():
        if not output.is_dir():raise ValueError('Output must be a directory')
        if any(output.iterdir()):raise ValueError('Output must be empty: refusing to overwrite a prior staged release')
    output.mkdir(parents=True,exist_ok=True)
    dest=output/'UI4ME.Core'
    shutil.copytree(MOD,dest,ignore=shutil.ignore_patterns('__pycache__','*.pyc','.DS_Store'))
    # Staged modules must not include placeholder runtime README or test data.
    (dest/'runtimes'/'README.md').unlink(missing_ok=True)
    if not (dest/'LICENSE').exists():shutil.copy2(ROOT/'LICENSE',dest/'LICENSE')
    shutil.copytree(ROOT/'skills'/'ui4me-powershell',dest/'skills'/'ui4me-powershell')
    # User documentation travels with the module; repository links become absolute.
    from sync_powershell_docs import standalone
    (dest/'docs').mkdir()
    for doc in ('docs/guides/powershell.md','docs/reference/powershell-api.md'):
        source=ROOT/doc
        (dest/'docs'/source.name).write_text(standalone(source),encoding='utf-8',newline='\n')
    reports={'version':VERSION,'python_runtime_required':False,'source_identity':source_identity,
             'linux':copy_wheel(linux,dest,'linux'),
             'windows':copy_wheel(windows,dest,'windows')}
    reports['package_source_sha256']={
        str(p.relative_to(dest)).replace(os.sep,'/'):digest(p)
        for p in sorted(dest.rglob('*')) if p.is_file()
    }
    (dest/'HOST-SOURCE-MANIFEST.json').write_text(json.dumps(reports,indent=2,sort_keys=True)+'\n',encoding='utf-8')
    archive=output/'UI4ME.Core-1.0.0.zip'
    with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
        for path in sorted(dest.rglob('*')):
            if path.is_file():z.write(path,path.relative_to(output).as_posix())
    from verify_psgallery import validate
    print(json.dumps(validate(dest),indent=2))
    print('Stage:',dest,'Zip:',archive)
    return dest

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--linux-wheel',type=Path,required=True)
    ap.add_argument('--windows-wheel',type=Path,required=True)
    ap.add_argument('--out',type=Path,default=Path('dist/psgallery'))
    ap.add_argument('--source-identity',default='')
    a=ap.parse_args()
    build(a.linux_wheel,a.windows_wheel,a.out,a.source_identity)
if __name__=='__main__':main()
