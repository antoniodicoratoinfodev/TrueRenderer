#!/usr/bin/env python3
"""Numerical tests use synthetic cameras, never measured camera profiles."""
import copy
import importlib.util
import importlib
import hashlib
import json
import math
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("calibration", Path(__file__).with_name("calibrate-raw-profile.py"))
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


def fixture():
    white = [.8, .7, .6]
    matrix = [[.9, .2, (c.D50[0] - .9 * .8 - .2 * .7) / .6],
              [.2, 1., (1 - .2 * .8 - .7) / .6],
              [.02, .1, (c.D50[2] - .02 * .8 - .1 * .7) / .6]]
    patches = []
    for split, shift in [("fit", 0), ("validation", .015)]:
        for i, rgb in enumerate([[.6, .2, .1], [.1, .5, .2], [.2, .1, .5], [.4, .35, .3],
                                  [.5, .4, .1], [.1, .4, .5], [.4, .15, .4], [.16, .14, .12]]):
            neutral = i in (3, 7)
            rgb = [v * (1 + shift) if neutral else v + shift for v in rgb]
            patches.append(dict(id=str(i), capture=split, split=split, camera_rgb=rgb,
                                xyz=c.transform(matrix, rgb), neutral=neutral))
    return dict(schema=1, kind="synthetic", camera="synthetic", mode="linear", illuminant="synthetic D50",
                reference_provenance="Analytical matrix; not a physical camera", uncertainty="No physical measurement",
                observer="CIE 1931 2 degree", reference_space="XYZ D50 Y=1", camera_white=white,
                white_capture="fit", patches=patches), matrix


