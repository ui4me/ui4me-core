#!/usr/bin/env python3
"""Measure host startup, first frame, idle CPU, memory and repeated window cycles.

Runs the real host with this same file as its managed backend, in the current
native desktop session. Startup-to-ready and first frame are reported
separately. On Linux, memory is reported as PSS (proportional set size, which
divides shared pages between processes) next to RSS; on Windows as private bytes
and working set. Results are measurements of this machine only.
"""
import argparse
from contextlib import nullcontext
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from run_example import checkout_environment, locate_host  # noqa: E402
from native_acceptance import source_identity  # noqa: E402


# --------------------------------------------------------------------------- backend
def backend(args):
    sys.path.insert(0, str(ROOT / "python"))
    from ui4me_core import RpcClient
    events = Path(args.events)

    def emit(event, **detail):
        with events.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps({"event": event, "t": time.time(), **detail}) + "\n")

    client = RpcClient.from_environment()
    main = client.call("window.create", {"title": "UI4ME measurement", "width": 800, "height": 500})
    main = main if isinstance(main, str) else main["window_id"]
    def load_document(window):
        result = client.call("document.load", {"window_id": window,
            "path": str(ROOT / "examples/python/ui/index.html")}, timeout=30)
        key = {"window_id": window, "document_id": result["document_id"]}
        client.call("document.command", {**key, "command": {"op": "set_environment", "reduced_motion": args.reduced_motion}})
        deadline = time.monotonic() + 20
        while client.call("document.get", {"window_id": window})["pending_resources"]:
            if time.monotonic() > deadline:
                raise TimeoutError("Renderer resources did not settle")
            time.sleep(0.02)
        return key

    if args.renderer:
        main_document = load_document(main)
    else:
        client.call("probe.set_text", {"window_id": main, "text": "Resource measurement"})
    client.call("app.ready")
    diagnostics = client.call("host.diagnostics")
    emit("ready", host_pid=diagnostics["pid"], backend_pid=os.getpid())
    if args.idle:
        emit("idle_start")
        time.sleep(args.idle)
        emit("idle_end")
        time.sleep(2.0)  # the driver samples the end of the idle period first
    for index in range(args.cycles):
        window = client.call("window.create", {"title": f"UI4ME cycle {index}", "width": 400, "height": 300})
        window = window if isinstance(window, str) else window["window_id"]
        if args.renderer:
            load_document(window)
        else:
            client.call("probe.set_text", {"window_id": window, "text": f"cycle {index}"})
        time.sleep(0.05)
        client.call("window.request_close", {"window_id": window})
        deadline = time.monotonic() + 10
        while any(w["window_id"] == window for w in client.call("window.list")):
            if time.monotonic() > deadline:
                raise TimeoutError("cycle window was not closed")
            time.sleep(0.01)
        if (index + 1) % args.sample_every == 0:
            emit("cycles", done=index + 1)
            time.sleep(1.0)  # let the driver sample a settled state
    if args.renderer:
        for index in range(args.dom_cycles):
            markup = f'<tr id="churn-{index}"><td>{index}</td><td>Transient document row</td><td>Normal</td><td>Yes</td></tr>'
            client.call("document.command", {**main_document, "command": {"op": "batch", "ops": [
                {"op": "insert_html", "element": "rows", "position": "beforeend", "html": markup},
                {"op": "set_text", "element": "status", "text": f"Document mutation {index}"},
            ]}})
            client.call("document.command", {**main_document,
                "command": {"op": "remove_element", "element": f"churn-{index}"}})
            if (index + 1) % args.sample_every == 0:
                emit("dom_cycles", done=index + 1)
                time.sleep(1.0)
    emit("done")
    client.call("app.request_quit")
    if not client.shutdown_requested.wait(10):
        raise TimeoutError("Missing orderly shutdown")
    client.close()


