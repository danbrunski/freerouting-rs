"""K04.5: what KiCad 10 makes of the old arc and overbar syntax.

Usage: python3 -I tools/kicad-reader/old_arcs.py <scratch dir>

Writes a (version 20210925) board with six old-format gr_arc items and a
(version 20201115) board with overbar net names into <scratch dir>, loads each
with pcbnew.LoadBoard and prints what KiCad holds. The numbers are pinned in
crates/freerouting-kicad/tests/legacy_kicad.rs. Never imports the plugin package.
"""
import os
import sys

import pcbnew

ARCS = [((0, 0), (10, 0), 90), ((0, 0), (10, 0), -90), ((5, 5), (5, 10), 180),
        ((0, 0), (10, 0), 30), ((1.234567, 2.345678), (9.1, 3.3), 123.4),
        ((1.234567, 2.345678), (9.1, 3.3), -200.5)]

HEAD = '''(kicad_pcb (version %d) (generator pcbnew)
  (general (thickness 1.6)) (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "~RESET~") (net 2 "A~~B") (net 3 "~OE GND")
'''


def main(scratch):
    os.makedirs(scratch, exist_ok=True)
    arcs = HEAD % 20210925 + "".join(
        '  (gr_arc (start %s %s) (end %s %s) (angle %s) (layer "Edge.Cuts") (width 0.1))\n' % (c + e + (a,))
        for c, e, a in ARCS) + ")\n"
    p = os.path.join(scratch, "old_arcs.kicad_pcb")
    open(p, "w").write(arcs)
    b = pcbnew.LoadBoard(p)
    for d in b.GetDrawings():
        if d.GetShape() == pcbnew.SHAPE_T_ARC:
            print("ARC", d.GetStart().x, d.GetStart().y, d.GetArcMid().x, d.GetArcMid().y, d.GetEnd().x, d.GetEnd().y)
    p = os.path.join(scratch, "old_bars.kicad_pcb")
    open(p, "w").write(HEAD % 20201115 + ")\n")
    b = pcbnew.LoadBoard(p)
    for name in sorted(str(n) for n, _ in b.GetNetsByName().items()):
        print("NET", name)


main(sys.argv[1])
