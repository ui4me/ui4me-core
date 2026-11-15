"""Run the qualification probes as separate processes and collect exact results.

Used by ``tools/release/qualify_windows_rc.py`` (installed release-candidate
wheel on Windows) and ``tools/windowed_smoke.py`` (source checkout under a
nested Wayland compositor). Nothing here imports ``ui4me_core``.
"""
from __future__ import annotations

from dataclasses import dataclass
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
PROBE_FILES = ('_probe.py', 'first_frame.py', 'depth.py', 'svgdepth.py', 'burst.py')
FIRST_FRAME = 'first frame presented'
RESULT = re.compile(r'^RESULT (PASS|FAIL) (\S+)(.*)$', re.M)


@dataclass(frozen=True)
class Probe:
    name: str
    script: str
    args: tuple[str, ...] = ()
    timeout: float = 90.0


def standard_probes(depth: int = 250, svg_depth: int = 700, burst: int = 100, burst_rounds: int = 6) -> list[Probe]:
    """The roadmap 6.0 set: depth.py 250, svgdepth.py 700, burst2.py 100, plus a first-frame check."""
    return [
        Probe('first-frame', 'first_frame.py'),
        Probe('depth', 'depth.py', (str(depth),)),
        Probe('svgdepth', 'svgdepth.py', (str(svg_depth),)),
        Probe('burst', 'burst.py', (str(burst), str(burst_rounds))),
    ]


def stage_probes(destination: Path) -> Path:
    """Copy the probe scripts to a private folder (no checkout on sys.path)."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in PROBE_FILES:
        shutil.copy2(HERE / name, destination / name)
    return destination


def tail(text: str, lines: int = 25) -> str:
    return '\n'.join(text.splitlines()[-lines:])


def stop_tree(process: subprocess.Popen) -> None:
    if os.name == 'nt':
        subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], capture_output=True, check=False)
    else:
        import signal
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()


def run_probe(python: str, folder: Path, probe: Probe, env: dict[str, str],
              require_first_frame: bool) -> dict:
    argv = [python, probe.script, *probe.args]
    started = time.monotonic()
    # Own process group: on POSIX the launcher execs the host, which starts
    # the backend; a timeout must stop all of them, not only the first pid.
    process = subprocess.Popen(argv, cwd=folder, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, encoding='utf-8', errors='replace',
                               start_new_session=os.name != 'nt',
                               creationflags=getattr(subprocess, 'CREATE_NEW_PROCESS_GROUP', 0))
    try:
        out, err = process.communicate(timeout=probe.timeout)
        code, timed_out = process.returncode, False
    except subprocess.TimeoutExpired:
        stop_tree(process)
        out, err = process.communicate(timeout=30)
        code, timed_out = 124, True
    results = [m for m in RESULT.finditer(out) if m.group(2) == probe.name]
    verdict = results[-1].group(1) if results else None
    first_frame = FIRST_FRAME in err
    reasons = []
    if timed_out:
        reasons.append(f'timed out after {probe.timeout:g}s')
    if code != 0:
        reasons.append(f'exit status {code}')
    if verdict != 'PASS':
        reasons.append('no RESULT PASS line' if verdict is None else 'probe reported FAIL')
    if require_first_frame and not first_frame:
        reasons.append(f'host never logged "{FIRST_FRAME}" (no window was presented)')
    return {
        'probe': probe.name, 'argv': [Path(argv[0]).name, *argv[1:]], 'exit_code': code,
        'seconds': round(time.monotonic() - started, 2), 'result': 'PASS' if not reasons else 'FAIL',
        'reasons': reasons, 'detail': results[-1].group(3).strip() if results else '',
        'first_frame_logged': first_frame, 'stdout_tail': tail(out), 'stderr_tail': tail(err),
    }


def probe_environment(base: dict[str, str] | None = None, **extra: str) -> dict[str, str]:
    env = dict(os.environ if base is None else base)
    for key in [k for k in env if k.startswith('UI4ME_') or k in ('PYTHONPATH', 'PYTHONHOME')]:
        env.pop(key)
    env['UI4ME_VERBOSE'] = '1'          # host records (first frame) on stderr
    env['PYTHONIOENCODING'] = 'utf-8'
    env.update(extra)
    return env


def run_all(python: str, folder: Path, probes: list[Probe], env: dict[str, str],
            require_first_frame: bool, echo=print) -> list[dict]:
    rows = []
    for probe in probes:
        row = run_probe(python, folder, probe, env, require_first_frame)
        rows.append(row)
        echo(f"{row['result']:4} {probe.name:12} {row['seconds']:7.2f}s "
             + (row['detail'] if row['result'] == 'PASS' else '; '.join(row['reasons'])))
        if row['result'] != 'PASS':
            echo(row['stderr_tail'])
    return rows


if __name__ == '__main__':
    print('Use tools/release/qualify_windows_rc.py or tools/windowed_smoke.py', file=sys.stderr)
    raise SystemExit(2)
