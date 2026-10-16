#!/usr/bin/env python3
"""Generate matrix-only references from DNG 1.7.1 pp. 100–103, in Decimal.

The white coordinates/CCT are independent input (e.g. offline dng_temperature
from the official SDK). This oracle uses the SPECIFICATION's separately mixed
CM/CC, not the SDK's mixture of their products. No RAW renderer, demosaic,
photographic truth, profile fitting or production Rust is used here.
"""
import argparse
from decimal import Decimal, localcontext
import hashlib
import json
from pathlib import Path
import random


def d(value):
    return Decimal(str(value))


def vector(matrix, values):
    return [sum(a * b for a, b in zip(row, values)) for row in matrix]


def multiply(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def diagonal(values):
    return [[values[r] if r == c else d(0) for c in range(3)] for r in range(3)]


def inverse(a):
    # Cofactors, independent of the production pivoted Gauss–Jordan solver.
    def minor(r, c):
        rows = [i for i in range(3) if i != r]
        cols = [i for i in range(3) if i != c]
        return a[rows[0]][cols[0]] * a[rows[1]][cols[1]] - a[rows[0]][cols[1]] * a[rows[1]][cols[0]]
    co = [[(-1 if (r + c) % 2 else 1) * minor(r, c) for c in range(3)] for r in range(3)]
    det = sum(a[0][c] * co[0][c] for c in range(3))
    assert abs(det) > d('0.01')
    return [[co[c][r] / det for c in range(3)] for r in range(3)]


def mix(a, b, w):
    return [[w * a[r][c] + (1 - w) * b[r][c] for c in range(3)] for r in range(3)]


def xyz(point):
    x, y = map(d, point)
    return [x / y, d(1), (1 - x - y) / y]


def cat(a, b):
    bradford = [[d(v) for v in row] for row in [
        ['0.8951', '0.2664', '-0.1614'], ['-0.7502', '1.7135', '0.0367'], ['0.0389', '-0.0685', '1.0296']]]
    src, dst = vector(bradford, a), vector(bradford, b)
    return multiply(inverse(bradford), multiply(diagonal([v / u for u, v in zip(src, dst)]), bradford))


def encoded(value):
    if isinstance(value, Decimal):
        return float(value)
    if isinstance(value, list):
        return [encoded(v) for v in value]
    if isinstance(value, dict):
        return {k: encoded(v) for k, v in value.items()}
    return value


def generate(whites, seed, count):
    rng = random.Random(seed)
    d50, d65 = xyz(['0.3457', '0.3585']), xyz(['0.3127', '0.3290'])
    # Derive Rec.2020 XYZ primary columns and their D65 scale, rather than
    # importing the production inverse coefficients.
    primaries = [xyz(p) for p in [['0.708', '0.292'], ['0.170', '0.797'], ['0.131', '0.046']]]
    p = [[primaries[c][r] for c in range(3)] for r in range(3)]
    to2020 = inverse(multiply(p, diagonal(vector(inverse(p), d65))))
    output = multiply(to2020, cat(d50, d65))
    rows = []
    for i in range(count):
        def near_identity(spread):
            return [[d(int(r == c)) + d(rng.randint(-spread, spread)) / 1000
                     for c in range(3)] for r in range(3)]
        cm = [near_identity(80), near_identity(80)]
        cc = [near_identity(25), near_identity(25)]
        analog = [d(rng.randint(800, 1200)) / 1000 for _ in range(3)]
        temps = [d(2850), d(6500)]
        if i % 3 == 1:
            temps.reverse()  # Reversed tag order is valid.
        elif i % 3 == 2:
            temps[1] = temps[0]  # Single illuminant, duplicated internally.
            cm[1], cc[1] = cm[0], cc[0]
        fm = []
        for _ in range(2):
            m = near_identity(220)
            fm.append([[v * d50[r] / sum(m[r]) for v in m[r]] for r in range(3)])
        if temps[0] == temps[1]:
            fm[1] = fm[0]
        use_forward = i % 2 == 1
        def at(white):
            t = d(white['cct'])
            w = d(1) if temps[0] == temps[1] else min(d(1), max(d(0), (1 / t - 1 / temps[1]) / (1 / temps[0] - 1 / temps[1])))
            individual = multiply(diagonal(analog), mix(cc[0], cc[1], w))
            combined = multiply(individual, mix(cm[0], cm[1], w))
            n = vector(combined, xyz(white['xy']))
            n = [v / n[1] for v in n]
            return w, individual, combined, n
        shot = whites[i % len(whites)]
        _, _, _, shot_neutral = at(shot)
        trials = []
        for wi in [i % len(whites), (i + 1) % len(whites), (i + 4) % len(whites)]:
            white = whites[wi]
            weight, individual, combined, neutral = at(white)
            relative = [a / b for a, b in zip(shot_neutral, neutral)]
            # Resolver arithmetic also exercises a wider range than the UI's
            # relative-slider domain; all selected sensor neutrals stay valid.
            assert all(d('0.0625') <= x <= 16 for x in relative)
            if use_forward:
                inv = inverse(individual)
                ref = vector(inv, neutral)
                transform = multiply(mix(fm[0], fm[1], weight), multiply(diagonal([1 / v for v in ref]), inv))
            else:
                inv = inverse(combined)
                white_xyz = vector(inv, neutral)
                transform = multiply(cat([v / white_xyz[1] for v in white_xyz], d50),
                                     [[v / white_xyz[1] for v in row] for row in inv])
            transform = multiply(output, transform)
            # Signed, dark, chromatic and over-one camera data; no clamp.
            samples = [[d(0)] * 3, neutral, [d('0.2'), d('0.3'), d('0.4')],
                       [d('-0.04'), d('0.001'), d('1.8')], [d('2.3'), d('0.5'), d('-0.2')]]
            samples += [[d(rng.randint(-400, 2400)) / 1000 for _ in range(3)] for _ in range(7)]
            trials.append({'xy': white['xy'], 'cct': white['cct'], 'weight': weight,
                           'relative': relative, 'neutral': neutral,
                           'camera_to_rec2020': transform,
                           'vectors': [{'camera': v, 'expected': vector(transform, v)} for v in samples]})
        rows.append({'id': f's{seed}-{i}', 'cm': cm, 'cc': cc,
                     'fm': fm if use_forward else [None, None], 'analog': analog,
                     'temperatures': temps, 'neutral': shot_neutral, 'trials': trials})
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--whites', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--seed', required=True, type=int)
    parser.add_argument('--count', type=int, default=12)
    parser.add_argument('--select', help='Comma-separated generated profile indices to retain; default all')
    args = parser.parse_args()
    whites = json.loads(args.whites.read_text())
    whites = whites.get('white_reference', whites)
    assert 1 <= args.count <= 1000 and len(whites['whites']) >= 5
    selected = list(range(args.count)) if args.select is None else [int(v) for v in args.select.split(',')]
    assert selected and len(set(selected)) == len(selected) and all(0 <= i < args.count for i in selected)
    with localcontext() as ctx:
        ctx.prec = 70
        result = {'schema': 1, 'scope': 'DNG matrix arithmetic; not photographic calibration or demosaic quality',
                  'decimal_precision': ctx.prec, 'seed': args.seed,
                  'generated_profiles': args.count, 'selected_indices': selected,
                  'white_reference': whites,
                  'whites_sha256': hashlib.sha256(json.dumps(whites, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
                  'generator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'cases': generate(whites['whites'], args.seed, args.count)}
        result['cases'] = [result['cases'][i] for i in selected]
    args.output.write_text(json.dumps(encoded(result), indent=2, allow_nan=False) + '\n')
    print(f'{len(result["cases"])} profiles, {len(selected) * 3 * 12} signed/color vectors -> {args.output}')


if __name__ == '__main__':
    main()
