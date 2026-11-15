"""Release-artifact checks: Windows stack reserve (F-11 interim / F-01) and debug hosts (RELEASE-17).

Synthetic PE32+ / ELF64 images keep the tests fast and platform independent;
a real debug host is used too when this checkout has one.
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import host_checks  # noqa: E402
import verify_linux_artifact  # noqa: E402
import verify_windows_artifact  # noqa: E402

MIB = 1024 * 1024


def pe_image(stack_reserve: int, *, subsystem: int = 3, payload: bytes = b'') -> bytes:
    data = bytearray(2 * MIB)
    data[0:2] = b'MZ'
    pe = 0x80
    struct.pack_into('<I', data, 0x3C, pe)
    data[pe:pe + 4] = b'PE\0\0'
    struct.pack_into('<H', data, pe + 4, 0x8664)
    optional = pe + 24
    struct.pack_into('<H', data, optional, 0x20B)
    struct.pack_into('<H', data, optional + 68, subsystem)
    struct.pack_into('<QQ', data, optional + 72, stack_reserve, 4096)
    data[MIB:MIB + len(payload)] = payload
    return bytes(data)


def elf_image(section_names: list[str], *, payload: bytes = b'') -> bytes:
    names = b'\0' + b''.join(n.encode() + b'\0' for n in ['.shstrtab'] + section_names)
    count = 2 + len(section_names)   # null, .shstrtab, others
    data = bytearray(2 * MIB)
    data[0:6] = b'\x7fELF\x02\x01'
    struct.pack_into('<H', data, 18, 62)
    strtab_offset = 0x1000
    data[strtab_offset:strtab_offset + len(names)] = names
    shoff = 0x2000
    struct.pack_into('<Q', data, 0x28, shoff)
    struct.pack_into('<HHH', data, 0x3A, 64, count, 1)
    offsets = [0]
    position = 1
    for name in ['.shstrtab'] + section_names:
        offsets.append(position)
        position += len(name) + 1
    for index in range(count):
        entry = shoff + index * 64
        struct.pack_into('<I', data, entry, offsets[index])
        if index == 1:
            struct.pack_into('<QQ', data, entry + 0x18, strtab_offset, len(names))
    data[MIB:MIB + len(payload)] = payload
    return bytes(data)


LICENSES = 'x86_64-pc-windows-msvc stylo MPL-2.0 windows-sys Apache-2.0'
MODULES = ['ui4me_core/skills.py', 'ui4me_core/scaffold.py', 'ui4me_core/_skills/ui4me-python/SKILL.md',
           'ui4me_core/_skills/ui4me-python/references/python-api.md', 'ui4me_core/application.py',
           'ui4me_core/client.py', 'ui4me_core/live_bitmap.py', 'ui4me_core/launcher.py',
           'ui4me_core/_bootstrap.py', 'ui4me_core/py.typed']


def wheel(folder: Path, host_name: str, host: bytes, tag: str) -> Path:
    path = folder / f'ui4me_core-0.0.0-{tag}.whl'
    with zipfile.ZipFile(path, 'w') as z:
        z.writestr(f'ui4me_core/_bin/{host_name}', host)
        z.writestr('ui4me_core/_bin/THIRD_PARTY_LICENSES.txt', LICENSES)
        z.writestr('ui4me_core-0.0.0.dist-info/WHEEL', f'Root-Is-Purelib: false\nTag: {tag}\n')
        for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md', 'FONT-LICENSE.txt', 'MPL-2.0.txt'):
            z.writestr(f'ui4me_core-0.0.0.dist-info/licenses/{name}', 'text')
        for name in MODULES:
            z.writestr(name, '')
    return path


class WindowsStackReserveTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.folder = Path(folder.name)

    def windows_wheel(self, image: bytes) -> Path:
        return wheel(self.folder, 'ui4me-host.exe', image, 'py3-none-win_amd64')

    def test_expected_reserve_is_one_named_constant_of_32_mib(self):
        self.assertEqual(host_checks.WINDOWS_HOST_STACK_RESERVE, 32 * MIB)
        self.assertIs(verify_windows_artifact.WINDOWS_HOST_STACK_RESERVE, host_checks.WINDOWS_HOST_STACK_RESERVE)

    def test_default_one_mib_stack_is_refused(self):
        # The 1.0.1 Windows host has SizeOfStackReserve 1048576 (F-01).
        with self.assertRaisesRegex(ValueError, 'SizeOfStackReserve is 1048576'):
            verify_windows_artifact.validate_wheel(self.windows_wheel(pe_image(MIB)))

    def test_32_mib_stack_is_accepted_and_reported(self):
        report = verify_windows_artifact.validate_wheel(
            self.windows_wheel(pe_image(host_checks.WINDOWS_HOST_STACK_RESERVE)))
        self.assertEqual(report['result'], 'PASS')
        self.assertEqual(report['pe']['stack_reserve'], 32 * MIB)
        self.assertEqual(len(report['sha256']), 64)

    def test_debug_windows_host_is_refused(self):
        image = pe_image(host_checks.WINDOWS_HOST_STACK_RESERVE, payload=host_checks.DEBUG_ASSERTION_MARKER)
        with self.assertRaisesRegex(ValueError, 'development'):
            verify_windows_artifact.validate_wheel(self.windows_wheel(image))

    def test_stack_reserve_reader(self):
        self.assertEqual(host_checks.pe_stack_reserve(pe_image(12345678)), 12345678)
        with self.assertRaises(ValueError):
            host_checks.pe_stack_reserve(b'\x7fELF' + bytes(100))

    def test_constant_matches_the_host_link_argument(self):
        sources = [ROOT / 'crates/ui4me-host/build.rs', ROOT / 'crates/ui4me-host/Cargo.toml',
                   ROOT / '.cargo/config.toml']
        values = set()
        for source in sources:
            if source.is_file():
                values.update(int(v, 0) for v in re.findall(r'/STACK:(0x[0-9a-fA-F]+|\d+)',
                                                             source.read_text(encoding='utf-8')))
        if not values:
            self.skipTest('the host build does not set /STACK: yet (workstream B adds it)')
        self.assertEqual(values, {host_checks.WINDOWS_HOST_STACK_RESERVE})


class DebugHostTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.folder = Path(folder.name)

    def linux_wheel(self, image: bytes) -> Path:
        return wheel(self.folder, 'ui4me-host', image, 'py3-none-manylinux_2_34_x86_64')

    def test_release_like_linux_host_passes(self):
        report = verify_linux_artifact.validate_wheel(self.linux_wheel(elf_image(['.text', '.symtab'])))
        self.assertEqual(report['result'], 'PASS')

    def test_linux_host_with_debug_sections_is_refused(self):
        with self.assertRaisesRegex(ValueError, r'debug sections \.debug_info'):
            verify_linux_artifact.validate_wheel(self.linux_wheel(elf_image(['.text', '.debug_info'])))

    def test_linux_host_with_debug_assertions_is_refused(self):
        image = elf_image(['.text'], payload=host_checks.DEBUG_ASSERTION_MARKER)
        with self.assertRaisesRegex(ValueError, 'debug-assertion'):
            verify_linux_artifact.validate_wheel(self.linux_wheel(image))

    def test_oversized_host_is_refused(self):
        big = elf_image(['.text']) + bytes(host_checks.MAX_RELEASE_HOST_BYTES)
        self.assertTrue(any('exceeds' in r for r in host_checks.debug_build_markers(big)))

    def test_real_debug_host_is_detected(self):
        name = 'ui4me-host.exe' if os.name == 'nt' else 'ui4me-host'
        host = ROOT / 'target' / 'debug' / name
        if not host.is_file():
            self.skipTest('no debug host in this checkout')
        self.assertTrue(host_checks.debug_build_markers(host.read_bytes()))

    def test_setup_py_refuses_a_debug_wheel_host(self):
        try:
            import setuptools  # noqa: F401
            import wheel  # noqa: F401
        except ImportError:
            self.skipTest('setuptools/wheel not installed')
        host = self.folder / 'ui4me-host'
        host.write_bytes(elf_image(['.debug_info']))
        env = {k: v for k, v in os.environ.items() if not k.startswith('UI4ME_')}
        env['UI4ME_WHEEL_HOST_BINARY'] = str(host)
        egg_info = ROOT / 'python' / 'ui4me_core.egg-info'
        if not egg_info.exists():
            import shutil
            self.addCleanup(shutil.rmtree, egg_info, True)
        result = subprocess.run([sys.executable, 'setup.py', '-q', 'build_py', '--build-lib',
                                 str(self.folder / 'lib')], cwd=ROOT, env=env, capture_output=True,
                                text=True, timeout=300)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('development host', result.stderr)


if __name__ == '__main__':
    unittest.main()
