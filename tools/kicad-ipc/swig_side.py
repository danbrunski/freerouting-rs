"""K11.1: the KiCad 10 (SWIG) side of the IPC probe, for comparison.

    python3 swig_side.py export BOARD.kicad_pcb OUT.dsn
    python3 swig_side.py import BOARD.kicad_pcb SESSION.ses OUT.kicad_pcb
    python3 swig_side.py lock BOARD.kicad_pcb OUT.kicad_pcb N

Runs under a KiCad interpreter with `pcbnew` (WSL: the system python3), on copies
of boards only.
- `export` is the plugin's `ExportSpecctraDSN`.
- `import` is its `ImportSpecctraSES`, saved to OUT so `probe.py items` reads both
  KiCads' results the same way.
- `lock` locks the first N tracks and the first via, to check that both imports
  skip locked items.

Never imports the freerouting-rs plugin package: register() outside the editor
raises a wxWidgets dialog on the desktop.
"""

import sys


def main(argv):
    import pcbnew

    print("KICAD_VERSION " + pcbnew.GetBuildVersion())
    command, board_path = argv[0], argv[1]
    board = pcbnew.LoadBoard(board_path)
    if command == "export":
        ok = pcbnew.ExportSpecctraDSN(board, argv[2])
        print(f"EXPORT ok={bool(ok)}")
        return 0 if ok else 1
    if command == "import":
        ok = pcbnew.ImportSpecctraSES(board, argv[2])
        pcbnew.SaveBoard(argv[3], board, aSkipSettings=True)
        print(f"IMPORT ok={bool(ok)}")
        return 0 if ok else 1
    if command == "lock":
        tracks = [t for t in board.GetTracks() if t.GetClass() == "PCB_TRACK"][: int(argv[3])]
        vias = [t for t in board.GetTracks() if t.GetClass() == "PCB_VIA"][:1]
        for t in tracks + vias:
            t.SetLocked(True)
        pcbnew.SaveBoard(argv[2], board, aSkipSettings=True)
        print(f"LOCKED tracks={len(tracks)} vias={len(vias)}")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
