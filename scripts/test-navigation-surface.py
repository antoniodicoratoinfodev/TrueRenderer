#!/usr/bin/env python3
"""Run generated-corpus selections in isolated cold, reopened-SSD and RAM states.

This measures command-to-encoded-raster and command-to-surface-readback, not
physical display presentation. The short default checks the harness; it is not
a p95 qualification. Each cold/SSD sample uses a fresh application process.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]


def quantile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def summarize(rows):
    values = [r["event_to_readback_ms"] for r in rows]
    return {
        "samples": len(values), "minimum_ms": min(values),
        "median_ms": quantile(values, 0.5), "maximum_ms": max(values),
        # Do not imply a tail estimate from a functional smoke run.
        "p95_ms": quantile(values, 0.95) if len(values) >= 100 else None,
        "p99_ms": quantile(values, 0.99) if len(values) >= 1000 else None,
        "scope": "Descriptive readback latency only; no confidence interval or performance gate claim",
    }


def run_probe(bundle, root, mode, quality, phase, transitions=False, require_full_coverage=False, memory_mib=2048, folder=None, targets=None, pressure=False, raw_engine="Apple", allow_fallback=False, allow_redecode=False):
    data = root / (mode + "-" + phase)
    data.mkdir()
    (data / "settings.json").write_text(json.dumps({
        "schema": 2, "quality": quality, "compute": mode,
        "prefetch": "Disabled", "adapt_on_battery": False,
        "memory_mib": memory_mib, "reusable_mib": 512, "raw_engine": raw_engine,
    }))
    report = root / "reports/navigation-surface.json"
    report.write_text('{"passed":false,"error":"not started"}')
    extra = []
    if folder:
        extra += ["--navigation-folder", str(folder)]
    if targets:
        extra += ["--navigation-first", targets[0], "--navigation-second", targets[1]]
    if pressure:
        extra += ["--navigation-pressure"]
    result = subprocess.run([
        str(bundle / "Contents/MacOS/TrueRenderer"), "--navigation-transitions-smoke" if transitions else "--navigation-smoke",
        "--root", str(root), "--data", str(data),
    ] + extra, capture_output=True, text=True, timeout=210)
    (root / (mode + "-" + phase + ".log")).write_text(result.stdout + result.stderr)
    output = json.loads(report.read_text())
    (root / "reports" / (mode + "-" + phase + ".json")).write_text(json.dumps(output, indent=2))
    if result.returncode or not output.get("passed"):
        raise RuntimeError(f"{mode}/{phase} failed: {output.get('error')}; local log: {root}")
    first, _, returned = output["samples"][:3]
    if len(output["samples"]) != (10 if transitions else 3):
        raise RuntimeError("Incomplete navigation trace")
    if transitions:
        if require_full_coverage and any(row["minimum_draw_coverage"] < 1. - 1e-6 for row in output["samples"][3:]):
            raise RuntimeError("Same-source transition lost full image coverage")
        if not any(row["reprojected_redraws"] > 0 for row in output["samples"][3:]):
            raise RuntimeError("Trace did not exercise reprojected content")
        if any(row["redraws"] < 1 or row["minimum_draw_coverage"] <= 0 for row in output["samples"][3:]):
            raise RuntimeError("Missing continuity observations or empty image draw")
        if output["samples"][8]["physical_1to1_exact"] is not True:
            raise RuntimeError("Physical 1:1 pixels or geometry differ from the reference")
        if [output["samples"][i]["effective_quality"] for i in [6,7,8,9]] != ["Standard","Full","Full","Standard"]:
            raise RuntimeError("Photo quality action did not take effect")
    if first["resident_at_command"] or (first["compute"] != mode.upper() and not (allow_fallback and mode == "Gpu" and first["compute"] == "CPU")):
        raise RuntimeError("First access unexpectedly resident or compute fallback occurred")
    if phase == "cold" and first["decode_jobs"] == 0:
        raise RuntimeError("Cold application cache did not decode")
    if phase == "ssd" and not allow_redecode and (first["cache_hits"] == 0 or first["decode_jobs"] != 0):
        raise RuntimeError("Reopened SSD sample did not reuse the persisted artifact")
    if not returned["resident_at_command"] or returned["decode_jobs"] != 0:
        raise RuntimeError("RAM return was not resident")
    mismatched = [row for row in output["samples"] if row["compute"] != mode.upper()]
    if mismatched and not (allow_fallback and mode == "Gpu"
                           and all(row["compute"] == "CPU" for row in mismatched)
                           and output["compute"]["fallbacks"] > 0
                           and output["compute"]["last_fallback"]):
        raise RuntimeError("Requested compute mode did not produce every captured frame")
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--quality", choices=["Standard", "Full"], default="Standard")
    parser.add_argument("--transitions", action="store_true", help="append zoom, pan, fit, photo quality and physical 1:1 actions")
    parser.add_argument("--require-full-coverage", action="store_true", help="require complete image draw coverage for the extended trace")
    args = parser.parse_args()
    if args.require_full_coverage and not args.transitions:
        parser.error("--require-full-coverage requires --transitions")
    if not 1 <= args.trials <= 1000:
        parser.error("--trials must be in 1..1000")
    bundle = args.bundle.resolve(strict=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    work = Path(tempfile.mkdtemp(prefix="navigation-surface-", dir=ROOT / "var"))
    print(f"Evidence: {work}", flush=True)
    report = {
        "application": "TrueRenderer", "passed": False,
        "checked_at": datetime.now(timezone.utc).isoformat(),
        "quality": args.quality, "trials_per_mode": args.trials,
        "transitions": args.transitions,
        "require_full_coverage": args.require_full_coverage,
        "os": platform.platform(), "machine": platform.machine(),
        "physical_memory_bytes": int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)),
        "power_state": subprocess.check_output(["pmset", "-g", "batt"], text=True).strip(),
        "binary_sha256": {str(p.relative_to(bundle)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in [bundle / "Contents/MacOS/TrueRenderer",
                      bundle / "Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder",
                      bundle / "Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder"]},
        "corpus_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted((ROOT / "corpus").glob("*.png"))},
        "runs": [], "summary": {},
        "limits": [
            "Synthetic Command::Select boundary, not OS input injection; no compositor/scanout timestamp.",
            "Readback includes GPU copy/map and event delivery. CPU reference comparison follows timestamp.",
            "Probe requests repaint after 10 ms while active; instrumentation can affect scheduling and is not a transparent production trace.",
            "No app image demand before first command; listing and GPU qualification excluded from latency.",
            "OS file cache not flushed; SSD means reopened application artifact storage, not physical disk cache cold.",
            "CPU/GPU order alternates per trial; SSD necessarily follows its cold population run.",
            "RAM sample is the third selection after visiting another image, in the same process as the SSD run.",
            "Two generated PNG sources below 2048 pixels; even the extended trace does not qualify reduced-resolution RAW, scrolling, disabled cache or pressure.",
            "Extended continuity measures image draw coverage per UI redraw, not all physical screen refreshes. Partial coverage is reported; zero image coverage fails.",
            "Fewer than 100 independent process trials suppress p95; fewer than 1000 suppress p99. No CI or acceleration claim.",
        ],
    }
    destination = work / "navigation-surface-macos.json"
    destination.write_text(json.dumps(report, indent=2) + "\n")
    try:
        for trial in range(args.trials):
            modes = ["Cpu", "Gpu"] if trial % 2 == 0 else ["Gpu", "Cpu"]
            for mode in modes:
                root = work / f"trial-{trial}-{mode}"
                root.mkdir()
                (root / "reports").mkdir()
                shutil.copytree(ROOT / "corpus", root / "corpus",
                                ignore=shutil.ignore_patterns(".truerenderer-cache"))
                for phase in ["cold", "ssd"]:
                    output = run_probe(bundle, root, mode, args.quality, phase, args.transitions, args.require_full_coverage)
                    report["runs"].append({"trial": trial, "mode": mode, "phase": phase, "result": output})
                    print(f"Trial {trial + 1}/{args.trials} {mode} {phase}: passed", flush=True)
                    destination.write_text(json.dumps(report, indent=2) + "\n")
                for name, digest in report["corpus_sha256"].items():
                    if hashlib.sha256((root / "corpus" / name).read_bytes()).hexdigest() != digest:
                        raise RuntimeError("Generated source changed during measurement")
        for mode in ["Cpu", "Gpu"]:
            for phase, step in [("cold", 0), ("ssd", 0), ("ram", 2)]:
                rows = [run["result"]["samples"][step] for run in report["runs"]
                        if run["mode"] == mode and run["phase"] == ("ssd" if phase == "ram" else phase)]
                report["summary"][mode + "-" + phase] = summarize(rows)
        # Matched geometry/quality is required before describing this as A/B.
        geometries = {json.dumps({
            "adapter":run["result"]["adapter"], "surface":run["result"]["surface"],
            "dpi":run["result"]["pixels_per_point"],
            "regions":[{key:row[key] for key in ["source", "size", "origin", "step_size", "source_dimensions"]}
                       for row in run["result"]["samples"]],
        }, sort_keys=True) for run in report["runs"]}
        if len(geometries) != 1:
            raise RuntimeError("Surface geometries differ between runs")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        destination.write_text(json.dumps(report, indent=2) + "\n")
        print(f"Report: {destination}", flush=True)


if __name__ == "__main__":
    main()
