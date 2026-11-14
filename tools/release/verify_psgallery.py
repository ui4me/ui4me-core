"""Offline post-staging checks for the PowerShell Gallery module. No pwsh required."""
from __future__ import annotations
import hashlib,json,os,re,stat,sys
from pathlib import Path

def digest(p:Path)->str:return hashlib.sha256(p.read_bytes()).hexdigest()
def validate(mod:Path)->dict:
    if not mod.is_dir() or mod.name!='UI4ME.Core':raise ValueError('Expected staged UI4ME.Core folder')
    psd1=mod/'UI4ME.Core.psd1'
    psm1=mod/'UI4ME.Core.psm1'
    for p in (psd1,psm1,mod/'private/UI4ME.Transport.cs',mod/'LICENSE'):
        if not p.is_file():raise ValueError(f'Missing module source: {p}')
    if not re.search(r"ModuleVersion\s*=\s*'1\.0\.1'",psd1.read_text()):raise ValueError('Unexpected module version')
    if 'Publish-PSResource' in psm1.read_text():raise ValueError('Runtime module must not publish')
    evidence=json.loads((mod/'HOST-SOURCE-MANIFEST.json').read_text())
    if evidence['version']!='1.0.1' or evidence['python_runtime_required'] is not False:raise ValueError('Invalid staged metadata')
    report={'version':'1.0.1','binaries':{},'native_files':0}
    for platform,relative,magic in [('linux','runtimes/linux-x64/ui4me_core/_bin/ui4me-host',b'\x7fELF'),
                                     ('windows','runtimes/win-x64/ui4me_core/_bin/ui4me-host.exe',b'MZ')]:
        path=mod/relative
        if not path.is_file() or path.stat().st_size<1024*1024 or not path.read_bytes().startswith(magic):
            raise ValueError(f'Missing real native {platform} host: {path}')
        expected=evidence[platform]['host_sha256']
        if digest(path)!=expected:raise ValueError(f'{platform} host SHA mismatch')
        if platform=='linux' and not os.access(path,os.X_OK):
            raise ValueError('Linux host is not executable')
        report['binaries'][platform]={'sha256':expected,'bytes':path.stat().st_size}
        for rel,sha in evidence[platform]['copied_native_files'].items():
            actual=mod/'runtimes'/('linux-x64' if platform=='linux' else 'win-x64')/rel
            if not actual.is_file() or digest(actual)!=sha:raise ValueError(f'Changed native binary: {actual}')
            report['native_files']+=1
    for rel,sha in evidence['package_source_sha256'].items():
        p=mod/rel
        if not p.is_file() or digest(p)!=sha:raise ValueError(f'Changed package source: {p}')
    for rel in ('skills/ui4me-powershell/SKILL.md','skills/ui4me-powershell/references/powershell-api.md',
                'README.md','docs/install-powershell.md','docs/quickstart-powershell.md','docs/powershell-api.md',
                'docs/troubleshooting.md','docs/patterns-powershell.md','licenses/LICENSE','licenses/THIRD_PARTY_NOTICES.md',
                'runtimes/linux-x64/ui4me_core/_bin/THIRD_PARTY_LICENSES.txt',
                'runtimes/win-x64/ui4me_core/_bin/THIRD_PARTY_LICENSES.txt'):
        if not (mod/rel).is_file():raise ValueError(f'Missing packaged file: {rel}')
    if any(part in ('tests','examples','__pycache__') for p in mod.rglob('*') for part in p.relative_to(mod).parts):
        raise ValueError('Test, example or cache folders must not be packaged')
    if 'Start-UI4MEScript' not in (mod/'README.md').read_text(encoding='utf-8'):
        raise ValueError('The packaged README must show direct script startup')
    exported=set(re.findall(r"'([A-Za-z]+-UI4ME[A-Za-z]*)'",psd1.read_text().split('FunctionsToExport',1)[1].split(')',1)[0]))
    defined=set(re.findall(r'(?m)^function ([A-Za-z]+-UI4ME[A-Za-z]*)',psm1.read_text()))
    if not exported or not exported<=defined:raise ValueError(f'Manifest exports undefined functions: {sorted(exported-defined)}')
    report['exported_functions']=len(exported)
    if any(p.suffix in ('.key','.pem','.pyc') or p.name=='.env' for p in mod.rglob('*') if p.is_file()):
        raise ValueError('Potential credentials/cache in module')
    report['status']='PASS'
    return report
if __name__=='__main__':
    mod=Path(sys.argv[1]) if len(sys.argv)>1 else Path('dist/psgallery/UI4ME.Core')
    print(json.dumps(validate(mod),indent=2))