def measured_fixture(root):
    config, matrix = fixture()
    white = config.pop("camera_white")
    config.pop("white_capture")
    config.update(schema=2, kind="measured", white=dict(capture="fit", patch="white", reference_y=.9), captures=[])
    for index, split in enumerate(("fit", "validation")):
        scale = 1.25 + index * .25
        rows = [(p["id"], p["camera_rgb"]) for p in config["patches"] if p["split"] == split]
        if split == "fit":
            rows.append(("white", [v * .9 for v in white]))
        measurements = []
        for row, (patch_id, rgb) in enumerate(rows):
            raw = [v / scale for v in rgb]
            phases = [dict(phase_xy=[i % 2, i // 2], color=color, sample_count=96,
                           mean=raw[color], minimum=raw[color], maximum=raw[color],
                           sample_variance=0., negative_sites=0, saturated_sites=0)
                      for i, color in enumerate([0, 1, 1, 2])]
            measurements.append(dict(id=patch_id, camera_rgb=raw, minimum=raw, maximum=raw,
                                     sample_variance=[0., 0., 0.], sample_count=[96, 192, 96],
                                     negative_sites=[0, 0, 0], saturated_sites=[0, 0, 0],
                                     rect_sensor=[row * 24, 0, 24, 16], fit_eligible_no_saturation=True,
                                     phase_statistics=phases, bayer_green_comparison=dict(
                                         green_red_row=[1, 0], green_blue_row=[0, 1], relative_mean_difference=0.)))
        report = dict(schema=2, camera="synthetic", domain=c.CFA_DOMAIN, recipe="synthetic-cfa-v1",
                      cfa_period=2, cfa_pattern=[0, 1, 1, 2],
                      source_sha256=hashlib.sha256(split.encode()).hexdigest(), sensor_size=[240, 160], patches=measurements)
        data = json.dumps(report).encode()
        name = f"{split}.json"
        (root / name).write_bytes(data)
        config["captures"].append(dict(id=split, split=split, exposure_scale=scale, statistics_file=name,
                                       statistics_sha256=hashlib.sha256(data).hexdigest()))
    for patch in config["patches"]:
        del patch["camera_rgb"], patch["split"]
    return config, matrix


def direct_phase_patch(period, pattern, rect):
    x0, y0, width, height = rect
    sites = [(x, y, (11 * x + 19 * y) % 41 / 100 - .02)
             for y in range(y0, y0 + height) for x in range(x0, x0 + width)]

    def moments(values):
        n = len(values)
        mean = math.fsum(values) / n if n else None
        return dict(sample_count=n, mean=mean, minimum=min(values) if n else None,
                    maximum=max(values) if n else None,
                    sample_variance=math.fsum((v - mean) ** 2 for v in values) / (n - 1) if n > 1 else None,
                    negative_sites=sum(v < 0 for v in values), saturated_sites=sum(v >= .985 for v in values))

    phases = [dict(phase_xy=[i % period, i // period], color=color,
                   **moments([v for x, y, v in sites if (y % period) * period + x % period == i]))
              for i, color in enumerate(pattern)]
    channels = [moments([v for x, y, v in sites if pattern[(y % period) * period + x % period] == color])
                for color in range(3)]
    patch = {k: [channel[k] for channel in channels] for k in channels[0]}
    patch["camera_rgb"] = patch.pop("mean")
    patch.update(rect_sensor=rect, phase_statistics=phases, bayer_green_comparison=None)
    if period == 2:
        greens = {pattern[(i // 2) * 2 + ((i % 2) ^ 1)]: p for i, p in enumerate(phases) if p["color"] == 1}
        gr, gb = greens[0]["mean"], greens[2]["mean"]
        patch["bayer_green_comparison"] = dict(green_red_row=greens[0]["phase_xy"],
                                              green_blue_row=greens[2]["phase_xy"],
                                              relative_mean_difference=(gr - gb) / ((gr + gb) * .5) if min(gr, gb) > 0 else None)
    return patch


class CalibrationTests(unittest.TestCase):
    def test_phase_recomposition_matches_direct_samples_at_all_bayer_and_xtrans_shifts(self):
        xtrans = [1, 1, 0, 1, 1, 2, 1, 1, 2, 1, 1, 0, 2, 0, 1, 0, 2, 1,
                  1, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2, 0, 2, 1, 2, 0, 1]
        invalid_layout = xtrans.copy()
        invalid_layout[0], invalid_layout[2] = invalid_layout[2], invalid_layout[0]
        with self.assertRaises(ValueError):
            c.cfa_layout(dict(cfa_period=6, cfa_pattern=invalid_layout))
        for period, base in [(2, [0, 1, 1, 2]), (6, xtrans)]:
            for shift in range(period * period):
                dx, dy = shift % period, shift // period
                pattern = [base[((i // period + dy) % period) * period + (i % period + dx) % period]
                           for i in range(period * period)]
                self.assertEqual(c.cfa_layout(dict(cfa_period=period, cfa_pattern=pattern)), (period, pattern))
                for rect in [[3, 2, 18, 15], [1, 2, 6, 3]]:
                    patch = direct_phase_patch(period, pattern, rect)
                    evidence = c.phase_evidence(patch, period, pattern)
                    self.assertEqual(evidence["phase_statistics"], patch["phase_statistics"])
                    if period == 6:
                        self.assertIsNone(evidence["bayer_green_comparison"])
                        if rect[2:] == [6, 3]:
                            self.assertEqual(sum(p["sample_count"] == 0 for p in evidence["phase_statistics"]), 18)
                            invalid = copy.deepcopy(patch)
                            next(p for p in invalid["phase_statistics"] if p["sample_count"] == 0)["mean"] = 0.
                            with self.assertRaises(ValueError):
                                c.phase_evidence(invalid, period, pattern)

    def test_measured_fit_binds_hashes_and_one_exposure_scale_per_capture(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            config, matrix = measured_fixture(root)
            report = c.calibrate(config, root)
            for actual, expected in zip(report["matrix_camera_to_xyz_d50"], matrix):
                for a, b in zip(actual, expected):
                    self.assertAlmostEqual(a, b, places=12)
            self.assertFalse(report["qualified"])
            self.assertEqual(len(report["measurement_evidence"]["patches"]), 16)
            self.assertEqual(len(report["measurement_evidence"]["patches"][0]["phase_statistics"]), 4)
            self.assertEqual(report["schema"], 2)
            self.assertLess(report["validation"]["metrics"]["maximum"], 1e-11)
            config["captures"][1]["exposure_scale"] *= 1.2
            changed = c.calibrate(config, root)
            self.assertEqual(report["matrix_camera_to_xyz_d50"], changed["matrix_camera_to_xyz_d50"])
            self.assertFalse(changed["validation"]["numerical_thresholds_passed"])

    def test_measured_fit_keeps_green_disparity_visible_without_inventing_a_correction(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            config, matrix = measured_fixture(root)
            path = root / "fit.json"
            report = json.loads(path.read_bytes())
            for patch in report["patches"]:
                green = patch["camera_rgb"][1]
                delta = green * .05
                for phase, shift in [(patch["phase_statistics"][1], delta),
                                     (patch["phase_statistics"][2], -delta)]:
                    phase.update(mean=green + shift, minimum=green + shift, maximum=green + shift)
                patch["minimum"][1] = green - delta
                patch["maximum"][1] = green + delta
                patch["sample_variance"][1] = 192 * delta ** 2 / 191
                patch["bayer_green_comparison"]["relative_mean_difference"] = .1
            data = json.dumps(report).encode()
            path.write_bytes(data)
            config["captures"][0]["statistics_sha256"] = hashlib.sha256(data).hexdigest()
            candidate = c.calibrate(config, root)
            for actual, expected in zip(candidate["matrix_camera_to_xyz_d50"], matrix):
                for a, b in zip(actual, expected):
                    self.assertAlmostEqual(a, b, places=12)
            trace = candidate["measurement_evidence"]["patches"][0]
            self.assertEqual(trace["bayer_green_comparison"]["relative_mean_difference"], .1)
            self.assertFalse(candidate["qualified"])

    def test_measured_fit_rejects_missing_or_inconsistent_phase_statistics(self):
        mutations = [lambda r: r.update(schema=1),
                     lambda r: r.update(cfa_pattern=[0, 0, 1, 2]),
                     lambda r: r.update(cfa_pattern=[0, 2, 1, 1]),
                     lambda r: r["patches"][0].pop("phase_statistics"),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(phase_xy=[1, 0]),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(sample_count=95),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(color=2),
                     lambda r: r["patches"][0]["phase_statistics"][1].update(
                         mean=.3, minimum=.3, maximum=.3),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(sample_variance=.1),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(mean=float("nan")),
                     lambda r: r["patches"][0]["phase_statistics"][0].update(saturated_sites=1),
                     lambda r: r["patches"][0]["bayer_green_comparison"].update(relative_mean_difference=.1),
                     lambda r: r["patches"][0]["bayer_green_comparison"].update(green_red_row=[0, 1])]
        for index, mutate in enumerate(mutations):
            with self.subTest(mutation=index), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                config, _ = measured_fixture(root)
                path = root / "validation.json"
                report = json.loads(path.read_bytes())
                mutate(report)
                data = json.dumps(report).encode()
                path.write_bytes(data)
                config["captures"][1]["statistics_sha256"] = hashlib.sha256(data).hexdigest()
                with self.assertRaises(ValueError):
                    c.calibrate(config, root)

    def test_measured_fit_rejects_hidden_saturation_reused_raw_and_unbound_means(self):
        mutations = [lambda r: r["patches"][0].update(maximum=[1., .5, .5]),
                     lambda r: r["patches"][0].update(saturated_sites=[1, 0, 0]),
                     lambda r: r["patches"][0].update(sample_count=[1, 2, 1]),
                     lambda r: r["patches"][0].update(rect_sensor=[239, 0, 24, 16]),
                     lambda r: r["patches"][0].update(camera_rgb=[.8, .8, .8]),
                     lambda r: r.update(source_sha256=hashlib.sha256(b"fit").hexdigest()),
                     lambda r: r.update(recipe="other-recipe"),
                     lambda r: r.update(camera="other-camera")]
        for mutate in mutations:
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                config, _ = measured_fixture(root)
                path = root / "validation.json"
                report = json.loads(path.read_bytes())
                mutate(report)
                data = json.dumps(report).encode()
                path.write_bytes(data)
                config["captures"][1]["statistics_sha256"] = hashlib.sha256(data).hexdigest()
                with self.assertRaises(ValueError):
                    c.calibrate(config, root)
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            config, _ = measured_fixture(root)
            for change in [lambda q: q["white"].update(capture="validation", patch="0"),
                           lambda q: q["patches"][0].update(exposure_scale=1.),
                           lambda q: q["patches"][0].update(camera_rgb=[.2]*3),
                           lambda q: q["captures"][0].update(statistics_sha256="0"*64)]:
                invalid = copy.deepcopy(config)
                change(invalid)
                with self.assertRaises(ValueError):
                    c.calibrate(invalid, root)
            plain, _ = fixture()
            plain["kind"] = "measured"
            with self.assertRaises(ValueError):
                c.calibrate(plain, root)

    def test_nonfinite_reference_cannot_report_success(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "reference.txt"
            for line in ["50 0 0 50 0 0 nan", "nan 0 0 50 0 0 0",
                         "50 0 0 50 0 inf 0", "50 0 0 50 0 0 -0.00001", ""]:
                with self.subTest(line=line), self.assertRaises(ValueError):
                    path.write_text(line)
                    c.reference_tests(path)
            path.write_text("50 0 0 50 0 0 0\n")
            self.assertTrue(c.reference_tests(path)["passed"])

    def test_wb_analysis_separates_precision_and_extended_domain(self):
        audit = importlib.import_module("analyze-raw-wb")
        m = [[1., 2., 3.], [0., 1., 4.], [5., 6., 0.]]
        identity = audit.multiply(m, audit.inverse(m))
        self.assertEqual(identity, [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]])
        sample = dict(continuous_rec2020=[.18]*3, quantized_rec2020=[.19, .18, .17],
                      precise_rec2020=[.18]*3, f64_projection=[.18]*3,
                      unsaturated_positive_camera=True)
        excluded = dict(sample, unsaturated_positive_camera=False)
        report = audit.analyze(dict(recipe="synthetic", limits="synthetic",
                                   cameras=[dict(camera="synthetic", source_sha256="test", profile_hash="test",
                                                 cases=[sample, excluded], rejected_wb_pairs=[])]))
        measured = report["cameras"][0]
        self.assertEqual(measured["outside_lab_comparison_domain"], 1)
        self.assertGreater(measured["wb_quantization_delta_e00"]["maximum"], .1)
        self.assertEqual(measured["precise_wb_delta_e00"]["maximum"], 0.)
        self.assertFalse(report["camera_color_accuracy_qualified"])
        rejected = [dict(continuous_error=None, quantized_error=None, precise_error="outside domain"),
                    dict(continuous_error="outside domain", quantized_error=None, precise_error=None)]
        data = dict(schema=2, recipe="synthetic", limits="synthetic", cameras=[dict(camera="synthetic",
                    source_sha256="test", profile_hash="test", cases=[sample], rejected_wb_pairs=rejected)])
        measured = audit.analyze(data)["cameras"][0]
        self.assertEqual(measured["wb_acceptance_changed"], 1)
        self.assertEqual(measured["precise_wb_acceptance_changed"], 2)

    def test_recovers_known_camera_and_preserves_white(self):
        config, expected = fixture()
        r = c.calibrate(config)
        for actual, row in zip(r["matrix_camera_to_xyz_d50"], expected):
            for x, y in zip(actual, row):
                self.assertAlmostEqual(x, y, places=12)
        self.assertLess(max(map(abs, r["white_residual"])), 1e-14)
        self.assertLess(r["validation"]["metrics"]["maximum"], 1e-11)
        self.assertTrue(r["validation"]["numerical_thresholds_passed"])
        self.assertFalse(r["qualified"])

    def test_validation_cannot_change_fit_and_detects_bad_color(self):
        config, _ = fixture()
        original = c.calibrate(config)
        for p in config["patches"]:
            if p["split"] == "validation":
                p["xyz"] = [p["xyz"][0] * 1.2, *p["xyz"][1:]]
        changed = c.calibrate(config)
        self.assertEqual(original["matrix_camera_to_xyz_d50"], changed["matrix_camera_to_xyz_d50"])
        self.assertFalse(changed["validation"]["numerical_thresholds_passed"])

    def test_rejects_data_leakage_degenerate_and_invalid_inputs(self):
        original, _ = fixture()
        mutations = [lambda p: p.update(capture="fit") if p["split"] == "validation" else None,
                     lambda p: p.update(camera_rgb=[.2, .2, .2]),
                     lambda p: p.update(weight=float("nan")),
                     lambda p: p.update(camera_rgb=[float("inf"), .2, .3]),
                     lambda p: p.update(camera_rgb=[1., .2, .3]),
                     lambda p: p.update(neutral=False),
                     lambda p: p.update(weight=-1)]
        for mutate in mutations:
            config = copy.deepcopy(original)
            for p in config["patches"]:
                mutate(p)
            with self.assertRaises(ValueError):
                c.calibrate(config)
        config = copy.deepcopy(original)
        config["patches"].append(config["patches"][0])
        with self.assertRaises(ValueError):
            c.calibrate(config)

    def test_lab_extended_domain_and_delta_e_invariants(self):
        for a, b in zip(c.lab(c.D50), (100., 0., 0.)):
            self.assertAlmostEqual(a, b, places=12)
        self.assertLess(c.lab([-.1, -.1, -.1])[0], 0)
        self.assertGreater(c.lab([2 * v for v in c.D50])[0], 100)
        for a in [(50., 0., 0.), (20., -10., 40.), (90., 60., -70.)]:
            self.assertEqual(c.delta_e(a, a), 0.)
            for b in [(0., 0., 0.), (30., 80., 20.), (75., -20., -55.)]:
                self.assertAlmostEqual(c.delta_e(a, b), c.delta_e(b, a), places=12)


if __name__ == "__main__":
    unittest.main()
