#!/usr/bin/env python3
"""Native qualification of the embedded decoder; run outside a terminal sandbox."""
from pathlib import Path
import argparse
from datetime import datetime, timezone
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def invoke(bundle, flag):
    return subprocess.run(
        [str(bundle / "Contents/MacOS/TrueRenderer"), flag, "--root", str(ROOT)],
        capture_output=True, text=True, timeout=75,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", default="dist/TrueRenderer.app")
    parser.add_argument("--fault-bundle", help="separately compiled test service under var/")
    args = parser.parse_args()
    bundle = (ROOT / args.bundle).resolve()
    assert bundle.suffix == ".app" and ROOT in bundle.parents
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    normal = invoke(bundle, "--verify-xpc")
    if normal.returncode:
        raise SystemExit(normal.stderr or normal.stdout)
    integrated = json.loads(normal.stdout)
    assert integrated["passed"] and integrated["two_distinct_processes"]
    assert integrated["corpus_images_checked"] == 12
    rejected = invoke(bundle, "--verify-xpc-growth")
    assert rejected.returncode == 1 and "non è una build di fault injection" in rejected.stderr, rejected
    growth = None
    if args.fault_bundle:
        fault = (ROOT / args.fault_bundle).resolve()
        assert (ROOT / "var") in fault.parents and fault != bundle
        measured = invoke(fault, "--verify-xpc-growth")
        if measured.returncode:
            raise SystemExit(measured.stderr or measured.stdout)
        growth = json.loads(measured.stdout)
        assert growth["passed"] and growth["observed_overshoot_bytes"] > 0
    report = {
        "application": "TrueRenderer", "version": integrated["version"], "passed": True,
        "checked_at": datetime.now(timezone.utc).isoformat(),
        "bundle": str(bundle.relative_to(ROOT)),
        "binary_sha256": {relative: hashlib.sha256((bundle / relative).read_bytes()).hexdigest()
                          for relative in ["Contents/MacOS/TrueRenderer",
                                           "Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder",
                                           "Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder"]},
        "integration_report": "xpc-integration-macos.json",
        "production_rejects_fault_injection": True,
        "production_rejection_exit_code": rejected.returncode,
        "production_rejection": rejected.stderr.strip(),
        "memory_growth_report": "xpc-memory-growth-macos.json" if growth else None,
        "full_sandbox_gate_passed": False,
    }
    (ROOT / "reports/xpc-qualification-macos.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
