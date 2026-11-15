"""Checks on a packaged native host binary that do not need to run it.

* ``debug_build_markers``: refuses a development (debug-profile) host in a
  release artifact (RELEASE-17). The developer guide exports
  ``UI4ME_HOST_BINARY=target/debug/ui4me-host`` and a 345 MB debug host
  otherwise passes every structural check. Markers, any one of which refuses:
  ``.debug_*`` ELF sections (the release profile strips debuginfo), Rust's
  ``unsafe precondition(s) violated`` debug-assertion messages (absent from
  release builds on both platforms, thousands of them in a debug build), and
  a size above ``MAX_RELEASE_HOST_BYTES``.
* ``pe_stack_reserve``: the main-thread stack the Windows loader reserves
  (``SizeOfStackReserve`` in the PE32+ optional header). F-01: the default
  1 MiB aborted content at nesting depth ~40; the release host is linked with
  ``/STACK:`` ``WINDOWS_HOST_STACK_RESERVE``.
"""
from __future__ import annotations

import struct

# The one place the expected Windows main-thread stack reserve is defined
# (32 MiB). Must equal the /STACK: link argument of crates/ui4me-host
# (checked by tools/test_release_artifacts.py once the build sets it).
WINDOWS_HOST_STACK_RESERVE = 33_554_432
# Release hosts are ~30 MB (Windows) and ~41 MB (Linux) in the first releases; the Linux
# debug host is ~345 MB.
MAX_RELEASE_HOST_BYTES = 150 * 1024 * 1024
DEBUG_ASSERTION_MARKER = b'unsafe precondition(s) violated'


def elf_section_names(data: bytes) -> list[str]:
    if data[:4] != b'\x7fELF' or data[4] != 2 or data[5] != 1:
        raise ValueError('Not an ELF64 little-endian image')
    shoff = struct.unpack_from('<Q', data, 0x28)[0]
    shentsize, shnum, shstrndx = struct.unpack_from('<HHH', data, 0x3A)
    if not shoff or not shnum or shentsize < 64 or shstrndx >= shnum:
        return []
    if shoff + shnum * shentsize > len(data):
        raise ValueError('Truncated ELF section header table')
    strtab = shoff + shstrndx * shentsize
    str_offset, str_size = struct.unpack_from('<QQ', data, strtab + 0x18)
    names = data[str_offset:str_offset + str_size]
    result = []
    for index in range(shnum):
        name_offset = struct.unpack_from('<I', data, shoff + index * shentsize)[0]
        end = names.find(b'\0', name_offset)
        result.append(names[name_offset:end if end >= 0 else None].decode('ascii', 'replace'))
    return result


def debug_build_markers(data: bytes) -> list[str]:
    """Reasons this host looks like a development build (empty for a release host)."""
    reasons = []
    if len(data) > MAX_RELEASE_HOST_BYTES:
        reasons.append(f'{len(data)} bytes exceeds the release-host bound {MAX_RELEASE_HOST_BYTES}')
    if data[:4] == b'\x7fELF':
        debug_sections = sorted(n for n in elf_section_names(data) if n.startswith(('.debug_', '.zdebug_')))
        if debug_sections:
            reasons.append('contains debug sections ' + ', '.join(debug_sections[:4]))
    if DEBUG_ASSERTION_MARKER in data:
        reasons.append('contains Rust debug-assertion checks (built without the release profile)')
    return reasons


def pe_stack_reserve(data: bytes) -> int:
    if data[:2] != b'MZ':
        raise ValueError('Not a PE image')
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    if data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Missing PE signature')
    optional = pe + 24
    magic = struct.unpack_from('<H', data, optional)[0]
    if magic == 0x20B:
        return struct.unpack_from('<Q', data, optional + 72)[0]
    if magic == 0x10B:
        return struct.unpack_from('<I', data, optional + 72)[0]
    raise ValueError(f'Unknown PE optional header magic {magic:#x}')
