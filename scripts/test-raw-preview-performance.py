#!/usr/bin/env python3
"""Measure production grid/viewer demand on private copies of authorized RAWs."""
import argparse
import ctypes
import hashlib
import json
import platform
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--raw-folder', type=Path, required=True)
    parser.add_argument('--passes', type=int, choices=[1, 2], default=2)
    args = parser.parse_args()
    originals = sorted(path for path in args.raw_folder.iterdir()
                       if path.is_file() and path.suffix.lower() == '.nef')
    assert originals, 'No authorized NEF files'
    before = [digest(path) for path in originals]
    root = Path(tempfile.mkdtemp(prefix='raw-performance-', dir=ROOT / 'var'))
    folder = root / 'photos'
    folder.mkdir()
    for index, path in enumerate(originals):
        target = folder / f'{index:03}.nef'
        shutil.copyfile(path, target)
        assert digest(target) == before[index]
    shutil.copytree(ROOT / 'corpus', root / 'corpus')
    bundle = root / 'TrueRenderer.app'
    shutil.copytree(args.bundle.resolve(), bundle)
    binary_paths = ['Contents/MacOS/TrueRenderer', 'Contents/MacOS/tr-worker'] + [
        f'Contents/XPCServices/Decoder{i}.xpc/Contents/MacOS/Decoder' for i in range(2)]
    metadata = {'passed': False, 'complete': False, 'sources': len(originals),
                'os': platform.platform(),
                'binary_sha256': {name: digest(bundle / name) for name in binary_paths},
                'scope': 'CPU artifact delivery via production UI demand, including source hash, '
                         'cache verification, decode and filtering. OS cache not flushed; '
                         'no display/compositor timing or statistical percentile qualification.'}
    report = root / 'performance.json'
    report.write_text(json.dumps(metadata, indent=2) + '\n')
    print(root, flush=True)
    library = root / 'memory-probe.dylib'
    subprocess.run(['xcrun', 'clang', '-dynamiclib', '-O2', '-Wall', '-Wextra', '-Werror',
                    str(ROOT / 'scripts/preview-memory-probe.c'), '-o', str(library)], check=True)
    probe = ctypes.CDLL(str(library)).tr_preview_usage
    probe.argtypes = [ctypes.c_char_p] + [ctypes.POINTER(ctypes.c_uint64)] * 4
    results = []
    for attempt in range(args.passes):
        memory = {'peak_sum_rss_bytes': 0, 'peak_sum_footprint_bytes': 0,
                  'incomplete_samples': 0, 'samples': 0, 'maximum_processes': 0,
                  'limit_bytes': 2048 * 1024**2,
                  'scope': 'Host and all its XPCs in this unique bundle; sampled at 50 ms. '
                           'Process sums may count shared pages twice; no display readback.'}
        with (root / f'pass-{attempt}.log').open('w') as log:
            process = subprocess.Popen([str(bundle / 'Contents/MacOS/TrueRenderer'),
                                        '--root', str(root), '--verify-raw-previews', str(folder)],
                                       stdout=log, stderr=log)
            started = time.monotonic()
            while process.poll() is None:
                rss, footprint, count = (ctypes.c_uint64() for _ in range(3))
                details = (ctypes.c_uint64 * (128 * 4))()
                status = probe(str(bundle).encode(), ctypes.byref(rss), ctypes.byref(footprint),
                               ctypes.byref(count), details)
                memory['peak_sum_rss_bytes'] = max(memory['peak_sum_rss_bytes'], rss.value)
                memory['peak_sum_footprint_bytes'] = max(memory['peak_sum_footprint_bytes'], footprint.value)
                memory['maximum_processes'] = max(memory['maximum_processes'], count.value)
                memory['incomplete_samples'] += int(status != 0)
                memory['samples'] += 1
                if time.monotonic() - started > 3600:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise RuntimeError(f'Benchmark timed out: {root}')
                time.sleep(.05)
        result = json.loads((root / 'raw-previews.json').read_text())
        result['application_cache'] = 'cold' if attempt == 0 else 'reopened'
        result['exit_code'] = process.returncode
        memory['within_configured_budget'] = max(memory['peak_sum_rss_bytes'], memory['peak_sum_footprint_bytes']) <= memory['limit_bytes']
        result['memory'] = memory
        results.append(result)
        metadata['passes'] = results
        report.write_text(json.dumps(metadata, indent=2) + '\n')
        assert process.returncode == 0 and result['passed'], root / f'pass-{attempt}.log'
    metadata['sources_unchanged'] = before == [digest(path) for path in originals]
    metadata['complete'] = True
    metadata['passed'] = metadata['sources_unchanged'] and all(r['passed'] for r in results)
    report.write_text(json.dumps(metadata, indent=2) + '\n')
    print(report, flush=True)
    assert metadata['passed']


if __name__ == '__main__':
    main()