# --------------------------------------------------------------------------- sampling
def cpu_seconds(pid):
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k.OpenProcess.restype = wintypes.HANDLE
        k.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        k.GetProcessTimes.restype = wintypes.BOOL
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = k.OpenProcess(0x1000, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        times = [wintypes.FILETIME() for _ in range(4)]
        try:
            if not k.GetProcessTimes(handle, *(ctypes.byref(t) for t in times)):
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            k.CloseHandle(handle)
        value = lambda t: ((t.dwHighDateTime << 32) | t.dwLowDateTime) / 1e7
        return value(times[2]) + value(times[3])
    # Exact scheduler runtime of every thread. Tick-sampled utime/stime
    # overstates processes that wake briefly but frequently.
    total = 0
    for task in Path(f"/proc/{pid}/task").iterdir():
        try:
            total += int((task / "schedstat").read_text().split()[0])
        except (OSError, ValueError, IndexError):
            pass  # thread exited while sampling
    return total / 1e9


def memory_kib(pid):
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in (
                    "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                    "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
                    "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k.OpenProcess.restype = wintypes.HANDLE
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        handle = k.OpenProcess(0x1000 | 0x0010, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        counters = Counters(cb=ctypes.sizeof(Counters))
        try:
            if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            k.CloseHandle(handle)
        return {"private_kib": counters.PrivateUsage // 1024, "working_set_kib": counters.WorkingSetSize // 1024}
    values = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines()[1:]:
        key, value = line.split(":", 1)
        values[key] = int(value.split()[0])
    return {"rss_kib": values["Rss"], "pss_kib": values["Pss"],
            "shared_kib": values.get("Shared_Clean", 0) + values.get("Shared_Dirty", 0),
            "private_kib": values.get("Private_Clean", 0) + values.get("Private_Dirty", 0)}


def combined(host, backend_pid):
    h, b = memory_kib(host), memory_kib(backend_pid)
    total = {key: h[key] + b[key] for key in h}
    return {"host": h, "backend": b, "combined": total}


# --------------------------------------------------------------------------- driver
def read_events(path):
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            rows.append(json.loads(line))
        except ValueError:
            pass
    return rows


def wait_event(path, name, deadline):
    while time.monotonic() < deadline:
        for row in read_events(path):
            if row["event"] == name:
                return row
        time.sleep(0.02)
    raise TimeoutError(f"backend did not report {name}")


def first_frame_time(log_path):
    for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("fields", {}).get("message") == "first frame presented":
            stamp = row["timestamp"].replace("Z", "+00:00")
            return datetime.fromisoformat(stamp).timestamp(), row["fields"].get("since_host_start_ms")
    return None, None


def run_once(host, folder, index, idle, cycles, sample_every, *, renderer=False, dom_cycles=0, reduced_motion="system"):
    events = folder / f"events-{index}.jsonl"
    log_path = folder / f"host-{index}.log"
    app = folder / f"app-{index}.toml"
    backend_args = [str(Path(__file__).resolve()), "--backend", "--events", str(events),
                    "--idle", str(idle), "--cycles", str(cycles), "--sample-every", str(sample_every)]
    if renderer:
        backend_args += ["--renderer", "--dom-cycles", str(dom_cycles), "--reduced-motion", reduced_motion]
    q = lambda value: json.dumps(value, ensure_ascii=False)
    app.write_text("\n".join([
        "[app]", 'id = "org.ui4me.Measurement"', 'name = "UI4ME measurement"', "",
        "[backend]", f"program = {q(sys.executable)}", f"args = {q(backend_args)}",
        f"cwd = {q(str(folder))}", "", "[host]", 'instance = "multiple"', "startup_timeout_ms = 30000", ""]), encoding="utf-8")
    with log_path.open("wb") as log:
        started = time.time()
        process = subprocess.Popen([host, "run", "--app", str(app)], stdout=log, stderr=subprocess.STDOUT,
                                   env=checkout_environment())
        try:
            ready = wait_event(events, "ready", time.monotonic() + 30)
            result = {"startup_to_ready_ms": round((ready["t"] - started) * 1000, 1)}
            host_pid, backend_pid = ready["host_pid"], ready["backend_pid"]
            if idle:
                start_row = wait_event(events, "idle_start", time.monotonic() + 10)
                time.sleep(3.0)  # skip startup work and window-open animations
                c0 = (cpu_seconds(host_pid), cpu_seconds(backend_pid), time.monotonic())
                wait_event(events, "idle_end", time.monotonic() + idle + 10)
                c1 = (cpu_seconds(host_pid), cpu_seconds(backend_pid), time.monotonic())
                span = c1[2] - c0[2]
                result["idle"] = {"seconds": round(span, 1),
                                  "host_cpu_percent": round(100 * (c1[0] - c0[0]) / span, 3),
                                  "backend_cpu_percent": round(100 * (c1[1] - c0[1]) / span, 3),
                                  "memory_after_idle": combined(host_pid, backend_pid)}
                del start_row
            if cycles or dom_cycles:
                samples, seen = {"cycles": [], "dom_cycles": []}, {"cycles": 0, "dom_cycles": 0}
                deadline = time.monotonic() + (cycles + dom_cycles) * 2 + 60
                completed = False
                while time.monotonic() < deadline:
                    for kind in samples:
                        rows = [r for r in read_events(events) if r["event"] == kind]
                        if len(rows) > seen[kind]:
                            seen[kind] = len(rows)
                            time.sleep(0.5)
                            samples[kind].append({kind: rows[-1]["done"], **combined(host_pid, backend_pid)})
                    if any(r["event"] == "done" for r in read_events(events)):
                        completed = True
                        break
                    time.sleep(0.05)
                if not completed:
                    raise TimeoutError("Window/DOM churn did not finish")
                result["window_cycles"] = samples["cycles"]
                if renderer:
                    result["dom_cycles"] = samples["dom_cycles"]
            process.wait(timeout=30)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
    frame_at, since_host_ms = first_frame_time(log_path)
    result["first_frame_ms"] = round((frame_at - started) * 1000, 1) if frame_at else None
    result["first_frame_since_host_start_ms"] = since_host_ms
    result["host_exit_code"] = process.returncode
    if process.returncode:
        raise RuntimeError(f"Measurement host failed with exit code {process.returncode}; {log_path}")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--events", help=argparse.SUPPRESS)
    parser.add_argument("--host")
    parser.add_argument("--startup-runs", type=int, default=5)
    parser.add_argument("--idle", type=float, default=25)
    parser.add_argument("--cycles", type=int, default=200)
    parser.add_argument("--sample-every", type=int, default=50)
    parser.add_argument("--renderer", action="store_true", help="Measure the actual loaded HTML/CSS reference document")
    parser.add_argument("--reduced-motion", choices=("system", "reduce", "no-preference"), default="system",
                        help="Default follows the OS; an explicit override is recorded in the report")
    parser.add_argument("--dom-cycles", type=int, default=400, help="Additional insert/update/remove cycles with --renderer")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.startup_runs < 1 or args.sample_every < 1 or min(args.cycles, args.dom_cycles) < 0:
        parser.error("Startup runs and sample interval must be positive; cycle counts must not be negative")
    if args.idle != 0 and args.idle < 5:
        parser.error("--idle must be zero or at least five seconds")
    if args.backend:
        return backend(args)
    host = locate_host(args.host, release=True)
    report = {"schema": 1, "time_utc": datetime.now(timezone.utc).isoformat(), "host": host,
              "source_sha256": source_identity(), "surface": "renderer" if args.renderer else "probe",
              "first_frame_scope": "First native window frame; not proof that asynchronous document resources are visible",
              "readiness_scope": "HTML loaded and resources settled" if args.renderer else "Probe created",
              "reduced_motion_request": args.reduced_motion if args.renderer else None,
              "platform": platform.platform(), "python": sys.version.split()[0],
              "cpu_metric": "Linux: summed per-thread schedstat runtime" if os.name != "nt"
              else "Windows: GetProcessTimes kernel+user time",
              "memory_metric": "Linux: PSS/RSS/shared/private from smaps_rollup" if os.name != "nt"
              else "Windows: private bytes and working set (GetProcessMemoryInfo)"}
    if args.output:
        raw = args.output.resolve().parent / (args.output.stem + "-raw-" + uuid.uuid4().hex[:8])
        raw.mkdir(parents=True, exist_ok=False)
        report["raw_directory"] = str(raw)
    else:
        raw = None
    with nullcontext(str(raw)) if raw else tempfile.TemporaryDirectory(prefix="ui4me-measure-") as tmp:
        folder = Path(tmp)
        report["startup"] = [run_once(host, folder, i, 0, 0, 1, renderer=args.renderer,
                                     reduced_motion=args.reduced_motion) for i in range(args.startup_runs)]
        report["sustained"] = run_once(host, folder, 99, args.idle, args.cycles, args.sample_every,
                                       renderer=args.renderer, dom_cycles=args.dom_cycles if args.renderer else 0,
                                       reduced_motion=args.reduced_motion)
    text = json.dumps(report, indent=2)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
