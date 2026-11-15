"""Direct `python app.py` startup: launcher/backend roles, manifests and lifecycle.

The integration tests run real scripts through `Application.start()` and the
real native host in its documented headless test mode (UI4ME_TEST_MODE=1).
They need a built host (`cargo build -p ui4me-host`) and are skipped without one.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(ROOT / "tools"))
from ui4me_core import Application, StartupError, _bootstrap  # noqa: E402
from run_example import checkout_environment  # noqa: E402

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 (supported): tomllib is 3.11+
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        tomllib = None


def built_host() -> str | None:
    name = "ui4me-host.exe" if os.name == "nt" else "ui4me-host"
    for mode in ("release", "debug"):
        candidate = ROOT / "target" / mode / name
        if candidate.is_file():
            return str(candidate)
    return None


CLEAN = {key: value for key, value in os.environ.items() if not key.startswith("UI4ME_")}


class RoleTests(unittest.TestCase):
    def test_launcher_backend_and_invalid_environments(self):
        with mock.patch.dict(os.environ, CLEAN, clear=True):
            self.assertFalse(_bootstrap.managed())
        with mock.patch.dict(os.environ, {**CLEAN, "UI4ME_ENDPOINT": "e", "UI4ME_TOKEN": "t"}, clear=True):
            self.assertTrue(_bootstrap.managed())
        for partial in ({"UI4ME_ENDPOINT": "e"}, {"UI4ME_TOKEN": "t"}, {"UI4ME_DIRECT_START": "42"}):
            with self.subTest(partial=partial), mock.patch.dict(os.environ, {**CLEAN, **partial}, clear=True):
                with self.assertRaises(StartupError):
                    _bootstrap.managed()

    def test_connection_secret_is_removed_from_the_backend_environment(self):
        values = {"UI4ME_ENDPOINT": "e", "UI4ME_TOKEN": "t", "UI4ME_APP_ID": "a.b",
                  "UI4ME_PROTOCOL_VERSION": "1", "UI4ME_DIRECT_START": "1", "UI4ME_VERBOSE": "1"}
        with mock.patch.dict(os.environ, {**CLEAN, **values}, clear=True):
            _bootstrap.forget_host_environment()
            self.assertEqual({k for k in os.environ if k.startswith("UI4ME_")}, {"UI4ME_VERBOSE"})

    def test_managed_start_connects_without_launching(self):
        self.addCleanup(setattr, _bootstrap, "_started", False)
        sentinel = object()
        with mock.patch.dict(os.environ, {**CLEAN, "UI4ME_ENDPOINT": "e", "UI4ME_TOKEN": "t"}, clear=True), \
                mock.patch.object(Application, "from_environment", return_value=sentinel) as connect, \
                mock.patch.object(_bootstrap, "launch") as launch:
            self.assertIs(Application.start(name="Managed"), sentinel)
            self.assertNotIn("UI4ME_TOKEN", os.environ)
        connect.assert_called_once_with()
        launch.assert_not_called()

    def test_options_are_validated_in_both_roles(self):
        for options in ({"instance": "many"}, {"app_id": "noperiod"}, {"app_id": "org.1bad"},
                        {"name": ""}, {"name": "tab\tname"}, {"max_windows": 0},
                        {"startup_timeout_ms": 10}, {"shutdown_timeout_ms": True}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                Application.start(**options)

    def test_second_start_in_one_process_is_refused(self):
        self.addCleanup(setattr, _bootstrap, "_started", False)
        with mock.patch.dict(os.environ, {**CLEAN, "UI4ME_ENDPOINT": "e", "UI4ME_TOKEN": "t"}, clear=True), \
                mock.patch.object(Application, "from_environment", return_value=object()), \
                mock.patch.object(_bootstrap, "launch") as launch:
            Application.start(name="First")
            # The first call removed the environment: without the guard this
            # second call would launch another host from inside the backend.
            with self.assertRaisesRegex(StartupError, "already called"):
                Application.start(name="Second")
        launch.assert_not_called()

    def test_host_killed_by_a_signal_maps_to_the_shell_status(self):
        with mock.patch("sys.stderr") as stderr:
            self.assertEqual(_bootstrap._exit_status(-9), 137)
            self.assertEqual(_bootstrap._exit_status(-6), 134)
        self.assertIn("SIGKILL", "".join(str(c) for c in stderr.mock_calls))
        self.assertEqual(_bootstrap._exit_status(3), 3)

    def test_from_environment_explains_direct_start(self):
        with mock.patch.dict(os.environ, CLEAN, clear=True), \
                self.assertRaisesRegex(StartupError, "Application.start"):
            Application.from_environment()


@unittest.skipUnless(os.name == "posix", "POSIX signals; Windows console events are verified on the lab VM")
class LauncherInterruptTests(unittest.TestCase):
    """The launcher waits for the host's own orderly shutdown on Ctrl+C/SIGTERM."""

    HOST = textwrap.dedent('''\
        import signal, sys, time
        def stop(signum, frame):
            time.sleep(0.4)          # an orderly shutdown takes a moment
            sys.stderr.write("host shut down\\n"); sys.stderr.flush()
            sys.exit(128 + signum)
        signal.signal(signal.SIGINT, stop)
        signal.signal(signal.SIGTERM, stop)
        sys.stderr.write("host ready\\n"); sys.stderr.flush()
        time.sleep(30)
        ''')

    def interrupt_after_ready(self, host_code, signum, **options):
        import signal, threading
        command = [sys.executable, "-c", host_code]
        result = {}

        def run():
            with mock.patch("sys.stderr") as stderr:
                result["status"] = _bootstrap.run_host(command, env=dict(os.environ), **options)
            result["stderr"] = "".join(str(c) for c in stderr.mock_calls)
        # run_host installs signal handlers, so it must run in the main thread;
        # a helper thread sends the signal to this process once the host is up.
        def send():
            time.sleep(1.0)
            os.kill(os.getpid(), signum)
        threading.Thread(target=send, daemon=True).start()
        started = time.monotonic()
        run()
        return result, time.monotonic() - started

    def test_interrupt_waits_for_the_host_and_returns_its_status(self):
        import signal
        result, elapsed = self.interrupt_after_ready(self.HOST, signal.SIGINT)
        self.assertEqual(result["status"], 130)
        self.assertIn("host shut down", result["stderr"])  # the host finished its shutdown
        self.assertNotIn("terminated by", result["stderr"])
        self.assertLess(elapsed, 10)

    def test_sigterm_is_forwarded(self):
        import signal
        result, _ = self.interrupt_after_ready(self.HOST, signal.SIGTERM)
        self.assertEqual(result["status"], 143)

    def test_a_host_that_ignores_the_interrupt_is_stopped_after_the_deadline(self):
        import signal
        stubborn = "import signal, time\nsignal.signal(signal.SIGINT, signal.SIG_IGN)\ntime.sleep(60)\n"
        result, elapsed = self.interrupt_after_ready(stubborn, signal.SIGINT, shutdown_timeout_ms=100)
        self.assertEqual(result["status"], 130)
        self.assertLess(elapsed, 15)


