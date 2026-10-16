#!/usr/bin/env python3
"""Exercise bracket admission with independent analytic sensor observations."""
import copy
import hashlib
import importlib
import json
from pathlib import Path
import tempfile
import unittest

bench = importlib.import_module('prepare-photo-brackets')


class BracketTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.width, self.height = 96, 80
        self.truth = [(.18+.003*(i % 96)+.001*(i//96)) if i//96 < 40
                      else 1.1+.001*(i % 96)+.002*(i//96)
                      for i in range(96*80)]
        self.values = [[min(v, 1.) for v in self.truth],
                       [v/4 for v in self.truth], [v/4 for v in self.truth]]
        self.states = [[2 if v >= 1 else 0 for v in self.truth], [0]*7680, [0]*7680]
        self.registration = {'offset': [0, 0], 'method': 'synthetic exact coordinate truth',
                             'landmarks': [[x, y, x, y] for x, y in [(4, 4), (45, 4), (90, 4), (4, 74), (45, 74), (90, 74)]]}
        self.config = {
            'protocol': 'TR-photo-bracket-v1', 'role': 'development', 'candidate_freeze_sha256': 'a'*64,
            'acquisition': {'scene_id': 'analytic', 'session_id': 'test', 'illuminant': 'model',
                            'rights': 'generated', 'static_scene': True, 'fixed_optics': True, 'fixed_lighting': True},
            'limits': {'min_signal': .02, 'max_signal': .9, 'scale_metadata_relative': .01,
                       'scale_phase_relative': .005, 'validation_relative': .01, 'repeat_relative': .01,
                       'registration_max_px': .1, 'max_translation_px': 12, 'minimum_sites_per_phase': 16},
            'regions': [{'id': 'fit', 'role': 'fit', 'category': 'neutral', 'rect': [4, 4, 24, 24]},
                        {'id': 'check', 'role': 'validation', 'category': 'coloured-surface', 'rect': [36, 4, 24, 24]},
                        {'id': 'clipped', 'role': 'evaluation', 'category': 'fine-texture', 'rect': [4, 40, 80, 32]}]}
        for index, name in enumerate(['target', 'reference', 'repeat']):
            self.config[name] = {'id': str(index), 'exposure_s': .04 if index == 0 else .01,
                                 'iso': 100, 'f_number': 8, 'focus_id': 'locked',
                                 'registration': copy.deepcopy(self.registration)}

    def run_bench(self):
        rows = []
        for index in range(3):
            row = {'id': str(index), 'source_sha256': str(index)*64, 'width': 96, 'height': 80,
                   'period': 2, 'pattern': [0, 1, 1, 2], 'camera': 'synthetic', 'crop': [0, 0, 96, 80], 'orientation': 1}
            for key, data in [('values', bench.encode(self.values[index])), ('state', bytes(self.states[index])),
                              ('recovered', bench.encode(self.truth))]:
                name = str(index)+'-'+key
                (self.root/name).write_bytes(data)
                row[key] = name
                row[key+'_sha256'] = hashlib.sha256(data).hexdigest()
            rows.append(row)
        manifest = json.dumps({'protocol': 'TR-photo-bracket-planes-v1', 'recipe': 'test', 'frames': rows}).encode()
        (self.root/'planes.json').write_bytes(manifest)
        self.config.update(planes='planes.json', planes_sha256=hashlib.sha256(manifest).hexdigest())
        (self.root/'manifest.json').write_text(json.dumps(self.config))
        return bench.prepare(self.root/'manifest.json', self.root/'result')

    def test_common_scale_and_clipped_error_are_recovered(self):
        result = self.run_bench()
        self.assertTrue(result['accepted'])
        self.assertAlmostEqual(result['common_scale'], 4., places=6)
        region = result['regions'][-1]
        self.assertEqual(region['uncertain_clipped_sites'], 0)
        self.assertEqual(region['supported_clipped_sites'], 2560)
        self.assertGreater(region['original'][0]['max_abs'], .1)
        self.assertLess(region['recovered'][0]['max_abs'], 1e-6)
        self.assertFalse(result['absolute_colour_reference'])

    def test_channel_scale_drift_cannot_be_absorbed_by_white_balance(self):
        for i in range(7680):
            if i % 2 == 0 and i//96 % 2 == 0:
                self.values[1][i] *= 1.08
        self.assertFalse(self.run_bench()['accepted'])
        self.assertFalse((self.root/'result/reference.f32').exists())

    def test_validation_region_is_not_refitted(self):
        for i in bench.indices([36, 4, 24, 24], 96):
            self.values[1][i] *= 1.03
            self.values[2][i] *= 1.03
        result = self.run_bench()
        self.assertAlmostEqual(result['common_scale'], 4., places=6)
        self.assertFalse(result['accepted'])

    def test_local_repeat_disagreement_rejects_reference(self):
        self.values[2][50*96+50] *= 1.1
        result = self.run_bench()
        self.assertFalse(result['accepted'])
        self.assertEqual(result['repeat_failed_sites'], 1)

    def test_clipped_dark_site_stays_uncertain(self):
        self.states[1][50*96+50] = 2
        result = self.run_bench()
        self.assertTrue(result['accepted'])
        self.assertEqual(result['regions'][-1]['uncertain_clipped_sites'], 1)

    def test_metadata_scale_is_an_independent_check(self):
        self.config['target']['exposure_s'] = .08
        self.assertFalse(self.run_bench()['accepted'])

    def test_non_cfa_translation_is_rejected(self):
        self.config['reference']['registration']['offset'] = [1, 0]
        with self.assertRaisesRegex(ValueError, 'CFA-period'):
            self.run_bench()

    def test_subpixel_alignment_error_is_rejected(self):
        self.config['reference']['registration']['landmarks'][0][2] += .2
        with self.assertRaisesRegex(ValueError, 'residual'):
            self.run_bench()

    def test_fit_validation_overlap_is_rejected(self):
        self.config['regions'][1]['rect'] = [4, 4, 24, 24]
        with self.assertRaisesRegex(ValueError, 'disjoint'):
            self.run_bench()

    def test_nonfinite_reference_is_rejected(self):
        self.values[1][0] = float('nan')
        with self.assertRaisesRegex(ValueError, 'nonfinite'):
            self.run_bench()

    def test_input_hash_is_verified(self):
        self.run_bench()
        (self.root/'0-values').write_bytes(b'invalid')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            bench.prepare(self.root/'manifest.json', self.root/'other')

    def test_verification_rejects_reused_capture_identities(self):
        self.config.update(role='verification', development_source_sha256=['1'*64])
        with self.assertRaisesRegex(ValueError, 'overlap development'):
            self.run_bench()

    def test_duplicate_capture_is_rejected(self):
        self.config['repeat']['id'] = self.config['reference']['id']
        with self.assertRaisesRegex(ValueError, 'distinct captures'):
            self.run_bench()

    def check_single_clipped_channel(self, channel):
        pattern = [0, 1, 1, 2]
        self.truth = [v if pattern[bench.phase(i, 96, 2)] == channel else .4
                      for i, v in enumerate(self.truth)]
        self.values = [[min(v, 1.) for v in self.truth], [v/4 for v in self.truth], [v/4 for v in self.truth]]
        self.states[0] = [2 if v >= 1 else 0 for v in self.truth]
        result = self.run_bench()
        self.assertTrue(result['accepted'])
        region = result['regions'][-1]
        self.assertEqual(region['clipping'], ['red', 'green', 'blue'][channel])
        self.assertEqual(sum(n > 0 for n in region['clipped_sites_by_channel']), 1)

    def test_red_clipping_is_kept_separate(self):
        self.check_single_clipped_channel(0)

    def test_green_clipping_is_kept_separate(self):
        self.check_single_clipped_channel(1)

    def test_blue_clipping_is_kept_separate(self):
        self.check_single_clipped_channel(2)


if __name__ == '__main__':
    unittest.main()
