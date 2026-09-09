#!/usr/bin/env python3
"""Regression checks for native memory verdicts, without opening photographs."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    'preview_memory', Path(__file__).with_name('test-preview-memory.py'))
memory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(memory)


class VerdictTests(unittest.TestCase):
    def test_failed_workload_cannot_pass_because_its_peak_is_low(self):
        samples = [[0, 10, 20, 2, 0]]
        self.assertTrue(memory.summarize(samples, 100, 0)['passed'])
        for exit_code, timeout in [(1, False), (-9, False), (0, True)]:
            with self.subTest(exit_code=exit_code, timeout=timeout):
                report = memory.summarize(samples, 100, exit_code, timeout)
                self.assertFalse(report['passed'])
                self.assertEqual(report['peak_sum_physical_footprint_bytes'], 20)
                self.assertEqual(report['workload_exit_code'], exit_code)

    def test_missing_processes_incomplete_samples_and_overshoot_fail(self):
        for samples in [[], [[0, 10, 20, 1, 0]], [[0, 10, 20, 2, 1]],
                        [[0, 10, 20, 2, -1]], [[0, 101, 20, 2, 0]],
                        [[0, 10, 101, 2, 0]]]:
            with self.subTest(samples=samples):
                self.assertFalse(memory.summarize(samples, 100, 0)['passed'])


if __name__ == '__main__':
    unittest.main()
