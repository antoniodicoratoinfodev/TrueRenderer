#!/usr/bin/env python3
"""Validate fixed-camera exposure brackets and prepare relative CFA references.

Offline, standard-library only. Reads the opt-in Rust extractor's normalized
sensor planes. Never registers/resamples a mosaic, adjusts WB per channel, or
uses a renderer to set exposure. Rejected pairs produce diagnostics, no truth.
"""
import argparse
from array import array
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def finite(value):
    require(type(value) in (int, float) and math.isfinite(value), 'finite number required')
    return float(value)


def hashed(root, record, key):
    data = (root / record[key]).read_bytes()
    require(hashlib.sha256(data).hexdigest() == record[key + '_sha256'], key + ' hash mismatch')
    return data


def floats(data, count):
    require(len(data) == 4 * count, 'plane size mismatch')
    values = array('f')
    require(values.itemsize == 4, 'IEEE fp32 array required')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), 'nonfinite plane')
    return values


def encode(values):
    result = array('f', values)
    if sys.byteorder != 'little':
        result.byteswap()
    return result.tobytes()


def rectangle(rect, width, height):
    require(isinstance(rect, list) and len(rect) == 4
            and all(type(v) is int for v in rect), 'integer rectangle required')
    x, y, w, h = rect
    require(min(x, y) >= 0 and min(w, h) > 0 and x+w <= width and y+h <= height,
            'rectangle outside active sensor')
    return rect


def indices(rect, width):
    x, y, w, h = rect
    for yy in range(y, y+h):
        for xx in range(x, x+w):
            yield yy * width + xx


def overlap(a, b):
    return max(a[0], b[0]) < min(a[0]+a[2], b[0]+b[2]) and max(a[1], b[1]) < min(a[1]+a[3], b[1]+b[3])


def load_planes(root, config):
    raw = hashed(root, config, 'planes')
    manifest = json.loads(raw)
    require(manifest['protocol'] == 'TR-photo-bracket-planes-v1', 'plane protocol')
    rows = manifest['frames']
    by_id = {r['id']: r for r in rows}
    require(len(by_id) == len(rows), 'duplicate extracted frame')
    frames = []
    plane_root = (root / config['planes']).parent
    for key in ['target', 'reference', 'repeat']:
        record = by_id[config[key]['id']]
        w, h, p = (record[k] for k in ['width', 'height', 'period'])
        require(all(type(v) is int and v > 0 for v in [w, h]) and w*h <= 60_000_000,
                'unsupported sensor size')
        require(p in (2, 6) and len(record['pattern']) == p*p
                and set(record['pattern']) == {0, 1, 2}, 'unsupported CFA')
        value = floats(hashed(plane_root, record, 'values'), w*h)
        state = hashed(plane_root, record, 'state')
        require(len(state) == w*h and set(state) <= {0, 1, 2}, 'invalid sensor state')
        frame = dict(record, values_data=value, state_data=state)
        if key == 'target':
            frame['recovered_data'] = floats(hashed(plane_root, record, 'recovered'), w*h)
        frames.append(frame)
    target = frames[0]
    for frame in frames[1:]:
        require(all(frame[k] == target[k] for k in ['width', 'height', 'period', 'pattern', 'camera', 'crop', 'orientation']),
                'sensor geometry/camera mismatch')
    require(len({f['source_sha256'] for f in frames}) == 3, 'three distinct captures required')
    return frames, manifest['recipe']


