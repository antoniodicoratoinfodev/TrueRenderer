#!/usr/bin/env python3
"""Guard the diagnostic comparison domain and its input identities."""
import copy
import importlib
import json
from pathlib import Path
import tempfile
import unittest
import subprocess
import sys

duel = importlib.import_module('analyze-renderer-duel')


class AnalysisTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        metric = {'rect':[1,1,2,2], 'actual':[.18]*3, 'reference':[.18]*3,
                  'rgb_rmse':[0.]*3, 'gradient_rmse':[0.]*3, 'maximum_error':[0.]*3}
        self.row = {'id':'test', 'original_unchanged':True, 'source_sha256':'input',
                    'truth_sha256':'truth', 'warmups':0, 'engines':[
                        {'engine':name, 'sha256':name, 'recipe':name, 'size':[5,6],
                         'truth_metrics':[copy.deepcopy(metric)],
                         'timing':{'count':1,'sorted_seconds':[.1],'median_seconds':.1,'p95_seconds':.1}}
                        for name in ['TrueRendererExperimental','TrueRenderer']]}
        self.data = {'protocol':'TR-renderer-duel-v1','complete':True,'passed':True,'sources':[self.row]}
        self.description = {'sources':[{'id':'test','kind':'validation-color','patch_sensor_range':[[.05,.8]]}]}

    def run_analysis(self, after=None):
        for name,data in [('before',self.data),('after',after or self.data),('description',self.description)]:
            (self.root/name).write_text(json.dumps(data))
        return duel.analyze(self.root/'before',self.root/'after',self.root/'description')

    def test_exact_unclipped_reference(self):
        result = self.run_analysis()
        self.assertEqual(result['cases'][0]['delta_e00'],[0.]*3)
        self.assertTrue(result['output_changes'][0]['candidate_identical_to_baseline'])

    def test_common_chroma_uses_one_bounded_population_and_finite_extrema(self):
        after = copy.deepcopy(self.data)
        metric = {'rect':[1,1,2,2], 'pixels':3, 'excluded_experimental_legacy_truth':[1,0,0],
                  'experimental_legacy_rmse':[0.,0.], 'experimental_legacy_max_abs':[0.,0.]}
        after['sources'][0]['common_chroma']=[metric]
        self.run_analysis(after)
        for field,value in [('pixels',4),('pixels',2),('rect',[1,1,1,1]),
                            ('experimental_legacy_rmse',[float('nan'),0.]),
                            ('experimental_legacy_max_abs',[-1.,0.])]:
            invalid=copy.deepcopy(after);invalid['sources'][0]['common_chroma'][0][field]=value
            with self.subTest(field=field,value=value), self.assertRaises(ValueError):
                self.run_analysis(invalid)

    def test_highlight_generator_requires_an_explicit_campaign_mode(self):
        output = self.root/'must-not-exist'
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('generate-highlight-edges.py')),
                                 str(output)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('--development --validation', result.stderr)
        self.assertFalse(output.exists())

    def test_clipping_is_excluded_without_erasing_linear_error(self):
        self.description['sources'][0]['patch_sensor_range']=[[.05,1.3]]
        after = copy.deepcopy(self.data)
        after['sources'][0]['engines'][0]['truth_metrics'][0]['rgb_rmse']=[.3,.4,0.]
        after['sources'][0]['engines'][0]['truth_metrics'][0]['maximum_error']=[.3,.4,0.]
        result = self.run_analysis(after)
        self.assertIsNone(result['cases'][0]['delta_e00'])
        self.assertAlmostEqual(result['cases'][0]['rgb_rmse'][1],(.25/3)**.5)
        self.assertEqual(result['cases'][0]['maximum_rgb_error'],[0.,.4,0.])
        self.assertEqual(result['groups']['validation-color']['candidate_maximum_rgb_worse_than_legacy_by_more_than_1e_minus_5'],1)

    def test_reference_or_legacy_changes_are_refused(self):
        for field in ['source_sha256','truth_sha256','legacy']:
            after = copy.deepcopy(self.data)
            if field == 'legacy':
                after['sources'][0]['engines'][1]['sha256']='changed'
            else:
                after['sources'][0][field]='changed'
            with self.assertRaises(ValueError):
                self.run_analysis(after)

    def test_incomplete_duplicate_and_reordered_engines_are_refused(self):
        for kind in ['incomplete','duplicate','engines']:
            after = copy.deepcopy(self.data)
            if kind == 'incomplete':
                after['complete']=False
            elif kind == 'duplicate':
                after['sources'].append(copy.deepcopy(self.row))
            else:
                after['sources'][0]['engines'].reverse()
            with self.assertRaises(ValueError):
                self.run_analysis(after)

    def test_nonfinite_negative_or_missing_metrics_are_refused(self):
        for key in ['actual','reference','rgb_rmse','gradient_rmse','maximum_error']:
            for value in [float('nan'),float('inf')]:
                with self.subTest(key=key,value=value):
                    after = copy.deepcopy(self.data)
                    after['sources'][0]['engines'][0]['truth_metrics'][0][key][0]=value
                    with self.assertRaises(ValueError):
                        self.run_analysis(after)
        for value in [[-1.,0.,0.], [], [0.,0.]]:
            after = copy.deepcopy(self.data)
            after['sources'][0]['engines'][0]['truth_metrics'][0]['rgb_rmse']=value
            with self.assertRaises(ValueError):
                self.run_analysis(after)

    def test_geometry_timing_and_legacy_evidence_cannot_drift(self):
        for kind in ['geometry','all-geometry','timing','samples','legacy-metric','legacy-recipe','truth']:
            after = copy.deepcopy(self.data)
            row=after['sources'][0]; candidate,old=row['engines']
            if kind=='geometry': candidate['size']=[6,5]
            elif kind=='all-geometry':
                for e in row['engines']: e['size']=[6,5]
            elif kind=='timing': row['warmups']=3
            elif kind=='samples': candidate['timing']['median_seconds']=.2
            elif kind=='legacy-metric': old['truth_metrics'][0]['actual'][0]=.2
            elif kind=='legacy-recipe': old['recipe']='changed'
            elif kind=='truth': candidate['truth_metrics']=None
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                self.run_analysis(after)

    def test_duplicate_descriptions_and_patches_are_refused(self):
        self.description['sources'].append(copy.deepcopy(self.description['sources'][0]))
        with self.assertRaises(ValueError): self.run_analysis()
        self.description['sources'].pop()
        after=copy.deepcopy(self.data)
        metrics=after['sources'][0]['engines'][0]['truth_metrics']
        metrics.append(copy.deepcopy(metrics[0]))
        with self.assertRaises(ValueError): self.run_analysis(after)

    def test_native_timing_keeps_different_crops_explicit(self):
        self.description['sources']=[]
        self.row['truth_sha256']=None
        self.row['warmups']=3
        for e in self.row['engines']:
            e['truth_metrics']=None
            e['timing']={'count':30,'sorted_seconds':[.1]*30,'median_seconds':.1,'p95_seconds':.1}
        self.row['engines'][1]['size']=[7,8]
        result=self.run_analysis()
        self.assertFalse(result['timing'][0]['equal_output_geometry'])
        self.assertEqual(result['timing'][0]['output_sizes'],[[5,6],[7,8]])


if __name__ == '__main__':
    unittest.main()
