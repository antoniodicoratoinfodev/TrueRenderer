#!/usr/bin/env python3
"""Sample the candidate host and its embedded XPCs, using native libproc counters."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import plistlib
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def summarize(samples, limit, exit_code, timed_out=False):
    """A failed workload is a failed measurement, even when its peak is small."""
    peak_rss = max((s[1] for s in samples), default=0)
    peak_footprint = max((s[2] for s in samples), default=0)
    processes = max((s[3] for s in samples), default=0)
    incomplete = sum(s[4] != 0 for s in samples)
    return {
        'passed': bool(samples) and processes >= 2 and incomplete == 0
                  and exit_code == 0 and not timed_out
                  and max(peak_rss, peak_footprint) <= limit,
        'workload_exit_code': exit_code,
        'timed_out': timed_out,
        'incomplete_samples': incomplete,
        'host_and_xpc_observed': processes >= 2,
        'limit_bytes': limit,
        'peak_sum_rss_bytes': peak_rss,
        'peak_sum_physical_footprint_bytes': peak_footprint,
        'maximum_processes_in_sample': processes,
        'sample_count': len(samples),
        'maximum_sample_gap_seconds': max((b[0] - a[0] for a, b in zip(samples, samples[1:])), default=0),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', default='var/preview-validation/TrueRenderer.app')
    parser.add_argument('--mode', choices=['cache', 'viewer', 'real-raw'], default='cache')
    parser.add_argument('--raw-folder', type=Path)
    parser.add_argument('--memory-mib', type=int, default=2048)
    parser.add_argument('--report', type=Path,
                        help='Optional JSON report under project reports/ or var/')
    args = parser.parse_args()
    assert 512 <= args.memory_mib <= 65536
    if args.mode == 'real-raw':
        assert args.raw_folder and args.raw_folder.is_dir(), 'Explicit authorized RAW folder required'
    bundle = (ROOT / args.bundle).resolve()
    assert bundle.suffix == '.app' and bundle.is_relative_to(ROOT)
    report_path = (ROOT / (args.report or f'reports/preview-memory-{args.mode}-macos.json')).resolve()
    assert report_path.suffix == '.json' and any(
        report_path.is_relative_to(ROOT / directory) for directory in ('reports', 'var'))
    report_path.parent.mkdir(parents=True, exist_ok=True)
    binary_names = ['Contents/MacOS/TrueRenderer', 'Contents/MacOS/tr-worker'] + [
        f'Contents/XPCServices/Decoder{slot}.xpc/Contents/MacOS/Decoder' for slot in range(2)]
    metadata = {
        'application': 'TrueRenderer',
        'version': plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString'],
        'recorded_at': datetime.now(timezone.utc).isoformat(),
        'mode': args.mode,
        'binary_sha256': {name: hashlib.sha256((bundle / name).read_bytes()).hexdigest()
                          for name in binary_names},
    }
    # Never let an interrupted/failed run masquerade as the previous success.
    report_path.write_text(json.dumps({**metadata, 'passed': False, 'status': 'running'}, indent=2) + '\n')
    workspace = ROOT / 'var/preview-validation'
    workspace.mkdir(parents=True, exist_ok=True)
    # An already-open instance may execute from the same bundle path. A unique
    # copy isolates this workload without dropping any of its embedded services
    # from accounting or terminating someone else's application.
    run_folder = Path(tempfile.mkdtemp(prefix='memory-run-', dir=workspace))
    isolated = run_folder / 'TrueRenderer.app'
    shutil.copytree(bundle, isolated)
    bundle = isolated
    metadata['isolated_bundle'] = True
    library = workspace / 'memory-probe.dylib'
    subprocess.run([
        'xcrun', 'clang', '-dynamiclib', '-O2', '-Wall', '-Wextra', '-Werror',
        str(ROOT / 'scripts/preview-memory-probe.c'), '-o', str(library),
    ], check=True)
    probe = ctypes.CDLL(str(library)).tr_preview_usage
    probe.argtypes = [ctypes.c_char_p] + [ctypes.POINTER(ctypes.c_uint64)] * 4
    command = [str(bundle / 'Contents/MacOS/TrueRenderer'),
               '--verify-previews' if args.mode == 'cache' else '--formats-smoke']
    if args.mode == 'viewer':
        command += ['--data', str(run_folder / 'data')]
    elif args.mode == 'real-raw':
        command = [str(bundle / 'Contents/MacOS/TrueRenderer'), '--verify-real-raws',
                   str(args.raw_folder.resolve()), '--raw-memory-mib', str(args.memory_mib)]
    command += ['--root', str(ROOT)]
    samples = []
    peak_processes = []
    peak_observed = 0
    timed_out = False
    start = time.monotonic()
    with (workspace / 'memory-run.log').open('w') as log:
        process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=log)
        while process.poll() is None:
            rss, footprint, count = (ctypes.c_uint64() for _ in range(3))
            details = (ctypes.c_uint64 * (128 * 4))()
            status = probe(str(bundle).encode(), ctypes.byref(rss),
                           ctypes.byref(footprint), ctypes.byref(count), details)
            if max(rss.value, footprint.value) > peak_observed:
                peak_observed = max(rss.value, footprint.value)
                peak_processes = [dict(pid=details[i*4], rss_bytes=details[i*4+1],
                                       footprint_bytes=details[i*4+2],
                                       role=['host', 'decoder0', 'decoder1'][details[i*4+3]])
                                  for i in range(min(count.value, 128))]
            samples.append([time.monotonic() - start, rss.value,
                            footprint.value, count.value, status])
            if time.monotonic() - start > (900 if args.mode == 'real-raw' else 180):
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                timed_out = True
                break
            time.sleep(.025)
    limit = args.memory_mib * 1024**2 if args.mode == 'real-raw' else 2 * 1024**3
    report = {
        **metadata,
        **summarize(samples, limit, process.returncode, timed_out),
        'status': 'completed',
        'processes_at_peak': peak_processes,
        'elapsed_seconds': time.monotonic() - start,
        'scope': ('Sampled host plus all embedded XPC processes in a unique copy of the test bundle, '
                  'excluding unrelated already-open app instances by executable path, '
                  'using libproc RSS and physical footprint; '
                  'sums can count shared pages twice. Includes process-attributed unified GPU memory, not independently '
                  'measured driver/system allocations or sub-sampling peaks. '
                  + ('Authorized real RAW copies; explicit test memory budget, user settings unchanged. '
                     if args.mode == 'real-raw' else 'Generated fixtures only. ')
                  + 'Not the 1000-real-RAW qualification.'),
    }
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    assert report['passed'], 'Workload failed, timed out, exceeded budget or accounting is incomplete; see saved report and var/preview-validation/memory-run.log'


if __name__ == '__main__':
    main()
