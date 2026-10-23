#!/usr/bin/env python3
"""Native gesture measurements in isolated roots; no pixels/readbacks in gesture.

Use --profile for CPU spans and asynchronous GPU timestamps; compare visible
cadence separately without profiling. Outputs stay under the explicit --output.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess


def percentile(values, fraction):
    values = sorted(values)
    return values[int((len(values) - 1) * fraction)] if values else None


def distribution(values):
    return {"count": len(values), "sum": sum(values),
            "p50": percentile(values, .5), "p95": percentile(values, .95),
            "max": max(values, default=None)}


def metrics(report, gesture):
    frames = report["frames"]
    # Do not charge post-gesture canonical development/export to live updates.
    frames = [f for f in frames if f["step"] < 120]
    visible, last = [], None
    for frame in frames:
        key = (frame.get("sample_step") if gesture == "zoom" else
               frame.get("origin") if gesture == "pan" else
               [frame.get("displayed_source"), frame.get("displayed_revision")])
        if key != last:
            visible.append(frame["elapsed_ms"])
            last = key
    first_ages = {}
    for frame in frames:
        if frame.get("age_ms") is not None:
            first_ages.setdefault(frame["displayed_revision"], frame["age_ms"])
    phases = {}
    for sample in report.get("renderer_timings", []):
        phases.setdefault(sample["phase"], []).append(sample["duration_us"])
    return {
        "draws": len(frames), "distinct_visible_updates": len(visible),
        "draw_interval_ms": distribution([b["elapsed_ms"] - a["elapsed_ms"]
                                           for a, b in zip(frames, frames[1:])]),
        "visible_interval_ms": distribution([b - a for a, b in zip(visible, visible[1:])]),
        "event_to_first_draw_ms": distribution(list(first_ages.values())),
        "event_to_draw_ms": distribution([f["age_ms"] for f in frames if f.get("age_ms") is not None]),
        "phases_us": {k: distribution(v) for k, v in phases.items()},
        "compute": report["compute"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--photo", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--gesture", choices=["zoom", "pan", "edit"], required=True)
    parser.add_argument("--engine", default="LibRawBilinear")
    parser.add_argument("--compute", choices=["Automatic", "Gpu", "Cpu"], default="Automatic")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--profile", action="store_true")
    parser.add_argument("--correctness", action="store_true")
    parser.add_argument("--visible", action="store_true", help="Show the native test window; avoids hidden-window throttling")
    parser.add_argument("--expected-viewport", type=int, nargs=2, metavar=("WIDTH", "HEIGHT"))
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if args.expected_viewport and min(args.expected_viewport) < 1:
        parser.error("--expected-viewport dimensions must be positive")
    repo = Path(__file__).resolve().parents[1]
    args.output.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve()
    packaged_worker = binary.with_name("tr-worker.exe" if os.name == "nt" else "tr-worker")
    assert packaged_worker.is_file(), "Worker must be beside the application"
    assert hashlib.sha256(packaged_worker.read_bytes()).digest() == hashlib.sha256(args.worker.read_bytes()).digest(), "Packaged worker differs from --worker"
    env = os.environ.copy()
    env["TR_WORKER_BINARY"] = str(args.worker.resolve())
    env["TR_RENDER_TIMINGS"] = "1" if args.profile else "0"
    startup = None
    if os.name == "nt":
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 1 if args.visible else 0
    photo = args.output / "photos" / args.photo.name
    photo.parent.mkdir()
    shutil.copy2(args.photo, photo)
    digest = hashlib.sha256(photo.read_bytes()).hexdigest()
    results = []
    for repeat in range(1, args.repetitions + 1):
        root = (args.output / f"run-{repeat}").resolve()
        root.mkdir()
        shutil.copytree(repo / "corpus", root / "corpus",
                        ignore=shutil.ignore_patterns(".truerenderer-cache"))
        for directory in ["data", "var", "reports"]:
            (root / directory).mkdir()
        (root / "data/settings.json").write_text(json.dumps({
            "schema": 3, "raw_engine": args.engine, "quality": "Full",
            "compute": args.compute, "language": "en", "folder_loading": "Background"
        }), encoding="utf-8")
        command = [str(binary), "--root", str(root), "--data", str(root / "data"),
                   "--open", str(photo.resolve()), "--develop-smoke", "--edit-pixel-smoke",
                   "--edit-zoom-smoke", f"--{args.gesture}-continuity-smoke",
                   "--pan-zoom", "1.37" if args.gesture == "pan" else "1.0"]
        if not args.correctness:
            command += ["--interaction-performance-smoke", "--zoom-performance-smoke"]
        print("START", args.output.name, repeat, flush=True)
        with (root / "stdout.log").open("w") as out, (root / "stderr.log").open("w") as err:
            result = subprocess.run(command, env=env, stdout=out, stderr=err,
                                    startupinfo=startup, timeout=180)
        report_path = root / f"reports/{args.gesture}-continuity.json"
        report = json.loads(report_path.read_text()) if report_path.exists() else {}
        development_path = root / "reports/develop-ui.json"
        development = json.loads(development_path.read_text()) if development_path.exists() else {}
        passed = result.returncode == 0 and report.get("passed") and development.get("passed")
        geometry_errors = []
        if report:
            frames = report["frames"]
            sizes = []
            for frame in frames:
                if args.gesture == "zoom":
                    size = frame["viewport_pixels"]
                elif args.gesture == "pan":
                    size = frame["size"]
                else:
                    match = re.search(r"size: \[(\d+), (\d+)\].*step: \[([^,]+), ([^\]]+)\]", frame["region"])
                    assert match, "Missing edit geometry"
                    size = [int(match[1]), int(match[2])]
                    if [float(match[3]), float(match[4])] != [1., 1.]:
                        geometry_errors.append("Edit gesture no longer at native 1:1")
                sizes.append(size)
            # Physical edge snapping can change the visible crop by one pixel.
            if any(any(abs(a - b) > 1 for a, b in zip(size, sizes[0])) for size in sizes):
                geometry_errors.append("Viewport changed during gesture")
            if args.expected_viewport and any(abs(a - b) > 1 for a, b in zip(sizes[0], args.expected_viewport)):
                geometry_errors.append(f"Viewport {sizes[0]} differs from {args.expected_viewport}")
        # A resize does not invalidate correct pixels, but is not comparable
        # timing. Preserve both outcomes instead of treating it as a render bug.
        if not args.correctness:
            passed = passed and not geometry_errors
        if passed and not args.correctness:
            assert not report.get("surface_checks") and not report.get("live_surface_checks")
        row = {"repeat": repeat, "passed": bool(passed), "exit_code": result.returncode,
               "geometry_errors": sorted(set(geometry_errors)),
               "viewport_sizes": sorted({tuple(size) for size in sizes}) if report else [],
               "geometry_tolerance_pixels": 1,
               "metrics": metrics(report, args.gesture) if report else None}
        results.append(row)
        manifest = {"gesture": args.gesture, "engine": args.engine, "compute": args.compute,
                    "profile": args.profile, "correctness": args.correctness, "visible": args.visible,
                    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "photo_sha256": digest, "runs": results}
        (args.output / "results.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        print("DONE", repeat, bool(passed),
              row["metrics"]["visible_interval_ms"] if report else "missing report", flush=True)
        if not passed:
            raise SystemExit(1)
    assert hashlib.sha256(photo.read_bytes()).hexdigest() == digest


if __name__ == "__main__":
    main()