def registration(record, width, height, period, limits):
    dx, dy = record['offset']
    require(all(type(v) is int and v % period == 0 for v in [dx, dy]),
            'only integer CFA-period translations are supported; no mosaic resampling')
    require(max(abs(dx), abs(dy)) <= limits['max_translation_px'], 'translation exceeds protocol')
    points = record['landmarks']
    require(len(points) >= 6, 'six independently measured registration landmarks required')
    require(isinstance(record['method'], str) and record['method'].strip(), 'registration method missing')
    residuals, positions = [], []
    for point in points:
        require(len(point) == 4, 'landmark needs target and reference coordinates')
        x, y, u, v = map(finite, point)
        require(0 <= x < width and 0 <= u < width and 0 <= y < height and 0 <= v < height,
                'landmark outside frame')
        positions.append((x, y))
        residuals.append(math.hypot(u-x-dx, v-y-dy))
    require(len(set(positions)) == len(positions), 'duplicate landmarks')
    require(max(x for x, _ in positions)-min(x for x, _ in positions) >= width/2
            and max(y for _, y in positions)-min(y for _, y in positions) >= height/2,
            'landmarks must span the frame')
    require(max(residuals) <= limits['registration_max_px'], 'subpixel/rotation/motion residual exceeds protocol')
    return (dx, dy), {'maximum_residual_px': max(residuals), 'landmarks': len(points), 'method': record['method']}