class ManifestTests(unittest.TestCase):
    def test_default_identity_is_valid_stable_and_distinct(self):
        first = _bootstrap.default_app_id("script:/home/a/app.py", "app")
        self.assertEqual(first, _bootstrap.default_app_id("script:/home/a/app.py", "app"))
        self.assertNotEqual(first, _bootstrap.default_app_id("script:/home/b/app.py", "app"))
        for label in ("app", "Café Ünïcode", "日本語", "123 start", "x" * 300, ""):
            with self.subTest(label=label):
                value = _bootstrap.default_app_id("script:" + label, label)
                self.assertTrue(_bootstrap.valid_app_id(value), value)

    def test_manifest_round_trips_unicode_quotes_and_backslashes(self):
        arguments = ["-X", "utf8", r"C:\My Apps\it's\app.py", 'quote " here', "Příliš 😀", "--flag"]
        with tempfile.TemporaryDirectory() as folder:
            path = _bootstrap.write_manifest(
                Path(folder), program=r"C:\Users\Zoë\.venv\Scripts\python.exe", arguments=arguments,
                cwd="/tmp/a b", app_id="org.example.App", name='Say "hi" — café', version="2.0",
                icon=None, instance="multiple", startup_timeout_ms=30000, shutdown_timeout_ms=5000,
                max_windows=4, env={"PYTHONUNBUFFERED": "1"})
            if tomllib is None:
                self.skipTest("needs tomllib (Python 3.11+) or tomli to parse TOML")
            manifest = tomllib.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(manifest["backend"]["args"], arguments)
        self.assertEqual(manifest["backend"]["program"], r"C:\Users\Zoë\.venv\Scripts\python.exe")
        self.assertEqual(manifest["app"]["name"], 'Say "hi" — café')
        self.assertEqual(manifest["backend"]["env"], {"PYTHONUNBUFFERED": "1"})
        self.assertEqual(manifest["host"]["instance"], "multiple")

    def test_non_unicode_arguments_are_refused_clearly(self):
        with tempfile.TemporaryDirectory() as folder, self.assertRaises(StartupError):
            _bootstrap.write_manifest(
                Path(folder), program="python", arguments=["bad\udcff"], cwd=".", app_id="a.b",
                name="n", version=None, icon=None, instance="single", startup_timeout_ms=30000,
                shutdown_timeout_ms=5000, max_windows=4, env={})

    def test_script_command_preserves_interpreter_options_and_arguments(self):
        main = type(sys)("__main__")
        main.__file__ = "/work/app.py"
        main.__spec__ = None
        with mock.patch.object(sys, "orig_argv", ["python3", "-X", "utf8", "app.py", "a b", "-m"], create=True), \
                mock.patch.object(sys, "argv", ["app.py", "a b", "-m"]), \
                mock.patch.dict(sys.modules, {"__main__": main}):
            program, arguments, identity, label = _bootstrap.backend_command()
        self.assertEqual(program, sys.executable)
        self.assertEqual(arguments, ["-X", "utf8", os.path.abspath("/work/app.py"), "a b", "-m"])
        self.assertEqual(label, "app")
        self.assertTrue(identity.startswith("script:"))

    def test_module_command_keeps_the_module_form(self):
        main = type(sys)("__main__")
        main.__file__ = "/work/pkg/app.py"
        main.__spec__ = type("Spec", (), {"name": "pkg.app"})()
        with mock.patch.object(sys, "orig_argv", ["python", "-m", "pkg.app", "--x"], create=True), \
                mock.patch.object(sys, "argv", ["/work/pkg/app.py", "--x"]), \
                mock.patch.dict(sys.modules, {"__main__": main}):
            _, arguments, identity, label = _bootstrap.backend_command()
        self.assertEqual(arguments, ["-m", "pkg.app", "--x"])
        self.assertEqual(label, "app")
        self.assertIn("pkg.app", identity)

    def test_interactive_and_command_strings_are_refused(self):
        main = type(sys)("__main__")
        main.__spec__ = None
        with mock.patch.object(sys, "argv", ["-c"]), mock.patch.dict(sys.modules, {"__main__": main}), \
                self.assertRaises(StartupError):
            _bootstrap.backend_command()


