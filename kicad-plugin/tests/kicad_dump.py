"""Run by KiCad's Python (not by pytest): loads a copy of a .kicad_pcb,
exports its DSN and dumps the rule areas collected by kicad_areas.

    python kicad_dump.py board.kicad_pcb out.dsn out.json

Never imports the plugin package (its register() needs a running editor).
"""

import json
import sys
from pathlib import Path

import pcbnew

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "freerouting_rs"))
import kicad_areas  # noqa: E402

board = pcbnew.LoadBoard(sys.argv[1])
if not pcbnew.ExportSpecctraDSN(board, sys.argv[2]):
    sys.exit("ExportSpecctraDSN failed")
with open(sys.argv[3], "w", encoding="utf-8") as f:
    json.dump({"version": pcbnew.Version(), "areas": kicad_areas.collect(board, pcbnew)}, f)