def phase(i, width, period):
    return (i // width % period) * period + i % width % period


def mapped(i, width, height, offset):
    x, y = i % width + offset[0], i // width + offset[1]
    return y * width + x if 0 <= x < width and 0 <= y < height else None


def metrics(actual, reference, mask, region, width, period, pattern):
    result = []
    for channel in range(3):
        sites = [i for i in indices(region, width) if mask[i] and pattern[phase(i, width, period)] == channel]
        if not sites:
            result.append({'channel': channel, 'samples': 0})
            continue
        errors = [actual[i]-reference[i] for i in sites]
        peak = max(range(len(sites)), key=lambda k: abs(errors[k]))
        site_set = set(sites)
        gradients = [(actual[j]-actual[i])-(reference[j]-reference[i])
                     for i in sites for j in (i+period, i+period*width)
                     if j in site_set and (j-i != period or j//width == i//width)]
        result.append({'channel': channel, 'samples': len(sites),
                       'bias': statistics.fmean(errors),
                       'rmse': math.sqrt(statistics.fmean(v*v for v in errors)),
                       'max_abs': abs(errors[peak]), 'peak_xy': [sites[peak] % width, sites[peak] // width],
                       'gradient_pairs': len(gradients),
                       'gradient_max_abs': max(map(abs, gradients), default=None)})
    return result


def prepare(path, output):
    raw = path.read_bytes()
    config = json.loads(raw)
    require(config['protocol'] == 'TR-photo-bracket-v1', 'bracket protocol')
    require(config['role'] in ['development', 'verification'], 'campaign role required')
    frozen = config['candidate_freeze_sha256']
    require(isinstance(frozen, str) and len(frozen) == 64 and all(c in '0123456789abcdef' for c in frozen), 'candidate freeze required')
    for key in ['scene_id', 'session_id', 'illuminant', 'rights']:
        require(isinstance(config['acquisition'][key], str) and config['acquisition'][key].strip(), 'acquisition provenance missing')
    require(all(config['acquisition'].get(k) is True for k in ['static_scene', 'fixed_optics', 'fixed_lighting']), 'fixed acquisition required')
    limits = config['limits']
    for key in ['min_signal', 'max_signal', 'scale_metadata_relative', 'scale_phase_relative',
                'validation_relative', 'repeat_relative', 'registration_max_px', 'max_translation_px']:
        require(finite(limits[key]) > 0, 'positive frozen limits required')
    require(limits['min_signal'] < limits['max_signal'] < 1, 'invalid signal interval')
    require(type(limits['minimum_sites_per_phase']) is int and limits['minimum_sites_per_phase'] >= 16,
            'minimum validation population too small')
    frames, recipe = load_planes(path.parent, config)
    target, dark, repeat = frames
    if config['role'] == 'verification':
        development = config['development_source_sha256']
        require(isinstance(development, list) and development, 'development identities required for verification')
        require(not ({f['source_sha256'] for f in frames} & set(development)),
                'verification captures overlap development')
    w, h, p = (target[k] for k in ['width', 'height', 'period'])
    offsets, alignments = [], []
    for name in ['reference', 'repeat']:
        offset, evidence = registration(config[name]['registration'], w, h, p, limits)
        offsets.append(offset)
        alignments.append(evidence)
    capture = [config[k] for k in ['target', 'reference', 'repeat']]
    for key in ['iso', 'f_number', 'focus_id']:
        require(all(f[key] == capture[0][key] for f in capture), 'ISO/aperture/focus must be fixed')
    times = [finite(c['exposure_s']) for c in capture]
    require(min(times) > 0 and times[0] > times[1] and times[1] == times[2], 'target must be brighter; references must repeat one exposure')
    expected = times[0]/times[1]
    regions = config['regions']
    require(0 < len(regions) <= 64 and len({r['id'] for r in regions}) == len(regions), 'unique bounded regions required')
    require({'fit', 'validation', 'evaluation'} <= {r['role'] for r in regions}, 'fit, validation and evaluation regions required')
    for region in regions:
        rectangle(region['rect'], w, h)
        require(region['rect'][2] * region['rect'][3] <= 512 * 512, 'region exceeds offline analysis bound')
        require(region['role'] in ['fit', 'validation', 'evaluation'], 'unknown region role')
        require(region['category'] in ['fine-texture', 'oblique-edge', 'coloured-surface', 'led', 'neutral'], 'unknown photographic category')
    fits = [r for r in regions if r['role'] == 'fit']
    for a, region in enumerate(regions):
        for other in regions[a+1:]:
            require(not overlap(region['rect'], other['rect']), 'regions must be disjoint')
    def pairs(region, reference, offset):
        for i in indices(region, w):
            j = mapped(i, w, h, offset)
            if j is None or target['state_data'][i] != 0 or reference['state_data'][j] != 0:
                continue
            a, b = target['values_data'][i], reference['values_data'][j]
            if limits['min_signal'] <= min(a, b) and max(a, b) <= limits['max_signal']:
                yield i, a, b
    fit = [v for r in fits for v in pairs(r['rect'], dark, offsets[0])]
    require(fit, 'no unsaturated radiometric overlap')
    # One common exposure slope through zero after black subtraction; no channel
    # gains or offsets that could absorb colour drift, flare or a wrong black.
    scale = math.fsum(a*b for _, a, b in fit)/math.fsum(b*b for _, _, b in fit)
    require(math.isfinite(scale) and scale > 0, 'invalid radiometric scale')
    failures, checks = [], []
    if abs(scale/expected-1) > limits['scale_metadata_relative']:
        failures.append('exposure metadata disagrees with measured common scale')
    for name, frame, offset in zip(['reference', 'repeat'], frames[1:], offsets):
        for region in regions:
            if region['role'] == 'evaluation':
                continue
            group = list(pairs(region['rect'], frame, offset))
            for q in range(p*p):
                samples = [(a, b) for i, a, b in group if phase(i, w, p) == q]
                row = {'capture': name, 'region': region['id'], 'phase': q, 'samples': len(samples)}
                if len(samples) < limits['minimum_sites_per_phase']:
                    failures.append(f'{name}/{region["id"]}/phase{q}: insufficient overlap')
                else:
                    slope = math.fsum(a*b for a, b in samples)/math.fsum(b*b for _, b in samples)
                    errors = [abs(a-scale*b)/max(abs(a), abs(scale*b), limits['min_signal']) for a, b in samples]
                    row.update(scale=slope, relative_rmse=math.sqrt(statistics.fmean(e*e for e in errors)),
                               relative_max=max(errors))
                    if abs(slope/scale-1) > limits['scale_phase_relative'] or max(errors) > limits['validation_relative']:
                        failures.append(f'{name}/{region["id"]}/phase{q}: radiometry/motion/colour drift')
                checks.append(row)
    reference = array('f', [0.]) * (w*h)
    discrepancy = array('f', [0.]) * (w*h)
    mask = bytearray(w*h)
    repeat_failures = 0
    for region in regions:
        for i in indices(region['rect'], w):
            j, k = [mapped(i, w, h, o) for o in offsets]
            if j is None or k is None or dark['state_data'][j] != 0 or repeat['state_data'][k] != 0:
                continue
            a, b = dark['values_data'][j], repeat['values_data'][k]
            if not limits['min_signal'] <= min(a, b) <= max(a, b) <= limits['max_signal']:
                continue
            difference = abs(a-b)/max(abs(a), abs(b), limits['min_signal'])
            if difference > limits['repeat_relative']:
                repeat_failures += 1
                continue
            reference[i] = scale * a
            discrepancy[i] = scale * abs(a-b)
            mask[i] = 1
    if repeat_failures:
        failures.append('repeat inconsistency on fixed regions: possible movement, noise or LED variation')
    details = []
    for region in regions:
        clipped = bytearray(w*h)
        for i in indices(region['rect'], w):
            clipped[i] = bool(mask[i] and target['state_data'][i] == 2)
        count = sum(target['state_data'][i] == 2 for i in indices(region['rect'], w))
        clipped_by_channel = [sum(target['state_data'][i] == 2
                                  and target['pattern'][phase(i, w, p)] == c
                                  for i in indices(region['rect'], w)) for c in range(3)]
        channels = [c for c, n in enumerate(clipped_by_channel) if n]
        details.append({'id': region['id'], 'category': region['category'], 'role': region['role'],
                        'rect': region['rect'], 'clipped_sites': count, 'supported_clipped_sites': sum(clipped),
                        'clipped_sites_by_channel': clipped_by_channel,
                        'clipping': 'none' if not channels else ['red', 'green', 'blue'][channels[0]] if len(channels) == 1 else 'multiple',
                        'uncertain_clipped_sites': count-sum(clipped),
                        'original': metrics(target['values_data'], reference, clipped, region['rect'], w, p, target['pattern']),
                        'recovered': metrics(target['recovered_data'], reference, clipped, region['rect'], w, p, target['pattern'])})
    valid_changes = sum(a != b or (a == 0 and math.copysign(1., a) != math.copysign(1., b))
                        for a, b, s in zip(target['values_data'], target['recovered_data'], target['state_data']) if s != 2)
    require(valid_changes == 0, 'recovery changed an original non-clipped site')
    report = {'protocol': config['protocol'], 'manifest_sha256': hashlib.sha256(raw).hexdigest(),
              'candidate_freeze_sha256': frozen, 'role': config['role'], 'recipe': recipe,
              'source_sha256': [f['source_sha256'] for f in frames], 'accepted': not failures,
              'metrics_usable': not failures,
              'failures': failures, 'alignment': alignments, 'common_scale': scale, 'metadata_scale': expected,
              'radiometry': checks, 'repeat_failed_sites': repeat_failures, 'regions': details,
              'absolute_colour_reference': False,
              'limits': ['Relative sensor reference only; no absolute colour accuracy or full RGB/demosaic truth.',
                         'Integer CFA-period translations only; subpixel registration/resampling is deliberately unsupported.',
                         'Repeat difference is a diagnostic, not a calibrated confidence interval or noise estimate.',
                         'Global failure invalidates every reference and all provisional error metrics.',
                         'Unknown physical clipping below the metadata ceiling needs separate measured calibration.']}
    output.mkdir(parents=True, exist_ok=False)
    if not failures:
        for name, data in [('reference.f32', encode(reference)), ('repeat-discrepancy.f32', encode(discrepancy)), ('valid.mask', bytes(mask))]:
            (output / name).write_bytes(data)
            report[name + '_sha256'] = hashlib.sha256(data).hexdigest()
    (output / 'report.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('new_output', type=Path)
    args = parser.parse_args()
    result = prepare(args.manifest, args.new_output)
    print('Accepted relative CFA reference' if result['accepted'] else 'Rejected: ' + '; '.join(result['failures']))
    sys.exit(0 if result['accepted'] else 1)
