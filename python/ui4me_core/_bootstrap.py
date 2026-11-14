"""Direct startup for ``python app.py`` and ``uv run app.py``.

The Rust host must own the native windows and event loop, and it supervises
the language process that implements the application. ``Application.start()``
therefore works in two roles:

* **Launcher** (the process the developer started): no host connection
  exists yet. Write a private temporary manifest naming this exact Python
  interpreter and command line, run the packaged native host, relay the
  backend's output to this console, then exit with the host's status.
* **Backend** (the same script, started again by the host): the host provided
  ``UI4ME_ENDPOINT`` and ``UI4ME_TOKEN``. Connect and continue normally.

Everything before ``Application.start()`` runs in both processes, so keep
side effects after it and guard the entry point with
``if __name__ == "__main__":``.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import unicodedata
from typing import IO

ENDPOINT = "UI4ME_ENDPOINT"
TOKEN = "UI4ME_TOKEN"
# Set by the launcher for the host and therefore inherited by the backend it
# starts. A process carrying it but no endpoint was re-executed without a
# working host connection; starting yet another host would loop.
DIRECT_MARKER = "UI4ME_DIRECT_START"
VERBOSE = "UI4ME_VERBOSE"
_HOST_SCRUBBED = (ENDPOINT, TOKEN, "UI4ME_APP_ID", "UI4ME_PROTOCOL_VERSION", DIRECT_MARKER)
_INSTANCES = ("single", "multiple")


class StartupError(RuntimeError):
    """UI4ME could not start the native host or connect to it."""


def managed() -> bool:
    """True when this process is a backend started by the native host."""
    endpoint, token = os.environ.get(ENDPOINT), os.environ.get(TOKEN)
    if endpoint and token:
        return True
    if endpoint or token:
        raise StartupError(
            "Incomplete UI4ME host environment: UI4ME_ENDPOINT and UI4ME_TOKEN must both be set by "
            "the native host. Unset them to start the application directly.")
    if os.environ.get(DIRECT_MARKER):
        raise StartupError(
            "This backend was started by a UI4ME launcher but received no host connection. "
            "Refusing to start a second native host.")
    return False


def forget_host_environment() -> None:
    """Keep the connection secret and the role marker out of child processes
    the application starts, so another UI4ME app can be launched from it."""
    for name in _HOST_SCRUBBED:
        os.environ.pop(name, None)


def validate_options(*, name: str | None, app_id: str | None, version: str | None,
                     instance: str, startup_timeout_ms: int, shutdown_timeout_ms: int,
                     max_windows: int) -> None:
    for label, value in (("name", name), ("version", version)):
        if value is not None and (not isinstance(value, str) or not value.strip() or len(value) > 128
                                  or any(unicodedata.category(c) == "Cc" for c in value)):
            raise ValueError(f"{label} must be 1-128 characters without control characters")
    if app_id is not None and not valid_app_id(app_id):
        raise ValueError("app_id must be a dot-separated ASCII identity such as org.example.MyApp")
    if instance not in _INSTANCES:
        raise ValueError("instance must be 'single' or 'multiple'")
    for label, value, low, high in (("startup_timeout_ms", startup_timeout_ms, 1_000, 300_000),
                                    ("shutdown_timeout_ms", shutdown_timeout_ms, 100, 120_000),
                                    ("max_windows", max_windows, 1, 64)):
        if isinstance(value, bool) or not isinstance(value, int) or not low <= value <= high:
            raise ValueError(f"{label} must be an integer between {low} and {high}")


_COMPONENT = re.compile(r"[A-Za-z][A-Za-z0-9_-]*\Z")


def valid_app_id(value: object) -> bool:
    """The host's rule: ASCII, at most 128 bytes, at least two components,
    each starting with a letter and containing letters, digits, '_' or '-'."""
    return (isinstance(value, str) and 0 < len(value) <= 128 and value.isascii() and "." in value
            and all(_COMPONENT.match(part) for part in value.split(".")))


def _slug(text: str) -> str:
    folded = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode("ascii")
    slug = re.sub(r"[^A-Za-z0-9]+", "-", folded).strip("-")[:40].strip("-")
    if not slug or not slug[0].isalpha():
        slug = "app-" + slug if slug else "app"
    return slug


def default_app_id(identity: str, label: str) -> str:
    """A stable identity per script, so a second launch of the same script
    reaches the running instance while different scripts stay independent."""
    digest = hashlib.sha256(identity.encode("utf-8", "surrogatepass")).hexdigest()[:12]
    return f"local.ui4me.{_slug(label)}.h{digest}"


def backend_command() -> tuple[str, list[str], str, str]:
    """Return (program, arguments, identity, label) that re-run this program.

    The interpreter options and ``-m`` module form are preserved from
    ``sys.orig_argv``; a script path is made absolute so a later ``chdir``
    cannot break the second execution.
    """
    if getattr(sys, "frozen", False):
        executable = os.path.abspath(sys.executable)
        return executable, list(sys.argv[1:]), executable, Path(executable).stem
    main = sys.modules.get("__main__")
    main_file = getattr(main, "__file__", None)
    original = list(getattr(sys, "orig_argv", []) or [])
    if (not sys.executable or not main_file or not original or sys.flags.interactive
            or sys.argv[0] in ("", "-", "-c")):
        raise StartupError(
            "Application.start() must run from a script file or `python -m package.module`, "
            "not an interactive interpreter, `python -c` or standard input.")
    tail = len(sys.argv) - 1
    designator = len(original) - tail - 1
    if designator < 1:
        raise StartupError("Cannot reconstruct this Python command line for the native host")
    arguments = original[1:]
    spec = getattr(main, "__spec__", None)
    if spec is not None and spec.name:
        # `python -m package.app` (or -mpackage.app): keep the module form;
        # its import path is resolved from the working directory.
        module = spec.name[:-len(".__main__")] if spec.name.endswith(".__main__") else spec.name
        identity = f"module:{module}:{os.getcwd()}"
        label = module.rsplit(".", 1)[-1]
    else:
        script = os.path.abspath(main_file)
        arguments[designator - 1] = script
        identity = "script:" + os.path.normcase(script)
        label = Path(script).stem
    return sys.executable, arguments, identity, label


def _toml(value: str) -> str:
    try:
        value.encode("utf-8")
    except UnicodeEncodeError:
        raise StartupError(f"UI4ME cannot pass a non-Unicode path or argument to the native host: {value!r}") from None
    if "\x00" in value:
        raise StartupError("Arguments and paths must not contain NUL characters")
    # A JSON string with literal Unicode is a valid TOML basic string.
    return json.dumps(value, ensure_ascii=False)


def write_manifest(directory: Path, *, program: str, arguments: list[str], cwd: str,
                   app_id: str, name: str, version: str | None, icon: str | os.PathLike | None,
                   instance: str, startup_timeout_ms: int, shutdown_timeout_ms: int,
                   max_windows: int, env: dict[str, str]) -> Path:
    lines = ["[app]", f"id = {_toml(app_id)}", f"name = {_toml(name)}"]
    if version is not None:
        lines.append(f"version = {_toml(version)}")
    if icon is not None:
        lines.append(f"icon = {_toml(os.path.abspath(os.fspath(icon)))}")
    lines += ["", "[backend]", f"program = {_toml(program)}",
              "args = [" + ", ".join(_toml(a) for a in arguments) + "]",
              f"cwd = {_toml(cwd)}", "", "[backend.env]"]
    lines += [f"{key} = {_toml(value)}" for key, value in sorted(env.items())]
    lines += ["", "[host]", f"startup_timeout_ms = {startup_timeout_ms}",
              f"shutdown_timeout_ms = {shutdown_timeout_ms}", f"max_windows = {max_windows}",
              f"instance = {_toml(instance)}", ""]
    path = directory / "app.toml"
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


def _write(stream: IO[str] | None, text: str) -> None:
    if stream is None:
        return
    try:
        stream.write(text + "\n")
    except UnicodeEncodeError:
        encoding = getattr(stream, "encoding", None) or "ascii"
        stream.write(text.encode(encoding, "backslashreplace").decode(encoding) + "\n")
    except (OSError, ValueError):
        return
    try:
        stream.flush()
    except (OSError, ValueError):
        pass


def relay_line(raw: bytes, *, verbose: bool, stdout: IO[str] | None, stderr: IO[str] | None) -> None:
    """Turn one host log record back into what the developer expects to see.

    Backend ``print()`` output becomes plain text on the matching stream; host
    warnings and errors are summarized; informational host records appear only
    with ``UI4ME_VERBOSE=1``. Text that is not a host record passes through.
    """
    text = raw.decode("utf-8", "replace").rstrip("\r\n")
    try:
        record = json.loads(text)
    except ValueError:
        record = None
    if not isinstance(record, dict) or not isinstance(record.get("fields"), dict):
        if text:
            _write(stderr, text)
        return
    fields = record["fields"]
    if fields.get("source") == "backend" and isinstance(fields.get("text"), str):
        _write(stdout if fields.get("stream") == "stdout" else stderr, fields["text"])
        return
    level = str(record.get("level", "INFO")).upper()
    if not verbose and level not in ("WARN", "ERROR"):
        return
    message = str(fields.get("message", ""))
    extra = ", ".join(f"{key}={value}" for key, value in fields.items() if key != "message")
    _write(stderr, f"ui4me-host {level.lower()}: {message}" + (f" ({extra})" if extra else ""))


def _linux_parent_death_signal() -> None:  # pragma: no cover - runs in the child
    """If the launcher dies (even SIGKILL), terminate the host it started."""
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl(1, signal.SIGTERM)  # PR_SET_PDEATHSIG


def _windows_kill_on_close_job(process: subprocess.Popen):  # pragma: no cover - Windows only
    """Bind the host's lifetime to this launcher. If the launcher is killed,
    Windows closes the job handle and terminates the host, whose own job then
    ends the backend. Processes the host opens for the user (a browser, a file
    viewer) break away silently and stay open."""
    import ctypes
    from ctypes import wintypes

    class BasicLimits(ctypes.Structure):
        _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                    ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                    ("SchedulingClass", wintypes.DWORD)]

    class IoCounters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint64) for name in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class ExtendedLimits(ctypes.Structure):
        _fields_ = [("BasicLimitInformation", BasicLimits), ("IoInfo", IoCounters),
                    ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                    ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateJobObjectW.restype = wintypes.HANDLE
    kernel32.CreateJobObjectW.argtypes = (ctypes.c_void_p, wintypes.LPCWSTR)
    kernel32.SetInformationJobObject.argtypes = (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD)
    kernel32.AssignProcessToJobObject.argtypes = (wintypes.HANDLE, wintypes.HANDLE)
    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        return None
    limits = ExtendedLimits()
    # KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK
    limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x1000
    if not kernel32.SetInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits)) \
            or not kernel32.AssignProcessToJobObject(job, int(process._handle)):
        kernel32.CloseHandle(job)
        return None
    return job  # Deliberately held open until this process exits.


def spawn_host(command: list[str], *, env: dict[str, str]) -> tuple[subprocess.Popen, object]:
    options: dict = {"stdin": subprocess.DEVNULL, "stderr": subprocess.PIPE, "env": env}
    if sys.platform.startswith("linux"):
        options["preexec_fn"] = _linux_parent_death_signal
    process = subprocess.Popen(command, **options)
    job = _windows_kill_on_close_job(process) if os.name == "nt" else None
    return process, job


class _Interrupted(Exception):
    def __init__(self, signum: int):
        super().__init__(signum)
        self.signum = signum


def run_host(command: list[str], *, env: dict[str, str]) -> int:
    """Run the host, relay its output, and return its exit status. Ctrl+C,
    SIGTERM or SIGHUP stop the host (and with it the backend) boundedly."""
    process, _job = spawn_host(command, env=env)
    verbose = os.environ.get(VERBOSE, "").strip().lower() not in ("", "0", "false", "no")
    previous = {}

    def interrupted(signum, _frame):
        raise _Interrupted(signum)

    for name in ("SIGTERM", "SIGHUP"):
        number = getattr(signal, name, None)
        if number is not None:
            try:
                previous[number] = signal.signal(number, interrupted)
            except (OSError, ValueError):
                pass
    stopped = False
    try:
        try:
            assert process.stderr is not None
            for line in process.stderr:
                relay_line(line, verbose=verbose, stdout=sys.stdout, stderr=sys.stderr)
            return process.wait()
        except (KeyboardInterrupt, _Interrupted) as interruption:
            stopped = True
            # A terminal Ctrl+C reaches the host too; a signal sent to the
            # launcher alone does not. Either way, stop the host boundedly.
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            # The conventional shell status for a process ended by a signal.
            return 128 + getattr(interruption, "signum", signal.SIGINT)
    finally:
        for number, handler in previous.items():
            try:
                signal.signal(number, handler)
            except (OSError, ValueError):
                pass
        if process.poll() is None and not stopped:
            process.kill()
            process.wait()
        if process.stderr is not None:
            process.stderr.close()


_MANIFEST_PREFIX = "ui4me-start-"
_STALE_SECONDS = 24 * 3600


def _sweep_stale_manifests() -> None:
    """Remove manifests left by launchers that were killed outright. The host
    reads its manifest only while starting, so a day-old one is never in use."""
    import time

    try:
        candidates = list(Path(tempfile.gettempdir()).glob(_MANIFEST_PREFIX + "*"))
    except OSError:
        return
    limit = time.time() - _STALE_SECONDS
    for folder in candidates:
        try:
            if (folder.is_dir() and not folder.is_symlink() and folder.stat().st_mtime < limit
                    and (not hasattr(os, "getuid") or folder.stat().st_uid == os.getuid())):
                shutil.rmtree(folder, ignore_errors=True)
        except OSError:
            continue


def launch(*, name: str | None, app_id: str | None, version: str | None,
           icon: str | os.PathLike | None, instance: str, startup_timeout_ms: int,
           shutdown_timeout_ms: int, max_windows: int) -> int:
    """Start the native host for this script and return its exit status."""
    from .launcher import host_binary

    program, arguments, identity, label = backend_command()
    try:
        host = host_binary()
    except FileNotFoundError as error:
        raise StartupError(str(error)) from None
    if icon is not None and not Path(icon).is_file():
        raise StartupError(f"Application icon does not exist: {icon}")
    backend_env = {}
    # The backend's stdout/stderr are pipes into the host: keep print()
    # immediate and UTF-8 there, unless the developer chose otherwise.
    for key, value in (("PYTHONUNBUFFERED", "1"), ("PYTHONIOENCODING", "utf-8")):
        if key not in os.environ:
            backend_env[key] = value
    env = dict(os.environ)
    env[DIRECT_MARKER] = str(os.getpid())
    _sweep_stale_manifests()
    folder = Path(tempfile.mkdtemp(prefix=_MANIFEST_PREFIX))
    try:
        manifest = write_manifest(
            folder, program=program, arguments=arguments, cwd=os.getcwd(),
            app_id=app_id or default_app_id(identity, name or label), name=name or label,
            version=version, icon=icon, instance=instance, startup_timeout_ms=startup_timeout_ms,
            shutdown_timeout_ms=shutdown_timeout_ms, max_windows=max_windows, env=backend_env)
        for stream in (sys.stdout, sys.stderr):
            try:
                stream.flush()
            except (AttributeError, OSError, ValueError):
                pass
        command = [str(host), "run", "--app", str(manifest)]
        if os.environ.get("UI4ME_TEST_MODE") == "1":
            # The host's documented test mode: protocol and lifecycle without a
            # desktop (continuous integration). Never chosen implicitly.
            command.append("--headless")
        try:
            return run_host(command, env=env)
        except OSError as error:
            raise StartupError(f"Cannot start the UI4ME native host {host}: {error}") from error
    finally:
        shutil.rmtree(folder, ignore_errors=True)
