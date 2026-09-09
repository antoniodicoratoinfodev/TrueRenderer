#!/usr/bin/env python3
"""Native regression for source invalidation and bounded graphics recovery."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', default='dist/TrueRenderer.app')
    args = parser.parse_args()
    bundle = (ROOT / args.bundle).resolve()
    assert bundle.suffix == '.app' and bundle.is_relative_to(ROOT)
    workspace = ROOT / 'var/preview-review'
    workspace.mkdir(parents=True, exist_ok=True)
    binaries = {name: hashlib.sha256((bundle / name).read_bytes()).hexdigest() for name in [
        'Contents/MacOS/TrueRenderer', 'Contents/MacOS/tr-worker',
        'Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder',
        'Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder']}
    cases = []
    report = {'passed': False, 'status': 'running',
              'recorded_at': datetime.now(timezone.utc).isoformat(),
              'cases': cases, 'binary_sha256': binaries,
              'scope': 'Generated sources only; real device.destroy callbacks with one recovery '
                       'and stop on second loss. Does not qualify physical driver resets or OOM.'}
    report_path = ROOT / 'reports/preview-review-native-macos.json'
    def save_report():
        report_path.write_text(json.dumps(report, indent=2) + '\n')
    save_report()
    for flag, name, expected_exit in [
        ('--source-change-smoke', 'preview-source-changes-macos.json', 0),
        ('--device-loss-smoke', 'preview-device-recovery-macos.json', 0),
        ('--device-loss-twice-smoke', 'preview-device-recovery-bounded-macos.json', 1),
    ]:
        data = workspace / flag.lstrip('-')
        started = time.time()
        case = {'case': flag, 'passed': False, 'report': name, 'stage': 'launch'}
        cases.append(case)
        save_report()
        path = ROOT / 'reports' / name
        path.write_text(json.dumps({'passed': False, 'status': 'running'}) + '\n')
        try:
            with (workspace / (flag.lstrip('-') + '.log')).open('w') as log:
                result = subprocess.run([str(bundle / 'Contents/MacOS/TrueRenderer'), flag,
                                         '--data', str(data)], stdout=log, stderr=log,
                                        cwd=ROOT, timeout=90)
            case['exit_code'] = result.returncode
            case['stage'] = 'native-report'
            native_report = json.loads(path.read_text())
            assert result.returncode == expected_exit, f'{flag}: exit {result.returncode}'
            assert path.stat().st_mtime >= started and native_report.get('passed'), flag
            case['stage'] = 'library-integrity'
            wal = data / 'library.sqlite-wal'
            assert not wal.exists() or wal.stat().st_size == 0, 'Library WAL not checkpointed at shutdown'
            with sqlite3.connect(f'file:{data / "library.sqlite"}?mode=ro&immutable=1', uri=True) as db:
                assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
            case.update(passed=True, stage='completed', library_integrity='ok')
        except Exception as error:
            # Detailed exception/logs stay local; public reports contain no source paths.
            case['failure_type'] = type(error).__name__
            print(f'{flag}: failed at {case["stage"]}: {error}', flush=True)
        finally:
            case['elapsed_seconds'] = time.time() - started
            save_report()
        if case['passed']:
            print(f'{flag}: passed', flush=True)
    report['status'] = 'completed'
    report['passed'] = all(case['passed'] for case in cases)
    save_report()
    assert report['passed'], 'Native review failed; inspect the current aggregate report'



if __name__ == '__main__':
    main()
