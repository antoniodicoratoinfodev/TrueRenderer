#!/usr/bin/env python3
"""Deterministic linear denoise campaign with known truth; numpy is test-only.

Build: scripts/cargo-local.sh build --release -p tr-core --example denoise-probe
Run: python test-denoise-quality.py --root var/NEW_CAMPAIGN
No RAW decoder, model download, private-photo publication or product fallback.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import numpy as np

ROOT = Path(__file__).resolve().parents[1]


def luma(rgb):
    return rgb[..., 0] * .2627 + rgb[..., 1] * .678 + rgb[..., 2] * .0593


def rgb(y, r, b):
    return np.stack((y + r, y - (.2627 * r + .0593 * b) / .678, y + b), -1)


def fixture(seed, correlated):
    h, w = 512, 640
    y, x = np.mgrid[:h, :w]
    level = np.select([y < 170, y < 340], [.04, .25], default=1.1)
    texture = .035 * np.sin(x * .81) + .025 * np.cos(y * 1.31)
    edge = np.where(x % 80 < 40, -.03, .09)
    thin = np.where(x % 17 == 0, .09, 0)
    signal = level + np.select([x < 160, x < 320, x < 480], [0, texture, edge], default=thin)
    truth = rgb(signal, .03 * np.sin(y * .17), .02 * np.cos(x * .13))
    rng = np.random.default_rng(seed)
    noise = rng.normal(size=(h, w, 3))
    if correlated:
        for c in range(3):
            n = noise[..., c]
            # Correlation from a known separable filter, with variance restored.
            for axis in [0, 1]:
                n = (np.roll(n, 1, axis) + 2*n + np.roll(n, -1, axis)) / np.sqrt(6)
            noise[..., c] = n
    sigma = .035 * np.sqrt(np.abs(level) + .003)
    noise = rgb(noise[..., 0]*sigma, noise[..., 1]*sigma*1.4, noise[..., 2]*sigma*1.8)
    truth = np.asarray(truth, dtype='<f4')
    noisy = np.asarray(truth + noise, dtype='<f4')
    return truth, noisy


def metrics(actual, truth):
    result = {}
    for name, xs in [('flat', slice(16, 144)), ('texture', slice(176, 304)),
                     ('edges', slice(336, 464)), ('thin_lines', slice(496, 624))]:
        a, t = actual[16:-16, xs], truth[16:-16, xs]
        delta = np.asarray(a-t, dtype=np.float64)
        y = luma(delta)
        chroma = delta-y[..., None]
        result[name] = {
            'rgb_mse': float(np.mean(delta**2)),
            'luma_mse': float(np.mean(y**2)),
            'chroma_mse': float(np.mean(chroma**2)),
            'gradient_mse': float(np.mean(np.diff(delta, axis=1)**2)),
        }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--probe', type=Path, default=ROOT/'target/release/examples/denoise-probe')
    args = parser.parse_args()
    root, probe = args.root.resolve(), args.probe.resolve(strict=True)
    if ROOT/'var' not in root.parents or root.exists():
        raise SystemExit('Use a new isolated directory under project var/')
    root.mkdir(parents=True)
    report = {'passed': False, 'numpy': np.__version__,
              'probe_sha256': hashlib.sha256(probe.read_bytes()).hexdigest(), 'cases': [],
              'scope': 'Generated scene-linear signals; fixed seeds, independent and correlated noise. '
                       'Not a physical sensor calibration or a universal photo-quality claim.'}
    for seed in [42021, 42029]:
        for correlated in [False, True]:
            name = f'{seed}-{"correlated" if correlated else "independent"}'
            case = root/name
            case.mkdir()
            truth, noisy = fixture(seed, correlated)
            h, w = truth.shape[:2]
            rgba = np.concatenate([noisy, np.ones((h, w, 1), dtype='<f4')], axis=-1)
            rgba.tofile(case/'input.rgba32f')
            truth.tofile(case/'truth.rgb32f')
            (case/'input.json').write_text(json.dumps({'size': [w, h]}))
            rows = {'input': metrics(noisy, truth)}
            timings = {}
            for method, amount in [('legacy', 80), ('spectral', 40), ('spectral', 80), ('spectral', 100)]:
                key = f'{method}{amount}'
                with (case/f'{key}.log').open('w') as log:
                    subprocess.run([str(probe), str(case), str(case/key), str(amount), str(amount), method, '4'],
                                   check=True, stdout=log, stderr=log, timeout=90)
                actual = np.fromfile(case/f'{key}.rgba32f', dtype='<f4').reshape(h, w, 4)
                assert np.array_equal(actual[..., 3], rgba[..., 3]), 'alpha changed'
                assert np.isfinite(actual).all(), 'nonfinite output'
                rows[key] = metrics(actual[..., :3], truth)
                timings[key] = json.loads((case/f'{key}.json').read_text())['seconds']
            checks = {
                'flat_noise_mse_below_15_percent': rows['spectral80']['flat']['rgb_mse'] < rows['input']['flat']['rgb_mse']*.15,
                'texture_better_than_legacy': rows['spectral80']['texture']['rgb_mse'] < rows['legacy80']['texture']['rgb_mse'],
                'edges_better_than_legacy': rows['spectral80']['edges']['rgb_mse'] < rows['legacy80']['edges']['rgb_mse'],
                'thin_lines_better_than_legacy': rows['spectral80']['thin_lines']['rgb_mse'] < rows['legacy80']['thin_lines']['rgb_mse'],
                'strength_reduces_flat_noise': rows['spectral100']['flat']['rgb_mse'] < rows['spectral40']['flat']['rgb_mse'],
            }
            report['cases'].append({'name': name, 'metrics': rows, 'seconds': timings, 'checks': checks})
            (root/'quality.json').write_text(json.dumps(report, indent=2)+'\n')
            print(name, checks, flush=True)
    report['passed'] = all(all(c['checks'].values()) for c in report['cases'])
    (root/'quality.json').write_text(json.dumps(report, indent=2)+'\n')
    if not report['passed']:
        raise SystemExit('Denoise quality gates failed; inspect quality.json')


if __name__ == '__main__':
    main()
