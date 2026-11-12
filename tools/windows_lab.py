#!/usr/bin/env python3
"""Manage the explicitly configured UI4ME Windows test VM through SSH.

Credentials live in a user-supplied local JSON file outside the repository.
This helper never prints them, guesses a VM ID, or affects unrelated guests.
"""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def ssh(target, argv, timeout=20, stdin=None, quiet=False):
    command = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", target, shlex.join(argv)]
    result = subprocess.run(command, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, timeout=timeout, check=False)
    if result.returncode and not quiet:
        raise RuntimeError(f"SSH command failed on {target}: {result.stderr.strip()[:500]}")
    return result


def load_config(path):
    path = path.expanduser().resolve()
    if path.is_relative_to(ROOT):
        raise ValueError("Keep the Windows credential/configuration record outside the repository")
    config = json.loads(path.read_text(encoding="utf-8"))
    for field in ("admin_ssh", "proxmox_ssh", "vm_name"):
        if not isinstance(config.get(field), str) or not config[field]:
            raise ValueError(f"The configuration must name {field} explicitly")
    for field in ("admin_ssh", "proxmox_ssh"):
        if any(c.isspace() for c in config[field]) or config[field].startswith("-"):
            raise ValueError(f"Invalid SSH target in {field}")
    return config


def discover_vm(config):
    result = ssh(config["proxmox_ssh"], ["pvesh", "get", "/cluster/resources", "--type", "vm", "--output-format", "json"])
    entries = json.loads(result.stdout)
    matching = [v for v in entries if v.get("type") == "qemu" and
                str(v.get("name", "")).casefold() == config["vm_name"].casefold()]
    if len(matching) != 1:
        raise RuntimeError(f"Expected one VM named {config['vm_name']!r}; found {len(matching)}")
    vm = matching[0]
    node = ssh(config["proxmox_ssh"], ["hostname", "-s"]).stdout.strip()
    if vm.get("node") != node:
        raise RuntimeError("The verified guest belongs to a different Proxmox node; no action taken")
    if type(vm.get("vmid")) is not int or vm["vmid"] < 1:
        raise RuntimeError("Proxmox returned an invalid VM ID")
    return vm


def guest_reachable(config):
    try:
        result = ssh(config["admin_ssh"], ["whoami"], timeout=12, quiet=True)
        return result.returncode == 0
    except subprocess.TimeoutExpired:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("action", choices=("status", "recover"))
    parser.add_argument("--restart-running", action="store_true", help="Request orderly restart of a running but unresponsive VM")
    parser.add_argument("--force", action="store_true", help="Allow forced stop only after orderly shutdown has failed")
    args = parser.parse_args()
    config = load_config(args.config)
    vm = discover_vm(config)
    reachable = guest_reachable(config)
    print(json.dumps({"vm": vm["name"], "vmid": vm["vmid"], "node": vm["node"],
                      "status": vm.get("status"), "guest_ssh": reachable,
                      "memory": vm.get("mem"), "memory_limit": vm.get("maxmem")}, indent=2), flush=True)
    if args.action == "status" or reachable:
        return 0
    vmid = str(vm["vmid"])
    if vm.get("status") == "running":
        if not args.restart_running:
            print("Guest is running but SSH is unavailable. Inspect RDP/guest health before --restart-running.", file=sys.stderr)
            return 2
        print("Requesting orderly shutdown of the verified guest.", flush=True)
        try:
            ssh(config["proxmox_ssh"], ["qm", "shutdown", vmid, "--timeout", "30"], timeout=45, quiet=True)
        except subprocess.TimeoutExpired:
            print("Orderly shutdown command timed out; rechecking the exact guest before any further action.", flush=True)
        current = discover_vm(config)
        if current["vmid"] != vm["vmid"] or current["node"] != vm["node"]:
            raise RuntimeError("Guest identity changed during recovery; no further action taken")
        if current.get("status") != "stopped":
            if not args.force:
                print("Orderly shutdown did not finish. Force recovery was not enabled.", file=sys.stderr)
                return 2
            print("Force-stopping the verified unresponsive guest.", flush=True)
            ssh(config["proxmox_ssh"], ["qm", "stop", vmid], timeout=45)
    elif vm.get("status") != "stopped":
        raise RuntimeError(f"Unexpected VM state: {vm.get('status')}; no action taken")
    print("Starting the verified Windows guest.", flush=True)
    current = discover_vm(config)
    if current["vmid"] != vm["vmid"] or current["node"] != vm["node"] or current.get("status") != "stopped":
        raise RuntimeError("The original guest is no longer verified as stopped; no start command sent")
    ssh(config["proxmox_ssh"], ["qm", "start", vmid], timeout=45)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        if guest_reachable(config):
            print("Guest SSH is available. Reconnect the dedicated RDP session before GUI tests.")
            return 0
        print("Waiting for guest SSH...", flush=True)
        time.sleep(5)
    print("VM started but guest SSH did not recover before the deadline.", file=sys.stderr)
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"Windows lab operation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
