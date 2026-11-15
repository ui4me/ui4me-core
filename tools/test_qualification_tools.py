"""Qualification tooling (roadmap 6.0 / T14): probe runner, record comparison, windowed smoke guards.

The probes themselves need a real window (tools/windowed_smoke.py runs them under
a nested compositor; qualify_windows_rc.py on Windows); these tests cover the
runner's verdict logic with stand-in scripts.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
sys.path.insert(0, str(ROOT / 'tools' / 'release' / 'qualification'))
import qualify_windows_rc  # noqa: E402
import runner  # noqa: E402
import windowed_smoke  # noqa: E402


class RunnerTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.folder = Path(folder.name)

    def script(self, name: str, body: str) -> runner.Probe:
        (self.folder / name).write_text(textwrap.dedent(body), encoding='utf-8')
        return runner.Probe('demo', name, timeout=20)

    def run_demo(self, body: str, require_first_frame: bool = False, timeout: float = 20) -> dict:
        probe = self.script('demo.py', body)
        probe = runner.Probe(probe.name, probe.script, timeout=timeout)
        return runner.run_probe(sys.executable, self.folder, probe, runner.probe_environment(), require_first_frame)

    def test_pass_requires_result_line_and_zero_exit(self):
        row = self.run_demo("print('RESULT PASS demo ok')\n")
        self.assertEqual(row['result'], 'PASS', row)
        self.assertEqual(row['detail'], 'ok')
        self.assertEqual(self.run_demo("print('hello')\n")['result'], 'FAIL')
        self.assertEqual(self.run_demo("print('RESULT PASS demo')\nraise SystemExit(3)\n")['exit_code'], 3)
        self.assertEqual(self.run_demo("print('RESULT FAIL demo boom')\n")['result'], 'FAIL')
        # A RESULT line for another probe does not count.
        self.assertEqual(self.run_demo("print('RESULT PASS other')\n")['result'], 'FAIL')

    def test_host_abort_is_a_failure(self):
        row = self.run_demo("import sys\nsys.stderr.write('fatal runtime error: stack overflow, aborting\\n')\n"
                            "raise SystemExit(134)\n")
        self.assertEqual(row['result'], 'FAIL')
        self.assertIn('stack overflow', row['stderr_tail'])

    def test_windowed_run_requires_the_first_frame_record(self):
        body = "print('RESULT PASS demo')\n"
        self.assertIn('first frame', ' '.join(self.run_demo(body, require_first_frame=True)['reasons']))
        framed = "import sys\nsys.stderr.write('ui4me-host info: first frame presented (window=w1)\\n')\n" + body
        self.assertEqual(self.run_demo(framed, require_first_frame=True)['result'], 'PASS')

    def test_timeout_stops_the_whole_process_group(self):
        marker = self.folder / 'child.pid'
        body = f"""
        import subprocess, sys, time
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        open({str(marker)!r}, 'w').write(str(child.pid))
        time.sleep(60)
        """
        row = self.run_demo(body, timeout=2)
        self.assertEqual(row['exit_code'], 124)
        self.assertIn('timed out', row['reasons'][0])
        pid = int(marker.read_text())
        if os.name == 'nt':
            # os.kill(pid, 0) terminates the process on Windows; taskkill /T
            # already stopped the tree, so only the exit status is checked.
            return
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            except PermissionError:
                break
            time.sleep(0.1)
        else:
            self.fail('the probe child survived the timeout')

    def test_environment_drops_ui4me_and_pythonpath_overrides(self):
        env = runner.probe_environment({'UI4ME_HOST_BINARY': 'x', 'PYTHONPATH': 'y', 'PATH': 'p'})
        self.assertEqual(env['PATH'], 'p')
        self.assertNotIn('PYTHONPATH', env)
        self.assertNotIn('UI4ME_HOST_BINARY', env)
        self.assertEqual(env['UI4ME_VERBOSE'], '1')

    def test_standard_set_matches_the_roadmap(self):
        probes = {p.name: p.args for p in runner.standard_probes()}
        self.assertEqual(probes['depth'], ('250',))
        self.assertEqual(probes['svgdepth'], ('700',))
        self.assertEqual(probes['burst'][0], '100')
        self.assertIn('first-frame', probes)
        staged = runner.stage_probes(self.folder / 'staged')
        self.assertEqual(sorted(p.name for p in staged.iterdir()), sorted(runner.PROBE_FILES))

    def test_probe_scripts_carry_no_lab_or_machine_paths(self):
        for path in (ROOT / 'tools/release/qualification').glob('*.py'):
            text = path.read_text(encoding='utf-8')
            for forbidden in ('/home/', 'C:\\\\', 'F:\\\\', 'exp/', 'winDEV', 'pvez', 'pvey', 'scratchpad'):
                self.assertNotIn(forbidden, text, f'{path.name} contains {forbidden!r}')


class RecordComparisonTests(unittest.TestCase):
    def record(self, folder: Path, result: str = 'PASS', digest: str = 'a' * 64) -> Path:
        path = folder / 'record.json'
        path.write_text(json.dumps({'result': result, 'wheel': {'filename': 'w.whl', 'sha256': digest}}))
        return path

    def test_compare_against_sha256sums(self):
        with tempfile.TemporaryDirectory() as name:
            folder = Path(name)
            sums = folder / 'SHA256SUMS'
            sums.write_text(f"{'a' * 64}  w.whl\n{'b' * 64}  other.tar.gz\n", encoding='ascii')
            hashes = qualify_windows_rc.published_hashes(sums, None)
            self.assertTrue(qualify_windows_rc.compare(self.record(folder), hashes)[0])
            self.assertFalse(qualify_windows_rc.compare(self.record(folder, digest='c' * 64), hashes)[0])
            self.assertFalse(qualify_windows_rc.compare(self.record(folder, result='FAIL'), hashes)[0])
            self.assertFalse(qualify_windows_rc.compare(self.record(folder), {'x.whl': 'a' * 64})[0])

    def test_qualification_refuses_non_windows_without_opt_in(self):
        if sys.platform == 'win32':
            self.skipTest('Windows')
        with self.assertRaises(SystemExit):
            qualify_windows_rc.qualify(Path('missing.whl'), sys.executable, [], allow_non_windows=False)


class WindowedSmokeGuardTests(unittest.TestCase):
    def test_existing_compositor_must_not_be_the_session_display(self):
        if sys.platform != 'linux':
            self.skipTest('Linux only')
        old = os.environ.get('WAYLAND_DISPLAY')
        os.environ['WAYLAND_DISPLAY'] = 'wayland-0'
        try:
            with self.assertRaises(SystemExit) as caught:
                windowed_smoke.main(['--compositor', 'existing', '--socket', 'wayland-0',
                                     '--host', sys.executable])
            self.assertEqual(caught.exception.code, 2)
            with self.assertRaises(SystemExit):
                windowed_smoke.main(['--compositor', 'existing', '--host', sys.executable])
        finally:
            if old is None:
                os.environ.pop('WAYLAND_DISPLAY', None)
            else:
                os.environ['WAYLAND_DISPLAY'] = old

    def test_private_compositor_commands(self):
        weston = windowed_smoke.compositor_command('weston', 's1', 800, 600)
        self.assertEqual(weston[:2], ['weston', '--backend=headless'])
        self.assertIn('--socket=s1', weston)
        kwin = windowed_smoke.compositor_command('kwin', 's2', 800, 600)
        self.assertEqual(kwin[:2], ['kwin_wayland', '--virtual'])
        self.assertIn('s2', kwin)


if __name__ == '__main__':
    unittest.main()
