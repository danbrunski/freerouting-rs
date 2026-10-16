"""K06.5: the total length of the tracks of a board, as pcbnew computes it.

    python3 -I tools/kicad-reader/track_length.py BOARD.kicad_pcb

Run under a KiCad interpreter (WSL: the system python3). Prints
`KICAD_VERSION <v>`, then `TRACKS <n> ARCS <n> LENGTH_NM <sum of GetLength()>`
over segments and arcs with a net (net 0 is skipped like the DSN export does).
Loads a copy-free read-only board; never writes it.
"""
import sys

import pcbnew

if len(sys.argv) != 2:
    print("usage: track_length.py BOARD.kicad_pcb", file=sys.stderr)
    sys.exit(2)
b = pcbnew.LoadBoard(sys.argv[1])
total, n, arcs = 0, 0, 0
for t in b.GetTracks():
    if t.Type() not in (pcbnew.PCB_TRACE_T, pcbnew.PCB_ARC_T):
        continue
    if t.GetNetCode() == 0:
        continue
    n += 1
    arcs += t.Type() == pcbnew.PCB_ARC_T
    total += t.GetLength()
print("KICAD_VERSION " + pcbnew.GetBuildVersion())
print("TRACKS %d ARCS %d LENGTH_NM %d" % (n, arcs, total))
