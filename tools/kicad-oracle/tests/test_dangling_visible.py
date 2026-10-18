"""Unit tests of dangling_visible's DSN helpers (plain python3, no pcbnew):

    python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import dangling_visible as dv  # noqa: E402

DSN = """(pcb "x"
  (resolution um 10)
  (unit um)
  (structure
    (plane +3V3_PI (polygon B.Cu 0  127150 -63200  126560 -62600  124410 -62600  124410 -63140
            124616 -63325  125310 -63325  125410 -63200  126560 -63200  127150 -63200))
    (plane "/A B" (polygon F.Cu 0  0 0  1000 0  1000 -1000  0 -1000  0 0))
  )
)
"""


class DsnPlanes(unittest.TestCase):
    def planes(self):
        with tempfile.NamedTemporaryFile("w", suffix=".dsn", delete=False) as f:
            f.write(DSN)
        try:
            return dv.dsn_planes(f.name)
        finally:
            os.unlink(f.name)

    def test_planes_are_read_in_mm_with_kicads_y_axis(self):
        planes = self.planes()
        self.assertEqual(sorted(planes), [("+3V3_PI", "B.Cu"), ("/A B", "F.Cu")])
        self.assertEqual(planes[("/A B", "F.Cu")][0][:3], [(0.0, -0.0), (1.0, -0.0), (1.0, 1.0)])

    def test_q16_3_cm5_track_end_is_inside_the_plane_outline(self):
        polygon = self.planes()[("+3V3_PI", "B.Cu")][0]
        self.assertTrue(dv.inside((124.485, 62.98), polygon))
        self.assertFalse(dv.inside((124.0, 62.98), polygon))

    def test_borders_count_as_inside(self):
        square = self.planes()[("/A B", "F.Cu")][0]
        self.assertTrue(dv.inside((1.0, 0.5), square))
        self.assertTrue(dv.inside((0.0, 0.0), square))
        self.assertFalse(dv.inside((1.0001, 0.5), square))


if __name__ == "__main__":
    unittest.main()
