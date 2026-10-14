#!/usr/bin/env python3
"""Compare frozen renderer-duel reports against mathematical truth, not camera truth."""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path

wb = importlib.import_module('analyze-raw-wb')
color = importlib.import_module('calibrate-raw-profile')


def rms(values):
    return math.sqrt(math.fsum(v*v for v in values)/len(values))


def vector(value, nonnegative=False):
    if not isinstance(value, list) or len(value) != 3:
        raise ValueError('three finite components required')
    numbers = [color.number(v) for v in value]
    if nonnegative and min(numbers) < 0:
        raise ValueError('negative error measure')
    return numbers


def validate_engine(engine, has_truth):
    size = engine.get('size')
    if (not isinstance(size, list) or len(size) != 2
            or any(type(n) is not int or n <= 0 for n in size)):
        raise ValueError('invalid output geometry')
    if not engine.get('sha256') or not engine.get('recipe'):
        raise ValueError('missing renderer identity')
    timing = engine['timing']
    count = timing.get('count')
    samples = timing.get('sorted_seconds')
    if (type(count) is not int or count not in (1, 30)
            or not isinstance(samples, list) or len(samples) != count):
        raise ValueError('invalid timing population')
    samples = [color.number(v) for v in samples]
    if min(samples) < 0 or samples != sorted(samples):
        raise ValueError('invalid timing samples')
    for key, expected in [('median_seconds', (samples[(count-1)//2]+samples[count//2])/2),
                          ('p95_seconds', samples[math.ceil(count*.95)-1])]:
        if not math.isclose(color.number(timing[key]), expected, rel_tol=1e-12, abs_tol=1e-12):
            raise ValueError('inconsistent timing summary')
    metrics = engine.get('truth_metrics')
    if not has_truth:
        if metrics is not None:
            raise ValueError('metrics without reference identity')
        return
    if not isinstance(metrics, list) or not metrics:
        raise ValueError('missing reference metrics')
    rectangles = set()
    for metric in metrics:
        rect = metric.get('rect')
        if (not isinstance(rect, list) or len(rect) != 4
                or any(type(n) is not int or n < 0 for n in rect)
                or min(rect[2:]) == 0 or rect[0]+rect[2] >= size[0]
                or rect[1]+rect[3] >= size[1] or tuple(rect) in rectangles):
            raise ValueError('invalid or duplicate truth patch')
        rectangles.add(tuple(rect))
        vector(metric['actual'])
        vector(metric['reference'])
        for key in ['rgb_rmse', 'gradient_rmse', 'maximum_error']:
            vector(metric[key], nonnegative=True)
        if 'chroma_pixels' in metric:
            pixels = metric['chroma_pixels']
            if type(pixels) is not int or not 0 <= pixels <= rect[2]*rect[3]:
                raise ValueError('invalid chroma population')
            maximum = color.number(metric['gradient_max_abs'])
            if maximum < 0:
                raise ValueError('negative gradient maximum')
            if pixels:
                rms = color.number(metric['chroma_rmse'])
                peak = color.number(metric['chroma_max_abs'])
                if not 0 <= rms <= peak + 1e-12:
                    raise ValueError('invalid chroma extrema')
            elif metric['chroma_rmse'] is not None or metric['chroma_max_abs'] is not None:
                raise ValueError('chroma metric without usable pixels')
        # The harness accumulates per-pixel squares in f64. A constant error
        # can round the final RMS just above its maximum over a large patch.
        if any(r > m and not math.isclose(r, m, rel_tol=1e-10, abs_tol=1e-12)
               for r, m in zip(metric['rgb_rmse'], metric['maximum_error'])):
            raise ValueError('RMSE exceeds maximum error')


def load(path):
    data = json.loads(path.read_bytes())
    if data.get('protocol') != 'TR-renderer-duel-v1' or data.get('complete') is not True or data.get('passed') is not True:
        raise ValueError('incomplete or unknown campaign')
    rows = {s['id']: s for s in data['sources']}
    if not rows or len(rows) != len(data['sources']):
        raise ValueError('empty or duplicate sources')
    for row in rows.values():
        if row['original_unchanged'] is not True or len(row['engines']) != 2:
            raise ValueError('invalid evidence')
        if [e['engine'] for e in row['engines']] != ['TrueRendererExperimental', 'TrueRenderer']:
            raise ValueError('engine order mismatch')
        for engine in row['engines']:
            validate_engine(engine, row.get('truth_sha256') is not None)
        common = row.get('common_chroma')
        if common is not None:
            patches = row['engines'][0]['truth_metrics']
            if not isinstance(common, list) or not patches or len(common) != len(patches):
                raise ValueError('invalid common chroma patches')
            for metric, patch in zip(common, patches):
                if metric['rect'] != patch['rect']:
                    raise ValueError('common chroma rectangle mismatch')
                n = patch['rect'][2]*patch['rect'][3]
                count = metric['pixels']
                excluded = metric['excluded_experimental_legacy_truth']
                if (type(count) is not int or not isinstance(excluded, list) or len(excluded) != 3
                        or any(type(v) is not int or not 0 <= v <= n for v in excluded)
                        or not max(0, n-sum(excluded)) <= count <= n-max(excluded)):
                    raise ValueError('inconsistent common chroma population')
                values = [metric[key] for key in ['experimental_legacy_rmse', 'experimental_legacy_max_abs']]
                if any(not isinstance(v, list) or len(v) != 2 for v in values):
                    raise ValueError('invalid common chroma vectors')
                if count:
                    rms, peak = [[color.number(v) for v in vec] for vec in values]
                    if any(not 0 <= a <= b+1e-12 for a,b in zip(rms,peak)):
                        raise ValueError('invalid common chroma extrema')
                elif any(v is not None for vec in values for v in vec):
                    raise ValueError('common chroma without valid pixels')
        equal_geometry = row['engines'][0]['size'] == row['engines'][1]['size']
        if row.get('truth_sha256') is not None and not equal_geometry:
            raise ValueError('renderer geometries differ')
        if 'equal_output_geometry' in row and row['equal_output_geometry'] is not equal_geometry:
            raise ValueError('inconsistent geometry declaration')
        counts = [e['timing']['count'] for e in row['engines']]
        if counts[0] != counts[1] or row.get('warmups') != (3 if counts[0] == 30 else 0):
            raise ValueError('renderer timing conditions differ')
    return rows


def analyze(before_path, after_path, descriptions):
    before, after = load(before_path), load(after_path)
    if before.keys() != after.keys():
        raise ValueError('source sets differ')
    descriptions_rows = json.loads(descriptions.read_bytes())['sources']
    desc = {s['id']: s for s in descriptions_rows}
    if len(desc) != len(descriptions_rows):
        raise ValueError('duplicate descriptions')
    if not desc.keys() <= before.keys():
        raise ValueError('missing described source')
    matrix = wb.conversion()
    cases, timing, changes = [], [], []
    grouped = {}
    for ident, a in before.items():
        b = after[ident]
        if (a['source_sha256'], a['truth_sha256']) != (b['source_sha256'], b['truth_sha256']):
            raise ValueError('input identity changed')
        old, baseline, candidate = a['engines'][1], a['engines'][0], b['engines'][0]
        if (old['sha256'], old['recipe']) != (b['engines'][1]['sha256'], b['engines'][1]['recipe']):
            raise ValueError('legacy output or recipe changed')
        if baseline['size'] != candidate['size'] or old['size'] != b['engines'][1]['size']:
            raise ValueError('output geometry changed')
        if baseline['timing']['count'] != candidate['timing']['count'] or a['warmups'] != b['warmups']:
            raise ValueError('campaign timing conditions changed')
        changes.append({'id': ident, 'candidate_identical_to_baseline': baseline['sha256'] == candidate['sha256'],
                        'legacy_repeatable': True})
        if old['timing']['count'] == 30:
            timing.append({'id': ident, 'before': [e['timing'] for e in a['engines']],
                           'after': [e['timing'] for e in b['engines']],
                           'output_sizes': [e['size'] for e in b['engines']],
                           'equal_output_geometry': candidate['size'] == old['size'],
                           'scope': 'Complete native recipes; unequal geometry is not an equal-workload kernel comparison.',
                           'candidate_over_legacy_median': candidate['timing']['median_seconds']/b['engines'][1]['timing']['median_seconds'],
                           'candidate_over_baseline_median': candidate['timing']['median_seconds']/baseline['timing']['median_seconds']})
        if ident not in desc:
            continue
        d = desc[ident]
        metrics = [e['truth_metrics'] for e in [baseline, candidate, old, b['engines'][1]]]
        if any(m is None for m in metrics):
            raise ValueError('description without reference metrics')
        if any(len(m) != len(d['patch_sensor_range']) for m in metrics):
            raise ValueError('patch count mismatch')
        for index, (m0, m1, m2, m3) in enumerate(zip(*metrics)):
            if len({tuple(m['rect']) for m in [m0,m1,m2,m3]}) != 1 or len({tuple(m['reference']) for m in [m0,m1,m2,m3]}) != 1:
                raise ValueError('patch identity changed')
            # Additional diagnostics may be introduced after a frozen report.
            # Every historical field must still reproduce exactly.
            if any(key not in m3 or value != m3[key] for key, value in m2.items()):
                raise ValueError('legacy metrics changed despite identical output')
            low, high = [color.number(v) for v in d['patch_sensor_range'][index]]
            if low > high:
                raise ValueError('invalid sensor range')
            xyz = [color.transform(matrix, m['actual']) for m in [m0,m1,m2]]
            reference = color.transform(matrix, m0['reference'])
            in_lab = low >= 0 and high < .98 and all(min(v) >= 0 and v[1] <= 1 for v in [*xyz, reference])
            delta = [color.delta_e(color.lab(v), color.lab(reference)) for v in xyz] if in_lab else None
            kind = d['kind']
            group = kind if kind != 'color' else 'color-'+ident.rsplit('-p', 1)[0].rsplit('-', 1)[1]
            item = {'id': ident, 'patch': index, 'kind': kind, 'sensor_range': [low,high],
                    'unclipped_lab_domain': in_lab, 'delta_e00': delta,
                    'rgb_rmse': [rms(m['rgb_rmse']) for m in [m0,m1,m2]],
                    'maximum_rgb_error': [max(m['maximum_error']) for m in [m0,m1,m2]],
                    'gradient_rmse': [rms(m['gradient_rmse']) for m in [m0,m1,m2]]}
            cases.append(item)
            grouped.setdefault(group, []).append(item)
    groups = {}
    for key, rows in grouped.items():
        groups[key] = {'count': len(rows), 'outside_lab_domain': sum(r['delta_e00'] is None for r in rows)}
        for field in ['delta_e00', 'rgb_rmse', 'gradient_rmse', 'maximum_rgb_error']:
            groups[key][field] = [wb.summary([r[field][i] for r in rows if r[field] is not None]) for i in range(3)]
        groups[key]['candidate_rgb_worse_than_legacy_by_more_than_1e_minus_5'] = sum(r['rgb_rmse'][1] > r['rgb_rmse'][2]+1e-5 for r in rows)
        groups[key]['candidate_rgb_worse_than_baseline_by_more_than_1e_minus_5'] = sum(r['rgb_rmse'][1] > r['rgb_rmse'][0]+1e-5 for r in rows)
        groups[key]['candidate_maximum_rgb_worse_than_legacy_by_more_than_1e_minus_5'] = sum(r['maximum_rgb_error'][1] > r['maximum_rgb_error'][2]+1e-5 for r in rows)
        groups[key]['candidate_maximum_rgb_worse_than_baseline_by_more_than_1e_minus_5'] = sum(r['maximum_rgb_error'][1] > r['maximum_rgb_error'][0]+1e-5 for r in rows)
    return {'schema': 1, 'method_order': ['autonomous_before', 'autonomous_candidate', 'legacy'],
            'scope': 'Known mathematical model. The compatibility matrices generate the input as well as interpreting it; these results do not measure physical camera accuracy. Clipped cases retain linear errors and are excluded from the main Lab score. No exposure fitting.',
            'groups': groups, 'cases': cases, 'timing': timing, 'output_changes': changes,
            'inputs_sha256': {k: hashlib.sha256(p.read_bytes()).hexdigest() for k,p in
                              [('before',before_path),('after',after_path),('description',descriptions)]}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('description', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = analyze(args.before,args.after,args.description)
    result['analyzer_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    with args.output.open('x') as stream:
        stream.write(json.dumps(result,indent=2,allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
