#!/usr/bin/env python3
"""Prepare an isolated folder, then visit every edited photo at Fit, 1:1 and pan.

Cold and warm launches share only this test's copied photos, catalogue and cache.
No screenshots or readbacks during readiness timing. Exact pixels are checked by
the canonical-cache unit tests and the separate development surface probes.
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
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--photo', type=Path, action='append', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--engine', default='LibRawBilinear')
    parser.add_argument('--compute', choices=['Automatic', 'Cpu', 'Gpu'], default='Automatic')
    parser.add_argument('--quality', choices=['Standard', 'Full'], default='Full')
    parser.add_argument('--loading', choices=['Foreground', 'Background'], default='Foreground')
    parser.add_argument('--disk-mib', type=int, default=4096)
    parser.add_argument('--no-cache', action='store_true')
    parser.add_argument('--allow-recomputation', action='store_true')
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve()
    worker = binary.with_name('tr-worker.exe' if os.name == 'nt' else 'tr-worker')
    assert worker.is_file()
    shutil.copytree(repo / 'corpus', root / 'corpus', ignore=shutil.ignore_patterns('.truerenderer-cache'))
    for name in ['data', 'photos', 'reports', 'var']:
        (root / name).mkdir()
    sources = {}
    for index, original in enumerate(args.photo):
        photo = root / 'photos' / (str(index) + original.suffix)
        shutil.copy2(original, photo)
        sources[str(photo)] = sha(original)
    (root / 'data/settings.json').write_text(json.dumps({
        'schema': 3, 'quality': args.quality, 'raw_engine': args.engine,
        'compute': args.compute, 'language': 'en', 'folder_loading': args.loading,
        'enabled': not args.no_cache, 'disk_mib': args.disk_mib,
    }), encoding='utf-8')
    env = os.environ.copy()
    env['TR_WORKER_BINARY'] = str(worker)
    env['TR_RENDER_TIMINGS'] = '0'
    startup = None
    if os.name == 'nt':
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 1
    result = {'binary_sha256': sha(binary), 'worker_sha256': sha(worker),
              'source_sha256': list(sources.values()), 'engine': args.engine,
              'quality': args.quality, 'compute': args.compute, 'loading': args.loading,
              'disk_mib': args.disk_mib, 'cache_enabled': not args.no_cache, 'runs': []}
    for phase in ['cold', 'warm']:
        if shutil.disk_usage(root).free < 20 * 1024**3:
            raise SystemExit('Less than 20 GiB free; stopped before next native launch')
        command = [str(binary), '--root', str(root), '--data', str(root / 'data'),
                   '--open', str(root / 'photos'), '--folder-readiness-smoke']
        print('START', args.output.name, phase, flush=True)
        report_path = root / 'reports/folder-readiness.json'
        report_path.write_text('{"passed":false,"reason":"incomplete"}', encoding='utf-8')
        with (root / (phase + '.stdout.log')).open('w') as out, (root / (phase + '.stderr.log')).open('w') as err:
            process = subprocess.run(command, env=env, stdout=out, stderr=err,
                                     startupinfo=startup, timeout=620)
        report = json.loads(report_path.read_text(encoding='utf-8'))
        shutil.copy2(report_path, root / 'reports' / ('folder-readiness-' + phase + '.json'))
        unchanged = all(sha(Path(p)) == digest for p, digest in sources.items())
        reused = report.get('new_decodes_after_loading') == 0 and report.get('new_edit_renders_after_loading') == 0
        warm_reuse = phase != 'warm' or (report.get('prepared', {}).get('decode_jobs') == 0
                      and report.get('prepared', {}).get('editing', {}).get('renders') == 0)
        passed = process.returncode == 0 and report.get('passed') and unchanged and (
            args.allow_recomputation or (reused and warm_reuse))
        row = {'phase': phase, 'passed': bool(passed), 'exit_code': process.returncode,
               'sources_unchanged': unchanged, 'reused_after_loading': reused,
               'warm_preparation_reused': warm_reuse, 'report': report}
        result['runs'].append(row)
        result['passed'] = len(result['runs']) == 2 and all(r['passed'] for r in result['runs'])
        (root / 'results.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
        print('DONE', phase, bool(passed), report.get('prepared'),
              'new edits', report.get('new_edit_renders_after_loading'), flush=True)
        if not passed:
            raise SystemExit(1)
    for original, digest in zip(args.photo, sources.values()):
        assert sha(original) == digest


if __name__ == '__main__':
    main()
