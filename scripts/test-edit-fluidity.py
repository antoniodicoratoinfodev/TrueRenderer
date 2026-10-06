#!/usr/bin/env python3
"""Isolated macOS editing campaign; generated DNG inputs, no user library."""
from pathlib import Path
import argparse
import hashlib
import itertools
import json
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
ENGINES = ["Apple", "LibRawBilinear", "LibRawAhd", "TrueRenderer"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--fixtures", required=True, type=Path)
    parser.add_argument("--matrix", choices=["quick", "common", "zoom", "wb", "heavy", "large"], required=True)
    args = parser.parse_args()
    bundle, root, fixtures = args.bundle.resolve(), args.root.resolve(), args.fixtures.resolve()
    if not root.is_relative_to(ROOT / "var") or root.exists():
        raise SystemExit("Use a new isolated campaign root under var/.")
    executable = bundle / "Contents/MacOS/TrueRenderer"
    sizes = {"quick": "4000x3000", "common": "4000x3000", "zoom": "6000x4000", "wb": "4000x3000", "heavy": "6000x4000", "large": "8256x5504"}
    fixture = fixtures / f"bayer-{sizes[args.matrix]}.dng"
    source_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
    cases = list(itertools.product(ENGINES, ["Standard", "Full"], ["Cpu", "Gpu"]))
    if args.matrix == "quick":
        cases = [("Apple", "Standard", "Gpu"), ("LibRawAhd", "Full", "Cpu")]
    elif args.matrix in ("wb", "heavy"):
        cases = list(itertools.product(ENGINES, ["Standard", "Full"], ["Gpu"]))
    elif args.matrix == "large":
        cases = list(itertools.product(ENGINES, ["Full"], ["Cpu", "Gpu"]))
    root.mkdir(parents=True)
    report = {"matrix": args.matrix, "binary_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "fixture_sha256": source_hash, "fixture_size": sizes[args.matrix], "cases": [],
              "scope": "Generated Bayer DNG; event to native draw commands, not physical display latency. RSS sums may double-count shared pages. Each case has its own new library and cache."}
    for engine, quality, compute in cases:
        name = f"{engine}-{quality}-{compute}"
        case = root / name
        for directory in ("reports", "data", "inputs"):
            (case / directory).mkdir(parents=True)
        shutil.copytree(ROOT / "corpus", case / "corpus",
                        ignore=shutil.ignore_patterns(".truerenderer-cache", ".DS_Store"))
        # Independent generated input prevents cross-case cache reuse.
        image = case / "inputs" / fixture.name
        shutil.copyfile(fixture, image)
        settings = {"schema": 3, "presentation": "Compatible8", "language": "it",
                    "raw_engine": engine, "quality": quality, "compute": compute,
                    "profile": "Performance", "adapt_on_battery": False, "cpu_threads": 0,
                    "memory_mib": 7630, "gpu_mib": 1024, "enabled": False}
        (case / "data/settings.json").write_text(json.dumps(settings))
        command = [str(executable), "--root", str(case), "--data", str(case / "data"),
                   "--open", str(image), "--develop-smoke", "--edit-continuity-smoke"]
        if args.matrix == "wb":
            command.append("--edit-wb-continuity-smoke")
        if args.matrix in ("zoom", "large"):
            command.append("--edit-zoom-smoke")
        if args.matrix == "heavy":
            command.append("--advanced-edit-smoke")
        peak_rss_kib, start = 0, time.monotonic()
        with (case / "native.log").open("w") as log:
            process = subprocess.Popen(command, stdout=log, stderr=log)
            while process.poll() is None:
                listing = subprocess.run(["/bin/ps", "-axo", "rss=,comm="], capture_output=True, text=True, check=True).stdout
                rss = sum(int(line.split(None, 1)[0]) for line in listing.splitlines() if str(bundle) in line)
                peak_rss_kib = max(peak_rss_kib, rss)
                if time.monotonic() - start > 150:
                    process.terminate()
                    process.wait(timeout=15)
                    break
                time.sleep(0.2)
        def read(name):
            path = case / "reports" / name
            return json.loads(path.read_text()) if path.exists() else {"passed": False, "reason": "missing report"}
        continuity, development, gpu = map(read, ["edit-continuity.json", "develop-ui.json", "preview-quality-gpu-macos.json"])
        unchanged = hashlib.sha256(image.read_bytes()).hexdigest() == source_hash
        configuration_matches = continuity.get("quality") == quality and continuity.get("engine") == engine
        wb_exact = args.matrix != "wb" or development.get("raw_wb_observed") is True
        row = {"engine": engine, "quality": quality, "compute": compute,
               "passed": process.returncode == 0 and continuity.get("passed", False) and development.get("passed", False) and gpu.get("passed", False) and unchanged and configuration_matches and wb_exact,
               "exit_code": process.returncode, "seconds": time.monotonic() - start,
               "peak_bundle_rss_kib": peak_rss_kib, "source_unchanged": unchanged,
               "continuity": continuity, "development": development, "gpu_check": gpu}
        report["cases"].append(row)
        report["passed"] = all(row["passed"] for row in report["cases"]) and len(report["cases"]) == len(cases)
        (root / "campaign.json").write_text(json.dumps(report, indent=2) + "\n")
        print(name, "PASS" if row["passed"] else "FAIL", "updates", continuity.get("distinct_frames_during_gesture"), "seconds", round(row["seconds"], 1), flush=True)
        if not row["passed"]:
            raise SystemExit(f"Inspect {case}; campaign stopped at the first failure.")


if __name__ == "__main__":
    main()
