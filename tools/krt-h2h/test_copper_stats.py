"""Tests of copper_stats.py and via_in_pad_kicad.py (X01.6).

    python3 -m unittest discover -s tools/krt-h2h -p 'test_*.py'

The via-in-pad test needs a python3 that imports pcbnew and the KiCad demos
(/usr/share/kicad/demos); it is skipped without them.
"""

import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import copper_stats  # noqa: E402


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def save(path, text):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)

INPUT = """(kicad_pcb (version 20241229) (generator "pcbnew")
  (net 0 "") (net 1 "A") (net 2 "B")
  (footprint "R" (layer "F.Cu") (at 5 5)
    (fp_line (start 0 0) (end 9 9) (layer "F.SilkS") (width 0.1))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "A")))
  (segment (start 0 0) (end 3 4) (width 0.25) (layer "F.Cu") (net 1) (uuid "a"))
  (zone (net 2) (layer "B.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
)
"""

# The input's segment, two more segments, a quarter circle of radius 2 and two vias.
ROUTED = INPUT.replace(
    '  (zone',
    """  (segment (start 10 0) (end 10 2) (width 0.25) (layer "B.Cu") (net 2) (uuid "b"))
  (segment (start 0 0) (end 1 1) (width 0.25) (layer "F.Cu") (net 1) (uuid "c"))
  (arc (start 2 0) (mid 1.414213562373095 1.414213562373095) (end 0 2) (width 0.2)
    (layer "In1.Cu") (net 1) (uuid "d"))
  (via (at 10 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2) (uuid "e"))
  (via (at 1 1) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "f"))
  (zone""",
)


class CopperStatsTest(unittest.TestCase):
    def test_hand_written_board_gives_the_hand_computed_length_and_two_vias(self):
        result = copper_stats.compare(ROUTED, INPUT)
        routed = result["routed"]
        self.assertEqual((routed["segments"], routed["arcs"], routed["vias"]), (3, 1, 2))
        # 5 (input segment) + 2 + sqrt(2) + pi * 2 / 2 (quarter circle of radius 2).
        hand_total = 5.0 + 2.0 + 1.4142135623730951 + 3.141592653589793
        self.assertAlmostEqual(routed["length_mm"], hand_total, delta=1e-3 * 1e-3)
        added = result["added"]
        self.assertEqual(added["vias"], 2)
        self.assertAlmostEqual(added["length_mm"], hand_total - 5.0, delta=1e-6)
        self.assertAlmostEqual(added["per_layer_mm"]["In1.Cu"], math.pi, delta=1e-6)
        self.assertAlmostEqual(added["per_layer_mm"]["B.Cu"], 2.0, delta=1e-6)
        self.assertAlmostEqual(added["per_layer_mm"]["F.Cu"], math.sqrt(2), delta=1e-6)
        self.assertEqual(result["new"]["vias"], 2)
        self.assertEqual(result["lost"], {"segments": 0, "arcs": 0, "vias": 0, "length_mm": 0.0})

    def test_footprint_and_zone_geometry_is_not_copper(self):
        result = copper_stats.compare(INPUT, INPUT)
        self.assertEqual(result["input"]["segments"], 1)
        self.assertEqual(result["input"]["length_mm"], 5.0)
        self.assertEqual(result["added"]["length_mm"], 0.0)

    def test_arc_length_follows_the_mid_point(self):
        r = 3.0
        s = math.sqrt(0.5) * r
        # Clockwise quarter: (0,3) -> (s,s) -> (3,0).
        self.assertAlmostEqual(copper_stats.arc_length((0, r), (s, s), (r, 0)), math.pi * r / 2, places=9)
        # Three quarters: (3,0) -> (-s,-s)... mid on the far side.
        self.assertAlmostEqual(copper_stats.arc_length((r, 0), (-r, 0), (0, -r)), 1.5 * math.pi * r, places=9)
        # Collinear points are a straight line.
        self.assertAlmostEqual(copper_stats.arc_length((0, 0), (1, 1), (2, 2)), math.sqrt(8), places=12)

    def test_cli_writes_json(self):
        with tempfile.TemporaryDirectory() as d:
            routed, inp, out = (os.path.join(d, n) for n in ("r.kicad_pcb", "i.kicad_pcb", "o.json"))
            save(routed, ROUTED)
            save(inp, INPUT)
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "copper_stats.py"), routed, inp, out],
                           check=True, capture_output=True)
            self.assertEqual(load(out)["added"]["vias"], 2)


def kicad_python():
    """A python3 that imports pcbnew, or None."""
    exe = os.environ.get("KICAD_PYTHON", "python3")
    try:
        ok = subprocess.run([exe, "-c", "import pcbnew"], capture_output=True, timeout=120).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return None
    return exe if ok else None


DEMO = os.environ.get("VIA_IN_PAD_DEMO", "/usr/share/kicad/demos/kit-dev-coldfire-xilinx_5213/kit-dev-coldfire-xilinx_5213.kicad_pcb")
# (video itself has 13 same-net vias in SMD pads as the human laid it out.)


@unittest.skipUnless(os.path.exists(DEMO) and kicad_python(), "needs pcbnew and the KiCad demos")
class ViaInPadTest(unittest.TestCase):
    def run_script(self, *args):
        return subprocess.run([kicad_python(), os.path.join(HERE, "via_in_pad_kicad.py"), *args],
                              capture_output=True, text=True, timeout=600)

    def test_one_via_moved_into_a_pad_is_reported_exactly_once(self):
        with tempfile.TemporaryDirectory() as d:
            board = os.path.join(d, os.path.basename(DEMO))
            shutil.copy(DEMO, board)
            os.chmod(board, 0o644)
            before = os.path.join(d, "before.json")
            self.run_script(board, before)
            self.assertEqual(load(before)["via_in_pad"], 0, "the demo already has a via in a pad")
            planted = os.path.join(d, "planted.kicad_pcb")
            self.assertEqual(self.run_script("plant", board, planted).returncode, 0)
            after = os.path.join(d, "after.json")
            self.run_script(planted, after)
            result = load(after)
            self.assertEqual(result["via_in_pad"], 1, result["sites"])
            self.assertEqual(result["vias"], load(before)["vias"])


if __name__ == "__main__":
    unittest.main()
