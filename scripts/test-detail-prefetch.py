#!/usr/bin/env python3
"""Compare first zoom after a fixed Fit dwell, with active detail on/off.

Each run uses copied photos, a fresh catalogue/cache and the same executable.
No screenshots/readbacks during timed gestures unless --correctness is set.
The window is visible to avoid Windows background-window throttling.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--photo", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--engine", default="LibRawBilinear")
    parser.add_argument("--compute", choices=["Automatic", "Gpu", "Cpu"], default="Automatic")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--mode", choices=["both", "on", "off"], default="both")
    parser.add_argument("--correctness", action="store_true")
    parser.add_argument("--dwell-ms", type=int, default=6000)
    parser.add_argument("--require-ready", action="store_true",
                        help="Require native source and edited result before zoom in enabled runs")
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if not 0 <= args.dwell_ms <= 30000:
        parser.error("--dwell-ms must be between 0 and 30000")
    repo = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve()
    worker = binary.with_name("tr-worker.exe" if os.name == "nt" else "tr-worker")
    assert binary.is_file() and worker.is_file()
    args.output.mkdir(parents=True, exist_ok=False)
    digest = sha(args.photo)
    manifest = {"binary_sha256": sha(binary), "worker_sha256": sha(worker),
                "photo_sha256": digest, "engine": args.engine, "compute": args.compute,
                "correctness": args.correctness, "fixed_fit_observation_ms": args.dwell_ms,
                "require_ready": args.require_ready, "runs": []}
    env = os.environ.copy()
    env["TR_WORKER_BINARY"] = str(worker)
    env["TR_RENDER_TIMINGS"] = "0"
    startup = None
    expected_viewport = None
    if os.name == "nt":
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 1
    for repeat in range(args.repetitions):
        modes = (["off", "on"] if repeat % 2 == 0 else ["on", "off"]) if args.mode == "both" else [args.mode]
        for mode in modes:
            if shutil.disk_usage(args.output).free < 20 * 1024**3:
                raise SystemExit("Stopped before the next run: less than 20 GiB free on the test volume")
            root = (args.output / f"{repeat + 1}-{mode}").resolve()
            root.mkdir()
            shutil.copytree(repo / "corpus", root / "corpus",
                            ignore=shutil.ignore_patterns(".truerenderer-cache"))
            for directory in ["data", "var", "reports", "photos"]:
                (root / directory).mkdir()
            photo = root / "photos" / ("sample" + args.photo.suffix)
            shutil.copy2(args.photo, photo)
            (root / "data/settings.json").write_text(json.dumps({
                "schema": 3, "quality": "Full", "raw_engine": args.engine,
                "compute": args.compute, "language": "en", "folder_loading": "Background"
            }), encoding="utf-8")
            command = [str(binary), "--root", str(root), "--data", str(root / "data"),
                       "--open", str(photo), "--develop-smoke", "--edit-pixel-smoke",
                       "--zoom-continuity-smoke", "--zoom-from-fit-smoke", "--detail-prefetch-smoke",
                       "--detail-prefetch-dwell-ms", str(args.dwell_ms)]
            if mode == "off":
                command.append("--detail-prefetch-off-smoke")
            if not args.correctness:
                command.append("--zoom-performance-smoke")
            print("START", repeat + 1, mode, flush=True)
            with (root / "stdout.log").open("w") as out, (root / "stderr.log").open("w") as err:
                result = subprocess.run(command, env=env, stdout=out, stderr=err,
                                        startupinfo=startup, timeout=180)
            reports = []
            for name in ["zoom-continuity", "develop-ui"]:
                path = root / "reports" / f"{name}.json"
                reports.append(json.loads(path.read_text(encoding="utf-8")) if path.exists() else {})
            zoom, develop = reports
            before = zoom.get("detail_before_zoom") or {}
            delta = zoom.get("decode_jobs_after_gesture", 0) - before.get("decode_jobs", 0)
            checks = zoom.get("surface_checks", [])
            sizes = [f["viewport_pixels"] for f in zoom.get("frames", [])]
            if sizes and expected_viewport is None:
                expected_viewport = sizes[0]
            geometry_ok = bool(sizes) and all(
                all(abs(a - b) <= 1 for a, b in zip(size, expected_viewport)) for size in sizes)
            passed = bool(result.returncode == 0 and zoom.get("passed") and develop.get("passed")
                          and geometry_ok
                          and before and before["visible_fit_base"] > 0
                          and before["fixed_fit_observation_ms"] == args.dwell_ms
                          and (mode != "off" or not before["native_ready"])
                          and (mode != "on" or before["native_ready"] or before["native_pending"])
                          and (mode != "on" or not args.require_ready or (before["native_ready"] and before["edit_ready"] and delta == 0))
                          and (not args.correctness or len(checks) == 4)
                          and sha(photo) == digest)
            row = {"repeat": repeat + 1, "mode": mode, "passed": passed,
                   "exit_code": result.returncode, "detail_before_zoom": before,
                   "viewport_pixels": sizes[0] if sizes else None, "geometry_consistent": geometry_ok,
                   "first_native_zoom_ms": zoom.get("first_native_zoom_ms"),
                   "new_decode_jobs_after_zoom": delta,
                   "draws": len(zoom.get("frames", [])),
                   "surface_checks": len(checks),
                   "maximum_error_u8": max((c["maximum_error_u8"] for c in checks), default=None)}
            manifest["runs"].append(row)
            (args.output / "results.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
            print("DONE", row, flush=True)
            if not passed:
                raise SystemExit(1)
    assert sha(args.photo) == digest


if __name__ == "__main__":
    main()
