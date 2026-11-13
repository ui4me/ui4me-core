"""Offline structural validation for Windows UI4ME wheels (and the shared sdist)."""
from __future__ import annotations
from pathlib import Path
import argparse
import hashlib
import json
import struct
import sys
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_linux_artifact import check_distributable, validate_sdist  # noqa: E402

IMAGE_FILE_MACHINE_AMD64 = 0x8664
IMAGE_SUBSYSTEM_WINDOWS_CUI = 3


def pe_header(data: bytes) -> dict:
    """Machine and subsystem of a PE32+ image (rejects anything else)."""
    if len(data) < 1024 * 1024 or data[:2] != b'MZ':
        raise ValueError('Wheel binary is not a compiled Windows PE host')
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    if data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Missing PE signature')
    machine = struct.unpack_from('<H', data, pe + 4)[0]
    optional = pe + 24
    magic = struct.unpack_from('<H', data, optional)[0]
    if machine != IMAGE_FILE_MACHINE_AMD64 or magic != 0x20B:
        raise ValueError(f'Windows release requires a PE32+ x86-64 host (machine {machine:#x}, magic {magic:#x})')
    subsystem = struct.unpack_from('<H', data, optional + 68)[0]
    return {'machine': 'x86_64', 'subsystem': subsystem}


def validate_wheel(path: Path) -> dict:
    with zipfile.ZipFile(path) as z:
        bad = z.testzip()
        if bad:
            raise ValueError(f'Corrupt member in {path}: {bad}')
        entries = z.namelist()
        binaries = [x for x in entries if x.endswith('/_bin/ui4me-host.exe')]
        if len(binaries) != 1 or any(x.endswith('/_bin/ui4me-host') for x in entries):
            raise ValueError('Wheel must contain exactly one Windows UI4ME host')
        data = z.read(binaries[0])
        header = pe_header(data)
        if header['subsystem'] != IMAGE_SUBSYSTEM_WINDOWS_CUI:
            # The host is a console program so `--version`, `doctor` and `render`
            # print in a terminal; a GUI launch releases its own console at
            # startup (platform::windows::detach_owned_console).
            raise ValueError(f'Unexpected host subsystem {header["subsystem"]}')
        wheel_files = [x for x in entries if x.endswith('.dist-info/WHEEL')]
        if len(wheel_files) != 1:
            raise ValueError('Missing wheel metadata')
        wheel_meta = z.read(wheel_files[0]).decode()
        tags = [line[5:].strip() for line in wheel_meta.splitlines() if line.startswith('Tag: ')]
        if 'Root-Is-Purelib: false' not in wheel_meta:
            raise ValueError('A wheel with a native executable must not be purelib')
        # No CPython extension: one wheel serves every Python 3 on win_amd64.
        if tags != ['py3-none-win_amd64']:
            raise ValueError(f'Expected exactly py3-none-win_amd64: {tags}')
        if any('.data/' in x for x in entries):
            raise ValueError('Package modules must install at the wheel root, not under .data/')
        name = 'ui4me_core/_bin/THIRD_PARTY_LICENSES.txt'
        licenses = z.read(name).decode() if name in entries else ''
        if ('x86_64-pc-windows-msvc' not in licenses or 'stylo' not in licenses
                or 'MPL-2.0' not in licenses or 'windows-sys' not in licenses):
            raise ValueError('Wheel lacks the Windows host\'s statically linked dependency license texts')
        for license_name in ('LICENSE', 'THIRD_PARTY_NOTICES.md', 'FONT-LICENSE.txt', 'MPL-2.0.txt'):
            if not any('.dist-info/licenses/' in x and x.endswith('/' + license_name) for x in entries):
                raise ValueError(f'Wheel metadata lacks license file {license_name}')
        check_distributable(entries)
        for needed in ('ui4me_core/application.py', 'ui4me_core/client.py',
                       'ui4me_core/live_bitmap.py', 'ui4me_core/launcher.py'):
            if needed not in entries:
                raise ValueError(f'Missing module {needed}')
        return {'filename': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                'bytes': path.stat().st_size, 'host_bytes': len(data),
                'host_sha256': hashlib.sha256(data).hexdigest(), 'pe': header,
                'tags': tags, 'result': 'PASS'}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('dist_dir', type=Path, nargs='?', default=Path('dist'))
    args = ap.parse_args()
    sdists = sorted(args.dist_dir.glob('ui4me_core-1.0.0.tar.gz'))
    wheels = sorted(args.dist_dir.glob('ui4me_core-1.0.0-*win*.whl'))
    if not sdists or not wheels:
        raise SystemExit('Both the source distribution and a Windows wheel must exist')
    reports = [validate_sdist(x) for x in sdists] + [validate_wheel(x) for x in wheels]
    print(json.dumps({'version': '1.0.0', 'artifacts': reports, 'status': 'PASS'}, indent=2))


if __name__ == '__main__':
    main()