class RelayTests(unittest.TestCase):
    def relay(self, record, *, verbose=False):
        import io
        out, err = io.StringIO(), io.StringIO()
        raw = record if isinstance(record, bytes) else json.dumps(record).encode() + b"\n"
        _bootstrap.relay_line(raw, verbose=verbose, stdout=out, stderr=err)
        return out.getvalue(), err.getvalue()

    def test_backend_output_is_plain_text_on_the_matching_stream(self):
        record = {"level": "INFO", "fields": {"message": "backend output", "source": "backend",
                                              "stream": "stdout", "text": "Příliš 😀"}}
        self.assertEqual(self.relay(record), ("Příliš 😀\n", ""))
        record["fields"]["stream"] = "stderr"
        self.assertEqual(self.relay(record), ("", "Příliš 😀\n"))

    def test_host_records_are_summarized_by_level(self):
        info = {"level": "INFO", "fields": {"message": "window created", "id": 1}}
        self.assertEqual(self.relay(info), ("", ""))
        self.assertEqual(self.relay(info, verbose=True), ("", "ui4me-host info: window created (id=1)\n"))
        error = {"level": "ERROR", "fields": {"message": "application failed", "reason": "x"}}
        self.assertEqual(self.relay(error), ("", "ui4me-host error: application failed (reason=x)\n"))

    def test_other_output_passes_through(self):
        self.assertEqual(self.relay(b"ui4me-host: Invalid application manifest\n"),
                         ("", "ui4me-host: Invalid application manifest\n"))
        self.assertEqual(self.relay(b"\xff not utf-8\n"), ("", "\ufffd not utf-8\n"))


