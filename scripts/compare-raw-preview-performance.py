#!/usr/bin/env python3
"""Compare completed private A/B campaigns without publishing photographic identifiers."""
import argparse
from collections import defaultdict
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import statistics


def summarize(report):
    groups = defaultdict(list)
    pixels = {}
    for attempt in report['passes']:
        assert attempt['passed'] and attempt['complete'] and attempt['exit_code'] == 0
        for row in attempt['rows']:
            assert row['passed'] and len(row['pixel_digests']) == row['images']
            key = (attempt['application_cache'], row['engine'], row['quality'], row['phase'])
            groups[key].append(row)
            assert (*key, row['batch']) not in pixels, 'Duplicate benchmark stage'
            pixels[(*key, row['batch'])] = row['pixel_digests']
    return groups, pixels


def verification(root, navigation_path, log_path, hashes):
    ui = json.loads((root / 'var/raw-engine-ui.json').read_text())
    pixels = json.loads((root / 'var/raw-engine-ui-pixels.json').read_text())
    xpc = json.loads((root / 'reports/xpc-qualification-macos.json').read_text())
    isolation = json.loads((root / 'reports/xpc-integration-macos.json').read_text())
    folder = json.loads((root / 'reports/folder-loading.json').read_text())
    grid = json.loads((root / 'reports/smoke-macos.json').read_text())
    navigation = json.loads(navigation_path.read_text())
    assert all(r['passed'] for r in [ui, pixels, xpc, isolation, folder, grid, navigation])
    assert folder['complete'] and folder['foreground_and_background'] and folder['errors'] == 0
    assert grid['images'] == ui['scanned_items'] == folder['total'] == 30
    assert {'07-grid-small', '08-grid-large'} <= set(grid['screenshots']) and len(grid['screenshots']) == 8
    assert navigation['complete'] and len(ui['stages']) == len(pixels['checks']) == 5
    for run in [xpc, navigation]:
        assert all(hashes[name] == digest for name, digest in run['binary_sha256'].items())
    assert len(navigation['cases']) == 8
    assert {(c['engine'], c['mode']) for c in navigation['cases']} == {
        (engine, mode) for engine in ['Apple', 'LibRawBilinear', 'LibRawAhd', 'TrueRenderer']
        for mode in ['Cpu', 'Gpu']}
    cases = []
    for case in navigation['cases']:
        samples = case['result']['samples']
        assert case['passed'] and case['inputs_unchanged'] and len(samples) == 10
        assert all(s['passed'] for s in samples) and samples[8]['physical_1to1_exact']
        cases.append({'engine': case['engine'], 'requested_compute': case['mode'],
                      'passed': True, 'actions': len(samples),
                      'compute_used': sorted({s['compute'] for s in samples}),
                      'compute_statistics': case['result']['compute'],
                      'max_channel_error_u8': max(s['max_channel_error_u8'] for s in samples),
                      'minimum_transition_draw_coverage': min(s['minimum_draw_coverage'] for s in samples[3:]),
                      'physical_1to1_exact': True, 'memory': case['memory']})
    log = log_path.read_text()
    assert 'Native smoke passed' in log and 'Worker protocol: 6 checks passed' in log
    assert 'test result: FAILED' not in log
    ordinary, integrations = log.split('TR_RAW_SAMPLE unset:', 1)
    count = lambda text: sum(map(int, re.findall(r'test result: ok\. (\d+) passed;', text)))
    return {'workspace': {'command': 'scripts/verify.sh --gui', 'passed': True,
                          'ordinary_rust_tests': count(ordinary), 'integration_rust_tests': count(integrations),
                          'worker_protocol_checks': 6, 'private_camera_tests_replaced_by_this_campaign': 2},
            'xpc': {'passed': True, 'two_distinct_processes': isolation['two_distinct_processes'],
                    'other_isolate_survived': isolation['other_isolate_survived'],
                    'corpus_images_checked': isolation['corpus_images_checked'],
                    'same_pixels_after_recovery': isolation['same_pixels'],
                    'production_rejects_fault_injection': xpc['production_rejects_fault_injection'],
                    'full_sandbox_gate_passed': False},
            'native_raw_ui': {'passed': True, 'scanned_images': ui['scanned_items'],
                              'stages': len(ui['stages']), 'filmstrip_and_1to1': True,
                              'surface_pixel_checks': pixels['checks'],
                              'scope': pixels['scope']},
            'native_folder_loading': {name: folder[name] for name in
                                      ['passed', 'complete', 'processed', 'total', 'errors', 'foreground_and_background', 'scope']},
            'native_raw_grid': {'passed': True, 'images': grid['images'],
                                'elapsed_seconds': grid['elapsed_seconds'],
                                'screenshots': sorted(grid['screenshots']),
                                'presentation_errors': grid['presentation_errors'],
                                'scope': 'Visual inspection of small/large RAW grids using the existing eight-stage native sampler. Historical radial filenames are labels only: this run uses private D750 NEFs, not the radial-signal pixel verifier.'},
            'native_navigation': {'passed': True, 'actions': sum(c['actions'] for c in cases),
                                  'cases': cases, 'scope': navigation['scope']}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native-root', type=Path)
    parser.add_argument('--navigation-report', type=Path)
    parser.add_argument('--verification-log', type=Path)
    args = parser.parse_args()
    assert all(v is None for v in [args.native_root, args.navigation_report, args.verification_log]) or all(
        v is not None for v in [args.native_root, args.navigation_report, args.verification_log]), 'Provide all verification inputs'
    baseline = json.loads(args.baseline.read_text())
    candidate = json.loads(args.candidate.read_text())
    assert all(r['passed'] and r['complete'] and r['sources_unchanged'] for r in [baseline, candidate])
    assert baseline['sources'] == candidate['sources'], 'Different corpus sizes'
    old, old_pixels = summarize(baseline)
    new, new_pixels = summarize(candidate)
    assert old.keys() == new.keys() and old_pixels.keys() == new_pixels.keys()
    assert all(len(old_pixels[key]) == len(new_pixels[key]) for key in old_pixels), 'Different stage sizes'
    different = sum(a != b for key in old_pixels for a, b in zip(old_pixels[key], new_pixels[key]))
    cases = []
    for key in old:
        before = sum(r['seconds'] for r in old[key])
        after = sum(r['seconds'] for r in new[key])
        cases.append(dict(zip(['application_cache', 'engine', 'quality', 'phase'], key),
                          images=sum(r['images'] for r in new[key]),
                          before_seconds=before, after_seconds=after,
                          reduction_percent=100 * (1 - after / before),
                          before_first_thumbnail_median_seconds=statistics.median(r['first_thumbnail_seconds'] for r in old[key]),
                          after_first_thumbnail_median_seconds=statistics.median(r['first_thumbnail_seconds'] for r in new[key]),
                          before_all_thumbnails_seconds=sum(r['all_thumbnails_seconds'] for r in old[key]),
                          after_all_thumbnails_seconds=sum(r['all_thumbnails_seconds'] for r in new[key]),
                          before_decode_jobs=sum(r['decode_jobs'] for r in old[key]),
                          after_decode_jobs=sum(r['decode_jobs'] for r in new[key]),
                          before_disk_hits=sum(r['disk_hits'] for r in old[key]),
                          after_disk_hits=sum(r['disk_hits'] for r in new[key])))
    report = {'application': 'TrueRenderer', 'passed': different == 0, 'complete': True,
              'recorded_at': datetime.now(timezone.utc).isoformat(),
              'candidate_platform': candidate.get('os'),
              'sources': candidate['sources'], 'sources_unchanged': True,
              'pixel_comparison_scope': 'All thumbnail reference-mip tails in the three phases, both qualities and all engines; no lossy digest input.',
              'pixel_comparisons': sum(len(v) for v in old_pixels.values()),
              'different_pixel_digests': different,
              'baseline_binary_sha256': baseline['binary_sha256'],
              'candidate_binary_sha256': candidate['binary_sha256'],
              'physical_memory_gate_passed': False,
              'memory': [p.get('memory') for p in candidate['passes']], 'cases': cases,
              'limits': ['One sequential cold/reopened campaign per binary, 30 authorized D750 NEFs; no p95/p99 qualification.',
                         'CPU artifact availability includes queue, hash, decode and filtering; excludes GUI drawing and display.',
                         'OS cache not flushed, frequency/background load not controlled; reopened does not imply every artifact was persisted.',
                         'The headless demand probe uses production UI/service methods; native GUI and 1:1 require separate verification.',
                         'Measured aggregate memory can exceed the configured 2 GiB; this optimization does not close that known limit.',
                         'Native GPU navigation may use the declared CPU fallback under its budget; nonempty transition coverage is not always full coverage.',
                         'Both qualities use the selected full RAW development. No embedded JPEG, reduced demosaic, colour or release qualification.']}
    if args.native_root:
        report['verification'] = verification(args.native_root, args.navigation_report,
                                               args.verification_log, candidate['binary_sha256'])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(args.output)
    assert report['passed'], 'Pixel differences remain'


if __name__ == '__main__':
    main()
