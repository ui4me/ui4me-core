#!/usr/bin/env python3
"""Windowed GUI smoke test under a private nested Wayland compositor (T14 / R4b first step).

Opens real native windows (not the host's headless test mode), waits for the
host to present its first frame, and runs the burst (F-02), depth (F-01) and
image-SVG depth (F-03) probes from ``tools/release/qualification``. Each probe
must exit 0, print ``RESULT PASS`` and log ``first frame presented``.

It never uses the desktop you are logged into. By default it starts its own
compositor on a unique socket and stops it afterwards:

    python3 tools/windowed_smoke.py                       # weston --backend=headless
    python3 tools/windowed_smoke.py --compositor kwin     # kwin_wayland --virtual
    dbus-run-session -- python3 tools/windowed_smoke.py   # also a private session bus (CI)

``--compositor existing --socket NAME`` reuses a nested compositor you started
yourself (for example ``kwin_wayland --virtual --socket ui4me-wsf-0``); the
socket must be given explicitly and must not be your session's display.

The host is ``target/debug/ui4me-host`` (or ``--host``); the Python package is
the checkout's ``python/``. Results are printed and written as JSON
(``--output``). Exit status: 0 all passed, 1 a probe failed, 2 setup error.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release' / 'qualification'))
import runner  # noqa: E402


def default_host() -> Path:
    name = 'ui4me-host.exe' if os.name == 'nt' else 'ui4me-host'
    for profile in ('release', 'debug'):
        candidate = ROOT / 'target' / profile / name
        if candidate.is_file():
            return candidate
    return ROOT / 'target' / 'debug' / name


def compositor_command(kind: str, socket: str, width: int, height: int) -> list[str]:
    if kind == 'weston':
        return ['weston', '--backend=headless', '--renderer=pixman', f'--socket={socket}',
                f'--width={width}', f'--height={height}', '--idle-time=0', '--no-config']
    if kind == 'kwin':
        return ['kwin_wayland', '--virtual', '--socket', socket, '--width', str(width),
                '--height', str(height), '--no-lockscreen']
    raise ValueError(kind)


class Compositor:
    """A private compositor process on its own socket, stopped on exit."""

    def __init__(self, kind: str, socket: str, width: int, height: int, log: Path):
        self.kind, self.socket, self.log = kind, socket, log
        self.command = compositor_command(kind, socket, width, height)
        self.process: subprocess.Popen | None = None

    def __enter__(self):
        if not shutil.which(self.command[0]):
            raise RuntimeError(f'{self.command[0]} is not installed')
        env = {k: v for k, v in os.environ.items() if k not in ('WAYLAND_DISPLAY', 'DISPLAY', 'WAYLAND_SOCKET')}
        runtime = Path(env['XDG_RUNTIME_DIR'])
        self.socket_path = runtime / self.socket
        if self.socket_path.exists():
            raise RuntimeError(f'Wayland socket {self.socket_path} already exists; choose another --socket')
        self.log_file = self.log.open('wb')
        self.process = subprocess.Popen(self.command, env=env, stdout=self.log_file, stderr=subprocess.STDOUT,
                                        start_new_session=True)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f'{self.kind} exited with {self.process.returncode}; see {self.log}')
            if self.socket_path.exists():
                time.sleep(0.5)   # let the compositor finish initialising its globals
                return self
            time.sleep(0.1)
        raise RuntimeError(f'{self.kind} did not create {self.socket_path} within 20 s; see {self.log}')

    def __exit__(self, *exc):
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if getattr(self, 'log_file', None):
            self.log_file.close()
        return False


def ensure_runtime_dir(scratch: Path) -> None:
    current = os.environ.get('XDG_RUNTIME_DIR')
    if current and Path(current).is_dir():
        return
    private = scratch / 'runtime'
    private.mkdir(mode=0o700, exist_ok=True)
    os.environ['XDG_RUNTIME_DIR'] = str(private)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('--compositor', choices=('weston', 'kwin', 'existing'), default='weston')
    parser.add_argument('--socket', help='Wayland socket name (default: a unique ui4me-smoke-<id>)')
    parser.add_argument('--host', type=Path, default=None)
    parser.add_argument('--python', default=sys.executable)
    parser.add_argument('--depth', type=int, default=250)
    parser.add_argument('--svg-depth', type=int, default=700)
    parser.add_argument('--burst', type=int, default=100)
    parser.add_argument('--burst-rounds', type=int, default=6)
    parser.add_argument('--width', type=int, default=1280)
    parser.add_argument('--height', type=int, default=900)
    parser.add_argument('--output', type=Path, default=ROOT / 'verification' / 'windowed-smoke')
    args = parser.parse_args(argv)
    if sys.platform != 'linux':
        parser.exit(2, 'windowed_smoke.py drives a nested Wayland compositor and runs on Linux only\n')
    host = (args.host or default_host()).resolve()
    if not host.is_file():
        parser.exit(2, f'No host at {host}; run cargo build -p ui4me-host or pass --host\n')
    if args.compositor == 'existing':
        if not args.socket:
            parser.exit(2, '--compositor existing needs an explicit --socket of a nested compositor\n')
        if args.socket == os.environ.get('WAYLAND_DISPLAY'):
            parser.exit(2, f'Refusing to use {args.socket}: it is this session\'s WAYLAND_DISPLAY. '
                           'Start a nested compositor on its own socket.\n')
    socket = args.socket or f'ui4me-smoke-{uuid.uuid4().hex[:8]}'
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='ui4me-windowed-smoke-') as scratch_name:
        scratch = Path(scratch_name)
        ensure_runtime_dir(scratch)
        folder = runner.stage_probes(scratch / 'probes')
        env = runner.probe_environment(
            None, UI4ME_HOST_BINARY=str(host), PYTHONPATH=str(ROOT / 'python'),
            WAYLAND_DISPLAY=socket, XDG_RUNTIME_DIR=os.environ['XDG_RUNTIME_DIR'])
        env.pop('DISPLAY', None)
        probes = runner.standard_probes(args.depth, args.svg_depth, args.burst, args.burst_rounds)
        started = datetime.now(timezone.utc).isoformat()
        try:
            if args.compositor == 'existing':
                if not (Path(os.environ['XDG_RUNTIME_DIR']) / socket).exists():
                    parser.exit(2, f'No Wayland socket {socket} in XDG_RUNTIME_DIR\n')
                rows = runner.run_all(args.python, folder, probes, env, require_first_frame=True)
            else:
                with Compositor(args.compositor, socket, args.width, args.height,
                                output / f'{args.compositor}.log'):
                    rows = runner.run_all(args.python, folder, probes, env, require_first_frame=True)
        except RuntimeError as error:
            print(f'windowed smoke: setup failed: {error}', file=sys.stderr)
            return 2
    passed = all(row['result'] == 'PASS' for row in rows)
    report = {'schema': 1, 'kind': 'windowed-smoke', 'started_utc': started,
              'compositor': args.compositor, 'socket': socket, 'host': str(host),
              'windowed': True, 'probes': rows, 'result': 'PASS' if passed else 'FAIL'}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f"windowed smoke: {report['result']} ({sum(r['result'] == 'PASS' for r in rows)}/{len(rows)} probes) "
          f"-> {output / 'report.json'}")
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
