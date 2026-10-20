"""Q22.2: the grader's SES import sets a routed via's drill back to its session padstack's.

Needs a python3 that imports pcbnew (KiCad 10); skipped without one. Runs `kicad_steps.py import`
in a subprocess (a second LoadBoard in one process returns an unwrapped object in 10.0.7). The
board is make_legacy_board.py's with net A moved to a `Power` class whose vias are 0.8 / 0.4 mm
(Default: 0.6 / 0.3); the session puts a `Via[0-1]_600:300_um` on net A. KiCad's
ImportSpecctraSES gives it 0.6 / 0.4 (the class's drill: canary-pwr's four `annular_width` items);
the plugin's `fix_via_drills` (K09) and now the grader set it to 0.3 mm. Never starts the KiCad
GUI.
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

KICAD_PYTHON = os.environ.get("KICAD_PYTHON", "python3")
STEPS = os.path.join(HERE, "..", "kicad_steps.py")

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
          (path F.Cu 2000 50000 -100000 150000 -100000)
        )
        (via "Via[0-1]_600:300_um" 100000 -50000)
      )
    )
  )
)
"""


POWER_CLASS = """  (net_class Power "p"
    (clearance 0.15)
    (trace_width 0.2)
    (via_dia 0.8)
    (via_drill 0.4)
    (uvia_dia 0.3)
    (uvia_drill 0.1)
    (add_net A)
  )
"""


def power_board(path):
    """make_legacy_board's board with net A in a class of its own and Default vias 0.6 / 0.3."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    text = text.replace("    (add_net A)\n", "", 1).replace("(via_drill 0.4)", "(via_drill 0.3)")
    text = text.replace("  (module", POWER_CLASS + "  (module", 1)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def has_pcbnew():
    probe = subprocess.run([KICAD_PYTHON, "-c", "import pcbnew"], capture_output=True)
    return probe.returncode == 0


@unittest.skipUnless(has_pcbnew(), "no python3 with pcbnew")
class ImportViaDrillTest(unittest.TestCase):
    def import_session(self, tmp):
        with contextlib.redirect_stdout(io.StringIO()):
            make_legacy_board.main(tmp)
        power_board(os.path.join(tmp, "legacy", "legacy.kicad_pcb"))
        ses = os.path.join(tmp, "via.ses")
        with open(ses, "w", encoding="utf-8") as f:
            f.write(SESSION)
        out = os.path.join(tmp, "routed.kicad_pcb")
        run = subprocess.run(
            [KICAD_PYTHON, STEPS, "import", os.path.join(tmp, "legacy", "legacy.kicad_pcb"), ses, out],
            capture_output=True,
            text=True,
        )
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        with open(out, encoding="utf-8") as f:
            return lib.read_copper(f.read())

    def test_a_600_300_via_on_a_0_8_0_4_class_gets_drill_0_3(self):
        with tempfile.TemporaryDirectory() as tmp:
            copper = self.import_session(tmp)
        vias = [r for r in copper.values() if r["kind"] == "via"]
        self.assertEqual(len(vias), 1, copper)
        self.assertEqual((vias[0]["size"], vias[0]["drill"]), (600_000, 300_000))
        self.assertEqual(vias[0]["net"], "A")


if __name__ == "__main__":
    unittest.main()
