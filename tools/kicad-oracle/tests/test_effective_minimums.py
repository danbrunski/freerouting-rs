"""K15.44: the copper statistics count `below_minimum` against the minimums KiCad's DRC checks
(`oracle_lib.effective_minimums`, used by `kicad_steps.py stats`): an unconditional `.kicad_dru`
rule replaces Board Setup's value, larger or smaller; a rule with a condition or layers leaves
Board Setup's. Plain python3, no pcbnew:

    python3 -m pytest tools/kicad-oracle/tests/test_effective_minimums.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import oracle_lib as lib  # noqa: E402

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
PCU_DRU = os.path.join(REPO, "crates", "freerouting-kicad", "tests", "data", "pcu-rev2.kicad_dru")

# The pcu board's Board Setup: track 0.2, via diameter 0.5, through hole 0.3 mm.
PCU_SETUP = {"track_width": 200000, "via_diameter": 500000, "via_drill": 300000}


class EffectiveMinimums(unittest.TestCase):
    def test_the_pcu_files_unconditional_rules_replace_board_setup(self):
        with open(PCU_DRU, encoding="utf-8") as f:
            text = f.read()
        minimums, sources = lib.effective_minimums(PCU_SETUP, text)
        # track 0.127 (below Board Setup's 0.2), hole 0.254 (below 0.3); no via_diameter rule
        self.assertEqual(minimums, {"track_width": 127000, "via_diameter": 500000, "via_drill": 254000})
        self.assertEqual(sources["track_width"], "kicad_dru: Minimum trace width (5 mil)")
        self.assertEqual(sources["via_drill"], "kicad_dru: Minimum finished hole size (10 mil)")
        self.assertEqual(sources["via_diameter"], "board_setup")

    def test_a_rule_above_board_setup_replaces_it_too(self):
        text = '(version 1)\n(rule "w" (constraint track_width (min 0.127mm)))\n'
        minimums, _ = lib.effective_minimums({"track_width": 100000}, text)
        self.assertEqual(minimums["track_width"], 127000)

    def test_no_rule_leaves_board_setup(self):
        for text in ("", "(version 1)\n", '(version 1)\n(rule "c" (constraint clearance (min 0.1mm)))\n'):
            minimums, sources = lib.effective_minimums(PCU_SETUP, text)
            self.assertEqual(minimums, PCU_SETUP, text)
            self.assertEqual(set(sources.values()), {"board_setup"}, text)

    def test_a_conditional_or_layer_rule_leaves_board_setup(self):
        text = (
            "(version 1)\n"
            "(rule \"power\" (constraint track_width (min 0.1mm)) (condition \"A.NetClass == 'Power'\"))\n"
            '(rule "top" (layer F.Cu) (constraint track_width (min 0.1mm)))\n'
            '(rule "off" (constraint track_width (min 0.1mm)) (severity ignore))\n'
        )
        minimums, sources = lib.effective_minimums(PCU_SETUP, text)
        self.assertEqual(minimums["track_width"], 200000)
        self.assertEqual(sources["track_width"], "board_setup")

    def test_the_last_unconditional_rule_wins_and_units_convert(self):
        text = (
            "(version 1)\n"
            "# a comment (with parentheses\n"
            '(rule "a" (constraint track_width (min 0.3mm)))\n'
            '(rule "b" (constraint track_width (min 5mil)) (constraint via_diameter (min 0.45)))\n'
        )
        minimums, sources = lib.effective_minimums(PCU_SETUP, text)
        self.assertEqual(minimums["track_width"], 127000)
        self.assertEqual(minimums["via_diameter"], 450000)
        self.assertEqual(sources["track_width"], "kicad_dru: b")


if __name__ == "__main__":
    unittest.main()
