#!/usr/bin/env python3
"""Run the signed macOS XPC lab against synthetic fixtures only."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import platform
import plistlib
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
LAB = ROOT / "var/sandbox-probe"
SOURCE = b"TrueRenderer XPC probe: readonly source.\n"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limited", action="store_true")
    args = parser.parse_args()
    BUNDLE = (LAB / "limited" if args.limited else LAB) / "TrueRendererSandboxProbe.app"
    HOST = BUNDLE / "Contents/MacOS/TrueRendererSandboxProbe"
    WORKER = BUNDLE / "Contents/XPCServices/ProbeWorker.xpc"
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(BUNDLE)], check=True)
    signed = subprocess.run(["/usr/bin/codesign", "--display", "--entitlements", "-", "--xml", str(WORKER)],
                            check=True, capture_output=True)
    entitlements = plistlib.loads(signed.stdout)
    if entitlements != {"com.apple.security.app-sandbox": True}:
        raise SystemExit("Unexpected worker entitlements: aborting probe.")
    report = {
        "date": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "macos": platform.mac_ver()[0],
        "machine": platform.machine(),
        "signature": "ad hoc, bundle verified deep/strict",
        "worker_entitlements": entitlements,
        "host_sha256": digest(HOST),
        "worker_sha256": digest(WORKER / "Contents/MacOS/ProbeWorker"),
    }
    with tempfile.TemporaryDirectory(prefix="fixture-", dir=LAB) as temporary, \
            socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        fixture = Path(temporary)
        source = fixture / "readonly-source.txt"
        source.write_bytes(SOURCE)
        source.chmod(0o600)
        attempted_create = fixture / "worker-must-not-create.txt"
        listener.bind(("127.0.0.1", 0))
        listener.listen(8)
        port = listener.getsockname()[1]
        with socket.create_connection(("127.0.0.1", port), timeout=2):
            report["host_loopback_control_passed"] = True
        # Controls ensure a later denial is not a missing file, Unix mode or closed listener.
        descriptor = os.open(source, os.O_RDWR)
        os.close(descriptor)
        report["host_file_read_write_control_passed"] = source.read_bytes() == SOURCE
        try:
            completed = subprocess.run([str(HOST), str(source), str(attempted_create), str(port)],
                                       capture_output=True, text=True, timeout=100)
            report["host_exit_code"] = completed.returncode
            report["host_stderr"] = completed.stderr[:4096]
            if completed.returncode == 0:
                report.update(json.loads(completed.stdout))
            else:
                report["experiment_completed"] = False
        except subprocess.TimeoutExpired:
            report["experiment_completed"] = False
            report["error"] = "Host exceeded 100 second experiment deadline."
        report["source_unchanged"] = source.read_bytes() == SOURCE
        report["no_external_file_created"] = not attempted_create.exists()
    report["experiment_completed"] = report.get("host_exit_code") == 0
    report["full_sandbox_gate_passed"] = False
    report["experiment_checks_passed"] = all(report.get(key) is True for key in (
        "experiment_completed", "basic_files_network_fd_checks_passed", "protocol_lifecycle_checks_passed",
        "source_unchanged", "no_external_file_created", "host_loopback_control_passed",
        "host_file_read_write_control_passed",
    ))
    if args.limited:
        report["experiment_checks_passed"] &= report.get("capabilities", {}).get("nproc_policy_checks_passed") is True
    output = ROOT / ("reports/xpc-sandbox-limited-macos.json" if args.limited else "reports/xpc-sandbox-macos.json")
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    print(f"Report: {output}")
    raise SystemExit(0 if report["experiment_checks_passed"] else 1)


if __name__ == "__main__":
    main()
