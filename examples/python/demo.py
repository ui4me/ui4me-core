"""A two-window application controlled entirely through the host protocol.

Run with tools/run_example.py. --self-test is the headless protocol exercise;
--native-test additionally exercises native clipboard through the same client.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import threading
import time
import traceback

from client import RpcClient, RpcError


def window_id(value):
    if isinstance(value, str):
        return value
    return value.get("window_id", value.get("id"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--native-test", action="store_true")
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--external-target", help="Explicit file/URL to open through the desktop broker when F8 is pressed")
    args = parser.parse_args()
    results = []
    def record(name, status="PASS", detail=None):
        row = {"test": name, "status": status}
        if detail is not None:
            row["detail"] = detail
        results.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    client = None
    try:
        client = RpcClient.from_environment()
        record("authenticated_handshake")
        capabilities = client.call("host.capabilities")
        record("capabilities", detail=capabilities)
        paths = client.call("desktop.paths")
        record("desktop_paths", detail=paths)
        main_id = window_id(client.call("window.create", {
            "title": "UI4ME host — Python reference", "width": 900, "height": 560,
            "visible": True,
        }))
        second_id = window_id(client.call("window.create", {
            "title": "UI4ME — second window", "width": 540, "height": 360,
            "visible": True,
        }))
        if not main_id or not second_id or main_id == second_id:
            raise AssertionError("Host returned invalid window identities")
        record("two_windows")
        client.call("probe.set_text", {"window_id": main_id,
            "text": "Python is connected. Type here to test native text input.\n"
                    "F2 clipboard; F3 open; F4 save; F5 folder; F6 worker delay; F7 cancel dialog.\n"
                    "F8 opens the explicitly configured external target, if supplied.\n"
                    "Close either window to exercise an asynchronous close decision."})
        client.call("probe.set_text", {"window_id": second_id,
            "text": "Second independent native window.\nUnicode: Příliš žluťoučký kůň — 日本語 — 😀"})
        client.call("window.set", {"window_id": second_id, "title": "UI4ME — Unicode ✓"})
        record("window_update_and_probe")

        # This handler performs an RPC while answering a host request. Its reply
        # must be processed by the separate reader rather than deadlocking.
        def before_close(params):
            client.call("host.capabilities")
            print("Close accepted", params.get("window_id"), flush=True)
            return {"allow": True}
        client.on_request("window.before_close", before_close)
        client.on_request("app.before_quit", lambda _: {"allow": True})

        dialogs, dialog_lock = {}, threading.Lock()
        def window_event(params):
            event_type, data = params.get("type"), params.get("data", {})
            if event_type in ("text", "focused", "dropped_files", "dropped_text", "close_requested"):
                print("window.event", json.dumps(params, ensure_ascii=False), flush=True)
            if event_type != "key" or data.get("state") not in (None, "pressed"):
                return
            key = data.get("key", data.get("logical_key", ""))
            wid = params.get("window_id", main_id)
            if key == "F2":
                client.call("clipboard.write_text", {"text": "UI4ME clipboard — Český text 😀"})
                print("Clipboard:", client.call("clipboard.read_text"), flush=True)
            elif key in ("F3", "F4", "F5"):
                method = {"F3": "dialog.open_files", "F4": "dialog.save_file", "F5": "dialog.choose_folder"}[key]
                future = client.call_async(method, {"window_id": wid})
                with dialog_lock:
                    dialogs[wid] = future
                try:
                    print(method, future.result(), flush=True)
                except concurrent.futures.CancelledError:
                    print(method, "client cancellation requested", flush=True)
                except RpcError as error:
                    print(method, "host response:", error, flush=True)
                finally:
                    with dialog_lock:
                        if dialogs.get(wid) is future:
                            dialogs.pop(wid, None)
            elif key == "F6":
                print("Backend callback sleeping for five seconds; native input should continue.", flush=True)
                time.sleep(5)
                print("Backend callback resumed.", flush=True)
            elif key == "F7":
                # A modal parent may not receive keys. Use the other application
                # window to request cancellation of every outstanding dialog.
                with dialog_lock:
                    pending_dialogs = list(dialogs.values())
                for future in pending_dialogs:
                    future.cancel()
            elif key == "F8":
                if args.external_target:
                    result = client.call("desktop.open_external", {"target": args.external_target})
                    record("external_open_requested", detail={"target": args.external_target, "result": result})
                else:
                    print("Supply --external-target when launching this demo before using F8.", flush=True)
        client.on_notification("window.event", window_event)
        client.on_notification("app.activated", lambda p: print("Second launch:", json.dumps(p), flush=True))
        client.call("app.ready")
        record("application_ready")

        if args.self_test or args.native_test:
            outstanding = [client.call_async("host.capabilities") for _ in range(24)]
            for future in outstanding:
                future.result(10)
            record("concurrent_requests")
            if args.native_test:
                original = client.call("clipboard.read_text")
                try:
                    marker = "UI4ME native clipboard π — žluťoučký — 😀"
                    client.call("clipboard.write_text", {"text": marker})
                    actual = client.call("clipboard.read_text")
                    if isinstance(actual, dict):
                        actual = actual.get("text")
                    if actual != marker:
                        raise AssertionError(f"Clipboard round trip mismatch: {actual!r}")
                    record("native_clipboard_round_trip")
                finally:
                    old = original.get("text", "") if isinstance(original, dict) else original
                    client.call("clipboard.write_text", {"text": old or ""})
            else:
                record("native_desktop_checks", "BLOCKED", "Headless mode cannot qualify native desktop behavior")
            client.call("window.request_close", {"window_id": second_id})
            record("asynchronous_close_requested")
            time.sleep(0.3)
            client.call("app.request_quit")
            record("application_quit_requested")
            deadline = time.monotonic() + 10
            while not client.closed and not client.shutdown_requested.is_set() and time.monotonic() < deadline:
                time.sleep(0.05)
            if not client.closed and not client.shutdown_requested.is_set():
                raise TimeoutError("Host did not begin backend shutdown")
            record("shutdown_received")
        else:
            while not client.closed and not client.shutdown_requested.wait(0.25):
                pass
        return 0
    except Exception as error:
        record("reference_backend", "FAIL", f"{type(error).__name__}: {error}")
        traceback.print_exc()
        return 1
    finally:
        if client is not None:
            try:
                client.flush()
            except (TimeoutError, RpcError):
                pass
            client.close()
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(json.dumps({
                "schema": 1, "mode": "native" if args.native_test else "headless" if args.self_test else "interactive",
                "os": os.name, "results": results,
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