@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
class DirectStartIntegrationTests(unittest.TestCase):
    """Real scripts, real host (headless test mode), real IPC."""

    APP = textwrap.dedent('''\
        import os, sys, threading
        print("ROLE", "backend" if os.environ.get("UI4ME_ENDPOINT") else "launcher", flush=True)
        from ui4me_core import Application

        def main():
            with Application.start(name="Direct test", instance="multiple") as app:
                print("ARGV", repr(sys.argv[1:]))
                print("SECRET", "UI4ME_TOKEN" in os.environ)
                window = app.open_html("<p id='t'>Hello Příliš</p>")
                print("TEXT", window.document.get_text("t"))
                if "--crash" in sys.argv:
                    raise RuntimeError("deliberate failure")
                threading.Timer(0.2, app.request_quit).start()
                app.run()
                print("ORDERLY", file=sys.stderr)

        if __name__ == "__main__":
            main()
        ''')

    def run_app(self, *arguments, module=False, extra_env=None, timeout=90):
        temporary = tempfile.TemporaryDirectory(prefix="ui4me direct ")
        self.addCleanup(temporary.cleanup)
        folder = Path(temporary.name)
        app_dir = folder / "Příliš app"
        (app_dir / "pkg").mkdir(parents=True)
        (app_dir / "pkg" / "__init__.py").write_text("", encoding="utf-8")
        (app_dir / "pkg" / "main.py").write_text(self.APP, encoding="utf-8")
        (app_dir / "app.py").write_text(self.APP, encoding="utf-8")
        env = checkout_environment(CLEAN)
        env.update({"UI4ME_TEST_MODE": "1", "UI4ME_HOST_BINARY": built_host(), "PYTHONIOENCODING": "utf-8"})
        # A private temporary folder, so manifest cleanup is observable in isolation.
        self.temp = folder / "tmp"
        self.temp.mkdir()
        env.update({"TMPDIR": str(self.temp), "TEMP": str(self.temp), "TMP": str(self.temp)})
        if os.name != "nt":
            # Without XDG_RUNTIME_DIR the host falls back to a runtime folder
            # under TMPDIR, which is the folder this test inspects (RELEASE-16).
            runtime = folder / "runtime"
            runtime.mkdir(mode=0o700)
            env["XDG_RUNTIME_DIR"] = str(runtime)
        env.update(extra_env or {})
        target = ["-m", "pkg.main"] if module else ["app.py"]
        return subprocess.run([sys.executable, *target, *arguments], cwd=app_dir, env=env,
                              capture_output=True, timeout=timeout, check=False)

    def lines(self, data: bytes) -> list[str]:
        return data.decode("utf-8").splitlines()

    def test_script_runs_twice_preserves_arguments_and_exits_cleanly(self):
        arguments = ["a b", 'quote"d', "Ünï 😀", "--flag"]
        result = self.run_app(*arguments)
        out = self.lines(result.stdout)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(out[:2], ["ROLE launcher", "ROLE backend"])
        self.assertIn("ARGV " + repr(arguments), out)
        self.assertIn("SECRET False", out)
        self.assertIn("TEXT Hello Příliš", out)
        self.assertIn("ORDERLY", self.lines(result.stderr))
        # Host JSON diagnostics are not shown to the developer by default.
        self.assertNotIn('"level"', result.stderr.decode())

    def test_module_form_runs_through_the_host(self):
        result = self.run_app("--x", module=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("ARGV ['--x']", self.lines(result.stdout))

    def test_backend_failure_reports_traceback_and_nonzero_status(self):
        result = self.run_app("--crash")
        self.assertNotEqual(result.returncode, 0)
        error = result.stderr.decode()
        self.assertIn("RuntimeError: deliberate failure", error)
        self.assertEqual(error.count("RuntimeError: deliberate failure"), 1, error)
        self.assertIn("ui4me-host error: application failed", error)

    def test_verbose_mode_shows_host_records(self):
        result = self.run_app(extra_env={"UI4ME_VERBOSE": "1"})
        self.assertEqual(result.returncode, 0)
        self.assertIn("ui4me-host info:", result.stderr.decode())

    def test_missing_host_is_a_clear_startup_error(self):
        result = self.run_app(extra_env={"UI4ME_HOST_BINARY": str(ROOT / "missing-host")})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("StartupError: UI4ME_HOST_BINARY does not exist", result.stderr.decode())
        self.assertEqual(self.lines(result.stdout), ["ROLE launcher"])

    def run_script(self, body, *arguments, timeout=60):
        temporary = tempfile.TemporaryDirectory(prefix="ui4me direct ")
        self.addCleanup(temporary.cleanup)
        script = Path(temporary.name) / "app.py"
        script.write_text(textwrap.dedent(body), encoding="utf-8")
        env = checkout_environment(CLEAN)
        env.update({"UI4ME_TEST_MODE": "1", "UI4ME_HOST_BINARY": built_host(), "PYTHONIOENCODING": "utf-8"})
        return subprocess.run([sys.executable, str(script), *arguments], cwd=temporary.name, env=env,
                              capture_output=True, timeout=timeout, check=False)

    def test_second_start_raises_instead_of_launching_another_host(self):
        started = time.monotonic()
        result = self.run_script('''
            from ui4me_core import Application, StartupError
            app = Application.start(name="twice", instance="multiple")
            try:
                Application.start(name="twice", instance="multiple")
            except StartupError as error:
                print("REFUSED", error, flush=True)
            app.open_html("<p>x</p>")
            app.rpc.call_async("app.request_quit")
            app.run()
            ''')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("REFUSED Application.start() was already called", result.stdout.decode())
        self.assertLess(time.monotonic() - started, 30)

    def test_quit_requested_during_startup_is_orderly(self):
        result = self.run_script('''
            from ui4me_core import Application
            app = Application.start(name="early quit", instance="multiple")
            app.open_html("<p>x</p>")
            app.request_quit()
            app.run()
            print("RETURNED", flush=True)
            ''')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("RETURNED", result.stdout.decode())

    def test_failed_first_page_can_fall_back(self):
        result = self.run_script('''
            from ui4me_core import Application, RpcError
            app = Application.start(name="fallback", instance="multiple")
            try:
                app.open_html("<p onclick='x'>scripted</p>")
            except RpcError as error:
                print("FIRST FAILED", error.code, flush=True)
                window = app.open_html("<p id='f'>fallback</p>")
            print("TEXT", window.document.get_text("f"), flush=True)
            app.ready()
            print("WINDOWS", len(app.rpc.call("window.list")), flush=True)
            app.request_quit()
            app.run()
            ''')
        out = result.stdout.decode()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("FIRST FAILED -32602", out)
        self.assertIn("TEXT fallback", out)

    def test_sys_exit_in_a_close_policy_quits_instead_of_hanging(self):
        result = self.run_script('''
            import sys
            from ui4me_core import Application
            app = Application.start(name="policy exit", instance="multiple")
            window = app.open_html("<p>x</p>")
            window.on_before_close(lambda: sys.exit(0))
            app.ready()
            window.request_close()
            app.run()
            print("ORDERLY", flush=True)
            ''', timeout=40)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("ORDERLY", result.stdout.decode())
        self.assertIn("requesting application quit", result.stderr.decode())

    @unittest.skipUnless(sys.platform.startswith("linux"), "POSIX signal status")
    def test_host_killed_by_signal_gives_status_137_and_a_message(self):
        result = self.run_script('''
            import os, signal, time
            from ui4me_core import Application
            app = Application.start(name="killed", instance="multiple")
            app.open_html("<p>x</p>")
            app.ready()
            host = os.getppid()
            with open(f"/proc/{host}/comm") as comm:
                assert comm.read().strip() == "ui4me-host", "the backend's parent is not the host"
            os.kill(host, signal.SIGKILL)
            time.sleep(30)
            ''')
        self.assertEqual(result.returncode, 137, result.stderr.decode())
        self.assertIn("terminated by SIGKILL", result.stderr.decode())

    @unittest.skipUnless(sys.platform.startswith("linux"), "POSIX signals")
    def test_sigint_to_the_launcher_alone_ends_the_app_with_130(self):
        import signal
        temporary = tempfile.TemporaryDirectory(prefix="ui4me direct ")
        self.addCleanup(temporary.cleanup)
        script = Path(temporary.name) / "app.py"
        script.write_text(textwrap.dedent('''
            from ui4me_core import Application
            app = Application.start(name="interrupted", instance="multiple")
            app.open_html("<p>x</p>")
            app.ready()
            print("READY", flush=True)
            app.run()
            '''), encoding="utf-8")
        env = checkout_environment(CLEAN)
        env.update({"UI4ME_TEST_MODE": "1", "UI4ME_HOST_BINARY": built_host()})
        process = subprocess.Popen([sys.executable, str(script)], cwd=temporary.name, env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline().strip(), "READY")
            process.send_signal(signal.SIGINT)  # the launcher only, not its process group
            _, err = process.communicate(timeout=30)
        finally:
            if process.poll() is None:
                process.kill()
        self.assertEqual(process.returncode, 130, err)
        self.assertNotIn("terminated by", err)

    def test_temporary_manifest_is_removed(self):
        self.assertEqual(self.run_app().returncode, 0)
        self.assertEqual(list(self.temp.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
