#!/usr/bin/env python3
"""Native browser layout probe on generated sources and an isolated library."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", default="dist/TrueRenderer-navigator.app")
    parser.add_argument("--report", default="reports/filesystem-browser-macos.json")
    args = parser.parse_args()
    bundle = (ROOT / args.bundle).resolve(strict=True)
    binary = bundle / "Contents/MacOS/TrueRenderer"
    evidence = ROOT / "var/filesystem-browser"
    evidence.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="native-", dir=evidence))
    shutil.copytree(ROOT / "corpus", run / "corpus", ignore=shutil.ignore_patterns(".truerenderer-cache"))
    (run / "reports").mkdir()
    process = subprocess.run([str(binary), "--filesystem-smoke", "--root", str(run), "--data", str(run / "data")], capture_output=True, text=True, timeout=120)
    (run / "native.stdout.log").write_text(process.stdout)
    (run / "native.stderr.log").write_text(process.stderr)
    result = json.loads((run / "reports/filesystem-ui.json").read_text())
    report = {
        "application": "TrueRenderer", "checked_at": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(), "machine": platform.machine(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "worktree": "local navigator implementation; see STATO.md",
        "bundle": str(bundle.relative_to(ROOT)),
        "binary_sha256": {str(path.relative_to(bundle)): hashlib.sha256(path.read_bytes()).hexdigest() for path in [binary, bundle / "Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder", bundle / "Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder"]},
        "passed": process.returncode == 0 and result["passed"], "ui": result,
        "evidence": str(run.relative_to(ROOT)),
        "not_qualified": ["Windows native UI/filesystem", "VoiceOver/NVDA", "NAS blocked syscalls", "cloud providers", "removable-volume identity across remount", "100-sample latency/low-memory campaign"],
    }
    target = ROOT / args.report
    target.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["passed"]:
        raise SystemExit("Filesystem native smoke failed; inspect local evidence")


if __name__ == "__main__":
    main()
