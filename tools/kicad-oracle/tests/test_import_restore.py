"""Q22.3: the grader's SES import puts the input copper back at its own coordinates
(`restore_copper.py` of the plugin, loaded by path), with KiCad's own pcbnew.

Needs a python3 that imports pcbnew (KiCad 10); skipped without one. Runs `kicad_steps.py import`
in a subprocess, as test_import_via_drill.py does, on make_legacy_board.py's board with three
input items added:
- net A's track (5, 10)-(10.0004, 10.0003), which the session gives back at (5, 10)-(10, 10)
  (the 0.4 / 0.3 µm of a `%.6g` export), and a routed A track from (10, 10) to (15, 10);
- net B's via at (12.0002, 12.0004), 0.6 / 0.3 mm, back at (12, 12);
- net B's track (5, 12)-(8, 12), which the session moves 2 µm to (5, 12)-(8.002, 12).
After the import the first two are at their nanometre coordinates with their own uuids, the
routed track's end has followed, and the moved track stays as imported. Never starts the KiCad GUI.
"""

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
sys.path.insert(0, HERE)

import make_legacy_board  # noqa: E402
import oracle_lib as lib  # noqa: E402
from test_import_via_drill import has_pcbnew  # noqa: E402

KICAD_PYTHON = os.environ.get("KICAD_PYTHON", "python3")
STEPS = os.path.join(HERE, "..", "kicad_steps.py")

INPUT_COPPER = """  (segment (start 5 10) (end 10.0004 10.0003) (width 0.25) (layer F.Cu) (net 1) (tstamp 1234ABCD))
  (segment (start 5 12) (end 8 12) (width 0.25) (layer F.Cu) (net 2) (tstamp 1234ABCE))
  (via (at 12.0002 12.0004) (size 0.6) (drill 0.3) (layers F.Cu B.Cu) (net 2) (tstamp 1234ABCF))
"""
# KiCad reads a KiCad 4 timestamp as this uuid
UUID_A, UUID_MOVED, UUID_VIA = ("00000000-0000-0000-0000-00001234abc" + c for c in "def")

SESSION = """(session "legacy"
  (base_design "legacy")
  (was_is
  )
  (routes
    (resolution um 10)
    (parser
      (host_cad "KiCad's Pcbnew")
      (host_version "10.0.7")
    )
    (library_out
      (padstack "Via[0-1]_600:300_um"
        (shape
          (circle F.Cu 6000 0 0)
        )
        (shape
          (circle B.Cu 6000 0 0)
        )
        (attach off)
      )
    )
    (network_out
      (net A
        (wire
          (path F.Cu 2500 50000 -100000 100000 -100000)
        )
        (wire
          (path F.Cu 2500 100000 -100000 150000 -100000)
        )
      )
      (net B
        (wire
          (path F.Cu 2500 50000 -120000 80020 -120000)
        )
        (via "Via[0-1]_600:300_um" 120000 -120000)
      )
    )
  )
)
"""


def add_input_copper(path):
    with open(path, encoding="utf-8") as f:
        text = f.read().rstrip()
    assert text.endswith(")")
    with open(path, "w", encoding="utf-8") as f:
        f.write(text[:-1] + INPUT_COPPER + ")\n")


@unittest.skipUnless(has_pcbnew(), "no python3 with pcbnew")
class ImportRestoreTest(unittest.TestCase):
    def test_input_copper_comes_back_and_routed_ends_follow(self):
        with tempfile.TemporaryDirectory() as tmp:
            with contextlib.redirect_stdout(io.StringIO()):
                make_legacy_board.main(tmp)
            board = os.path.join(tmp, "legacy", "legacy.kicad_pcb")
            add_input_copper(board)
            ses = os.path.join(tmp, "restore.ses")
            with open(ses, "w", encoding="utf-8") as f:
                f.write(SESSION)
            out = os.path.join(tmp, "routed.kicad_pcb")
            run = subprocess.run([KICAD_PYTHON, STEPS, "import", board, ses, out], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn("input copper: 1 track(s) and 1 via(s) back at their own coordinates, 1 routed end(s)", run.stdout)
            with open(out, encoding="utf-8") as f:
                copper = lib.read_copper(f.read())
        tracks = sorted((r["net"], r["start"], r["end"], r["width"]) for r in copper.values() if r["kind"] == "segment")
        self.assertEqual(
            tracks,
            [
                ("A", (5_000_000, 10_000_000), (10_000_400, 10_000_300), 250_000),
                ("A", (10_000_400, 10_000_300), (15_000_000, 10_000_000), 250_000),
                ("B", (5_000_000, 12_000_000), (8_002_000, 12_000_000), 250_000),
            ],
        )
        vias = [(r["net"], r["at"], r["size"], r["drill"]) for r in copper.values() if r["kind"] == "via"]
        self.assertEqual(vias, [("B", (12_000_200, 12_000_400), 600_000, 300_000)])
        # the restored items keep their uuids (K13.2); the moved track is a new item
        self.assertEqual(copper[UUID_A]["end"], (10_000_400, 10_000_300))
        self.assertEqual(copper[UUID_VIA]["at"], (12_000_200, 12_000_400))
        self.assertNotIn(UUID_MOVED, copper)


if __name__ == "__main__":
    unittest.main()
