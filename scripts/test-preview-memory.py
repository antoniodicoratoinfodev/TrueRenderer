#!/usr/bin/env python3
"""Sample the candidate host and its embedded XPCs, using native libproc counters."""
import argparse
import ctypes
import json
import plistlib
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', default='var/preview-validation/TrueRenderer.app')
    parser.add_argument('--mode', choices=['cache', 'viewer'], default='cache')
    args = parser.parse_args()
    bundle = (ROOT / args.bundle).resolve()
    assert bundle.suffix == '.app' and bundle.is_relative_to(ROOT)
    workspace = ROOT / 'var/preview-validation'
    workspace.mkdir(parents=True, exist_ok=True)
    library = workspace / 'memory-probe.dylib'
    subprocess.run([
        'xcrun', 'clang', '-dynamiclib', '-O2', '-Wall', '-Wextra', '-Werror',
        str(ROOT / 'scripts/preview-memory-probe.c'), '-o', str(library),
    ], check=True)
    probe = ctypes.CDLL(str(library)).tr_preview_usage
    probe.argtypes = [ctypes.c_char_p] + [ctypes.POINTER(ctypes.c_uint64)] * 3
    command = [str(bundle / 'Contents/MacOS/TrueRenderer'),
               '--verify-previews' if args.mode == 'cache' else '--formats-smoke']
    if args.mode == 'viewer':
        command += ['--data', str(workspace / 'memory-data')]
    samples = []
    start = time.monotonic()
    with (workspace / 'memory-run.log').open('w') as log:
        process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=log)
        while process.poll() is None:
            rss, footprint, count = (ctypes.c_uint64() for _ in range(3))
            status = probe(str(bundle).encode(), ctypes.byref(rss),
                           ctypes.byref(footprint), ctypes.byref(count))
            samples.append([time.monotonic() - start, rss.value,
                            footprint.value, count.value, status])
            if time.monotonic() - start > 180:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise RuntimeError('Native preview test timed out')
            time.sleep(.025)
    assert process.returncode == 0, 'Read var/preview-validation/memory-run.log'
    assert samples and max(s[3] for s in samples) >= 2, 'Missing host/XPC accounting'
    limit = 2 * 1024**3
    peak_rss = max(s[1] for s in samples)
    peak_footprint = max(s[2] for s in samples)
    report = {
        'application': 'TrueRenderer',
        'version': plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString'],
        'mode': args.mode,
        'passed': max(peak_rss, peak_footprint) <= limit and all(s[4] == 0 for s in samples),
        'incomplete_samples': sum(s[4] != 0 for s in samples),
        'limit_bytes': limit,
        'peak_sum_rss_bytes': peak_rss,
        'peak_sum_physical_footprint_bytes': peak_footprint,
        'maximum_processes_in_sample': max(s[3] for s in samples),
        'sample_count': len(samples),
        'maximum_sample_gap_seconds': max((b[0] - a[0] for a, b in zip(samples, samples[1:])), default=0),
        'elapsed_seconds': time.monotonic() - start,
        'scope': ('Sampled host plus embedded XPC processes in this test bundle, using libproc RSS and physical footprint; '
                  'sums can count shared pages twice. Includes process-attributed unified GPU memory, not independently '
                  'measured driver/system allocations or sub-sampling peaks. Generated fixtures only; not the 1000-real-RAW qualification.'),
    }
    (ROOT / f'reports/preview-memory-{args.mode}-macos.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    assert report['passed'], 'Observed process peak exceeds 2 GiB or process accounting is incomplete'


if __name__ == '__main__':
    main()
