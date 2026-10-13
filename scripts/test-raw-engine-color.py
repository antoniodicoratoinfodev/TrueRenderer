#!/usr/bin/env python3
"""Analytical regressions for diagnostic color comparisons."""
import copy
import importlib
import math
import unittest

analysis = importlib.import_module("analyze-raw-engine-color")


def block(rgb):
    return {"rgb": list(rgb), "minimum": min(rgb), "maximum": max(rgb),
            "y_mean": sum(x * y for x, y in zip(rgb, (.2627002120112671, .6779980715188708, .0593017164698620))),
            "y_stddev": 0.}


def zones(gains=(.5, .5, .5)):
    result = []
    for zi in range(9):
        rows = []
        for y in range(32, 288, 16):
            for x in range(32, 288, 16):
                value = .1 + (x + y + zi) / 4000
                rgb = [value] * 3
                rows.append({"xy": [x, y], "anchor": block(rgb),
                             "comparison": block([v * g for v, g in zip(rgb, gains)])})
        result.append({"registration": {"dx": 0, "dy": 0, "supported": True,
                                        "gradient_cosine": .99, "distant_runner_up_cosine": .89,
                                        "peak_gap": .10, "at_search_boundary": False}, "blocks": rows})
    return result


class EngineColorTests(unittest.TestCase):
    def test_single_exposure_factor_recovers_known_linear_neutral(self):
        result = analysis.analyze_pair(zones())
        self.assertEqual(result["comparison_exposure_gain"], 2.)
        self.assertEqual(result["registered_zones"], 9)
        self.assertGreater(result["raw"]["delta_e00"]["median"], 10)
        self.assertLess(result["exposure_normalized"]["delta_e00"]["maximum"], 1e-10)
        self.assertEqual(result["low_chroma_anchor_cab_le_8"]["delta_e00"]["count"], 9 * 256)
        lab = analysis.color.lab(analysis.color.transform(analysis.MATRIX, [1., 1., 1.]))
        self.assertLess(max(abs(a - b) for a, b in zip(lab, [100., 0., 0.])), 1e-8)

    def test_exposure_cannot_hide_a_channel_bias(self):
        result = analysis.analyze_pair(zones((.6, .5, .4)))
        self.assertGreater(result["exposure_normalized"]["delta_e00"]["minimum"], 4.)
        self.assertGreater(result["exposure_normalized"]["delta_lab"][2]["median"], 0.)
        self.assertGreater(abs(result["exposure_normalized"]["delta_xy"][0]["median"]), .001)

    def test_inadequate_geometry_cannot_produce_an_exposure_estimate(self):
        data = zones()
        for zone in data[2:]:
            zone["registration"] = {"dx": 0, "dy": 0, "supported": False,
                                     "reason": "insufficient gradient energy"}
        result = analysis.analyze_pair(data)
        self.assertFalse(result["sufficient_for_exposure_estimate"])
        self.assertIsNone(result["comparison_exposure_gain"])
        self.assertEqual(result["excluded_blocks"]["weak_geometry"], 7 * 256)
        self.assertEqual(result["exposure_normalized"]["delta_e00"]["count"], 0)

    def test_single_extended_sample_is_excluded_before_and_after_scaling(self):
        data = zones()
        data[0]["blocks"][0]["anchor"]["maximum"] = 1.
        data[0]["blocks"][1]["comparison"]["maximum"] = .7
        result = analysis.analyze_pair(data)
        self.assertEqual(result["excluded_blocks"]["sample_range"], 1)
        self.assertEqual(result["after_scale_excluded_blocks"]["sample_range"], 1)
        self.assertEqual(result["raw"]["delta_e00"]["count"], 9 * 256 - 1)
        self.assertEqual(result["raw_on_normalized_subset"]["delta_e00"]["count"], 9 * 256 - 2)
        self.assertEqual(result["exposure_normalized"]["delta_e00"]["count"], 9 * 256 - 2)

    def test_nonfinite_values_are_errors_even_in_excluded_regions(self):
        data = zones()
        data[0]["registration"] = {"dx": 0, "dy": 0, "supported": False,
                                    "reason": "insufficient gradient energy"}
        for key in ("minimum", "maximum", "y_mean", "y_stddev"):
            for invalid in (math.nan, math.inf, -math.inf):
                changed = copy.deepcopy(data)
                changed[0]["blocks"][0]["comparison"][key] = invalid
                with self.assertRaises(ValueError):
                    analysis.analyze_pair(changed)

    def test_incomplete_or_tampered_geometry_is_rejected(self):
        data = zones()
        with self.assertRaises(ValueError):
            analysis.analyze_pair(data[:8])
        data[0]["blocks"][1]["xy"] = data[0]["blocks"][0]["xy"]
        with self.assertRaises(ValueError):
            analysis.analyze_pair(data)
        data = zones()
        data[0]["registration"]["supported"] = False
        with self.assertRaises(ValueError):
            analysis.analyze_pair(data)
        data = zones()
        data[0]["blocks"][0]["comparison"]["y_mean"] *= 2
        with self.assertRaises(ValueError):
            analysis.analyze_pair(data)


if __name__ == "__main__":
    unittest.main()
