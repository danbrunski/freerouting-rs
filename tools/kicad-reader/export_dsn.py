"""K04.8: export a board's Specctra DSN with KiCad's own exporter.

    python3 -I export_dsn.py BOARD.kicad_pcb OUT.dsn

Runs under a KiCad interpreter (WSL: the system python3; Windows: KiCad's
python.exe). Loads the board with pcbnew.LoadBoard, writes OUT.dsn with
pcbnew.ExportSpecctraDSN and prints one line, `KICAD_VERSION <build version>`.
Exit status 0 on success, 1 when the export failed.

Never imports the freerouting-rs plugin package: register() outside the
editor raises a wxWidgets dialog on the desktop. Run it on a copy of the
board (tools/kicad-reader/export.sh does).
"""

import sys

import pcbnew


def main(board_path, out_path):
    board = pcbnew.LoadBoard(board_path)
    ok = pcbnew.ExportSpecctraDSN(board, out_path)
    # KiCad's leak messages share stdout at exit; a prefix keeps the line findable.
    print("KICAD_VERSION " + pcbnew.GetBuildVersion())
    return 0 if ok else 1


if len(sys.argv) != 3:
    print("usage: export_dsn.py BOARD.kicad_pcb OUT.dsn", file=sys.stderr)
    sys.exit(2)
sys.exit(main(sys.argv[1], sys.argv[2]))
