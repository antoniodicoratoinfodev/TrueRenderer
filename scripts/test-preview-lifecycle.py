#!/usr/bin/env python3
"""Repeat native startup and view transitions; record failures instead of retrying them away."""
import argparse
import json
from pathlib import Path
import plistlib
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', default='dist/TrueRenderer.app')
    parser.add_argument('--runs', type=int, default=20)
    args = parser.parse_args()
    assert 1 <= args.runs <= 100
    bundle = (ROOT / args.bundle).resolve()
    assert bundle.suffix == '.app' and bundle.is_relative_to(ROOT)
    version = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString']
    workspace = ROOT / 'var/preview-validation'
    workspace.mkdir(parents=True, exist_ok=True)
    cases = []
    try:
        with (workspace / 'lifecycle.log').open('w') as log:
            for index in range(args.runs):
                sampling = index % 4 == 3
                mode = '--sampling-smoke' if sampling else '--settings-smoke'
                name = 'smoke-macos.json' if sampling else 'cache-settings-macos.json'
                started = time.time()
                result = subprocess.run([
                    str(bundle / 'Contents/MacOS/TrueRenderer'), mode,
                    '--data', str(workspace / 'lifecycle-data'),
                ], cwd=ROOT, stdout=log, stderr=log, timeout=90)
                path = ROOT / 'reports' / name
                report = json.loads(path.read_text()) if path.exists() else {}
                passed = (result.returncode == 0 and path.exists() and path.stat().st_mtime >= started
                          and report.get('passed') and report.get('version') == version)
                cases.append({'run': index + 1, 'mode': mode, 'exit_code': result.returncode,
                              'passed': bool(passed), 'seconds': time.time() - started})
                print(f'Native lifecycle {index + 1}/{args.runs}: {passed}', flush=True)
                assert passed, 'Read var/preview-validation/lifecycle.log'
    finally:
        report = {'application': 'TrueRenderer', 'version': version,
                  'passed': len(cases) == args.runs and all(c['passed'] for c in cases),
                  'requested_runs': args.runs, 'cases': cases,
                  'scope': 'Repeated native startup plus grid/viewer/compare/zoom transitions on generated corpus; no automatic retries after a failure. Does not simulate device loss or qualify all window/display changes.'}
        (ROOT / 'reports/preview-lifecycle-macos.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
