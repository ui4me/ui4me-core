#!/usr/bin/env python3
"""Qualify the exact Windows release-candidate wheel and record its hash (roadmap 6.0, F-11 interim).

Run on a Windows machine **in an interactive desktop session** (the probes open
real windows; an SSH session without a desktop cannot present frames):

    py -3.12 tools\\release\\qualify_windows_rc.py --wheel ui4me_core-X.Y.Z-py3-none-win_amd64.whl ^
        --output qualification-windows-X.Y.Z.json

It:

1. hashes the wheel and runs ``verify_windows_artifact.validate_wheel`` on it
   (PE32+, console subsystem, ``SizeOfStackReserve`` = 33554432, no debug host);
2. installs exactly that wheel into a fresh virtual environment
   (``pip install --no-index --no-deps``), outside the checkout;
3. runs the probes from ``tools/release/qualification`` against the installed
   package: first frame, ``depth.py 250``, ``svgdepth.py 700`` and
   ``burst.py 100 6`` (the audit's ``burst2.py``);
4. writes a JSON record with the wheel's SHA-256, the embedded host's SHA-256,
   the stack reserve and every probe's argv, exit status and output tail.

Afterwards compare the record with what is (or is about to be) published:

    python tools/release/qualify_windows_rc.py --compare qualification-windows-X.Y.Z.json --sha256sums SHA256SUMS
    python tools/release/qualify_windows_rc.py --compare qualification-windows-X.Y.Z.json --pypi X.Y.Z

and pass the recorded wheel hash to ``publish.yaml`` as
``expected_windows_wheel_sha256`` together with ``candidate_run_id``; the
workflow refuses to publish any other Windows wheel. ``--allow-non-windows``
runs steps 1-4 on Linux with a Linux wheel (for testing the tool itself; the
record then says so and is not a Windows qualification).
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / 'qualification'))
import runner  # noqa: E402
from host_checks import WINDOWS_HOST_STACK_RESERVE  # noqa: E402

SCHEMA = 1


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def validate(wheel: Path) -> dict:
    if wheel.name.endswith('-win_amd64.whl'):
        from verify_windows_artifact import validate_wheel
    else:
        from verify_linux_artifact import validate_wheel
    return validate_wheel(wheel)


def wheel_version(wheel: Path) -> str:
    with zipfile.ZipFile(wheel) as archive:
        metadata = [n for n in archive.namelist() if n.endswith('.dist-info/METADATA')]
        if len(metadata) != 1:
            raise ValueError('Wheel has no unique METADATA')
        for line in archive.read(metadata[0]).decode('utf-8').splitlines():
            if line.startswith('Version: '):
                return line[9:].strip()
    raise ValueError('Wheel METADATA has no Version')


def venv_python(venv: Path) -> Path:
    return venv / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')


def qualify(wheel: Path, python: str, probes: list[runner.Probe], allow_non_windows: bool, echo=print) -> dict:
    windows = sys.platform == 'win32'
    if not windows and not allow_non_windows:
        raise SystemExit('Run this on Windows in an interactive desktop session (or pass --allow-non-windows '
                         'to exercise the tool with a Linux wheel)')
    wheel = wheel.resolve(strict=True)
    record = {
        'schema': SCHEMA, 'kind': 'windows-rc-qualification' if windows else 'non-windows-tool-check',
        'created_utc': datetime.now(timezone.utc).isoformat(),
        'machine': {'platform': platform.platform(), 'python': platform.python_version(),
                    'session': os.environ.get('SESSIONNAME', ''), 'interactive_desktop_required': True},
        'wheel': {'filename': wheel.name, 'sha256': sha256(wheel), 'bytes': wheel.stat().st_size,
                  'version': wheel_version(wheel)},
    }
    structural = validate(wheel)
    record['wheel']['host_sha256'] = structural.get('host_sha256')
    record['wheel']['structural'] = structural['result']
    if windows or wheel.name.endswith('-win_amd64.whl'):
        record['wheel']['stack_reserve'] = structural['pe']['stack_reserve']
        record['wheel']['expected_stack_reserve'] = WINDOWS_HOST_STACK_RESERVE
    echo(f"wheel {wheel.name} sha256 {record['wheel']['sha256']} structural {structural['result']}")
    with tempfile.TemporaryDirectory(prefix='ui4me-rc-') as scratch_name:
        scratch = Path(scratch_name)
        venv = scratch / 'venv'
        subprocess.run([python, '-m', 'venv', str(venv)], check=True)
        python_in_venv = str(venv_python(venv))
        subprocess.run([python_in_venv, '-m', 'pip', 'install', '--quiet', '--no-index', '--no-deps', str(wheel)],
                       check=True)
        installed = subprocess.run([python_in_venv, '-c', 'import ui4me_core,sys;print(ui4me_core.__version__)'],
                                   cwd=scratch, capture_output=True, text=True, check=True).stdout.strip()
        if installed != record['wheel']['version']:
            raise SystemExit(f'Installed version {installed} differs from the wheel version')
        folder = runner.stage_probes(scratch / 'probes')
        env = runner.probe_environment()
        rows = runner.run_all(python_in_venv, folder, probes, env, require_first_frame=True, echo=echo)
    record['probes'] = rows
    record['result'] = 'PASS' if all(r['result'] == 'PASS' for r in rows) else 'FAIL'
    if not windows:
        record['note'] = 'Not a Windows qualification: tool self-check on ' + sys.platform
    return record


def published_hashes(sha256sums: Path | None, pypi_version: str | None) -> dict[str, str]:
    hashes: dict[str, str] = {}
    if sha256sums:
        for line in sha256sums.read_text(encoding='ascii').splitlines():
            if line.strip():
                digest, name = line.split(None, 1)
                hashes[name.lstrip('*').strip()] = digest.lower()
    if pypi_version:
        url = f'https://pypi.org/pypi/ui4me-core/{pypi_version}/json'
        with urllib.request.urlopen(url, timeout=30) as response:
            for item in json.load(response)['urls']:
                hashes[item['filename']] = item['digests']['sha256']
    return hashes


def compare(record_path: Path, hashes: dict[str, str]) -> tuple[bool, str]:
    record = json.loads(record_path.read_text(encoding='utf-8'))
    name, recorded = record['wheel']['filename'], record['wheel']['sha256']
    if record.get('result') != 'PASS':
        return False, f'{record_path} records a FAILED qualification'
    if name not in hashes:
        return False, f'{name} is not in the published set ({sorted(hashes)})'
    if hashes[name] != recorded:
        return False, f'{name}: published {hashes[name]} != qualified {recorded}'
    return True, f'{name}: published hash equals the qualified hash {recorded}'


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('--wheel', type=Path)
    parser.add_argument('--python', default=sys.executable, help='interpreter used to create the venv')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--depth', type=int, default=250)
    parser.add_argument('--svg-depth', type=int, default=700)
    parser.add_argument('--burst', type=int, default=100)
    parser.add_argument('--burst-rounds', type=int, default=6)
    parser.add_argument('--allow-non-windows', action='store_true')
    parser.add_argument('--compare', type=Path, metavar='RECORD')
    parser.add_argument('--sha256sums', type=Path)
    parser.add_argument('--pypi', metavar='VERSION')
    args = parser.parse_args(argv)
    if args.compare:
        if not (args.sha256sums or args.pypi):
            parser.error('--compare needs --sha256sums and/or --pypi')
        ok, message = compare(args.compare, published_hashes(args.sha256sums, args.pypi))
        print(('MATCH ' if ok else 'MISMATCH ') + message)
        return 0 if ok else 1
    if not args.wheel:
        parser.error('--wheel is required (or --compare)')
    probes = runner.standard_probes(args.depth, args.svg_depth, args.burst, args.burst_rounds)
    record = qualify(args.wheel, args.python, probes, args.allow_non_windows)
    text = json.dumps(record, indent=2) + '\n'
    if args.output:
        args.output.write_text(text, encoding='utf-8')
    print(f"qualification {record['result']}: {record['wheel']['filename']} sha256 {record['wheel']['sha256']}"
          + (f' -> {args.output}' if args.output else ''))
    if not args.output:
        print(text)
    return 0 if record['result'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
