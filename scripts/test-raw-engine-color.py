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


def symmetric_registration():
    def search(dx, dy, cosine=.99, threshold=.90):
        return {"dx": dx, "dy": dy, "supported": cosine >= threshold,
                "gradient_cosine": cosine, "distant_runner_up_cosine": cosine - .10,
                "peak_gap": .10, "at_search_boundary": False}
    return {"method": "symmetric-binomial-v2", "dx": 3, "dy": -2, "supported": True,
            "minimum_gradient_cosine": .99, "fine": search(4, -2, .6, .85),
            "scales": [{"radius": radius, "forward": search(3, -2), "reverse": search(-3, 2)}
                       for radius in (2, 4)]}


class EngineColorTests(unittest.TestCase):
    def test_symmetric_geometry_preserves_unfiltered_color_differences(self):
        data = zones((.6, .5, .4))
        old = analysis.analyze_pair(data)
        for zone in data:
            zone["registration"] = symmetric_registration()
        self.assertEqual(analysis.analyze_pair(data, analysis.PROTOCOL_V2), old)

    def test_every_scale_and_direction_must_pass_even_with_strong_fine_geometry(self):
        for scale in (0, 1):
            for direction in ("forward", "reverse"):
                reg = symmetric_registration()
                reg["fine"] = copy.deepcopy(reg["scales"][0]["forward"])
                changed = reg["scales"][scale][direction]
                changed.update(gradient_cosine=.87, distant_runner_up_cosine=.77, supported=False)
                reg.update(minimum_gradient_cosine=.87, supported=False)
                self.assertFalse(analysis.validate_registration(reg, analysis.PROTOCOL_V2))
                reg["supported"] = True
                with self.assertRaises(ValueError):
                    analysis.validate_registration(reg, analysis.PROTOCOL_V2)

    def test_wrong_reciprocal_or_scale_offsets_cannot_be_admitted(self):
        for scale, direction in ((0, "reverse"), (1, "forward"), (1, "reverse")):
            reg = symmetric_registration()
            reg["scales"][scale][direction]["dx"] += 1
            with self.assertRaises(ValueError):
                analysis.validate_registration(reg, analysis.PROTOCOL_V2)
            reg["supported"] = False
            self.assertFalse(analysis.validate_registration(reg, analysis.PROTOCOL_V2))

    def test_missing_nonfinite_or_conflicting_multiscale_evidence_is_an_error(self):
        reg = symmetric_registration()
        for key in ("method", "fine", "scales", "minimum_gradient_cosine"):
            changed = copy.deepcopy(reg)
            del changed[key]
            with self.assertRaises(ValueError):
                analysis.validate_registration(changed, analysis.PROTOCOL_V2)
        for invalid in (None, math.nan, math.inf, -math.inf, .98):
            changed = copy.deepcopy(reg)
            changed["minimum_gradient_cosine"] = invalid
            with self.assertRaises(ValueError):
                analysis.validate_registration(changed, analysis.PROTOCOL_V2)
        for invalid in ([], reg["scales"][:1], reg["scales"][::-1], reg["scales"] * 2):
            changed = copy.deepcopy(reg)
            changed["scales"] = invalid
            with self.assertRaises(ValueError):
                analysis.validate_registration(changed, analysis.PROTOCOL_V2)
        changed = copy.deepcopy(reg)
        changed["dx"] = 4
        with self.assertRaises(ValueError):
            analysis.validate_registration(changed, analysis.PROTOCOL_V2)
        with self.assertRaises(ValueError):
            analysis.validate_registration(reg, analysis.PROTOCOL_V1)
        with self.assertRaises(ValueError):
            analysis.validate_registration(reg["fine"], analysis.PROTOCOL_V2)

    def test_missing_energy_is_retained_as_refusal_at_every_scale(self):
        reg = symmetric_registration()
        reg["scales"][1]["reverse"] = {"dx": 0, "dy": 0, "supported": False,
                                         "reason": "insufficient gradient energy"}
        reg.update(minimum_gradient_cosine=None, supported=False)
        self.assertFalse(analysis.validate_registration(reg, analysis.PROTOCOL_V2))
        reg["minimum_gradient_cosine"] = .99
        with self.assertRaises(ValueError):
            analysis.validate_registration(reg, analysis.PROTOCOL_V2)

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
