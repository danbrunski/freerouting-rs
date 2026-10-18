"""Counts routed tracks that cross another net's pour (K09.2's check).

    python3 -I kicad-plugin/pour_crossings.py INPUT.kicad_pcb ROUTED.kicad_pcb CLASS[,CLASS...]

The pours are those of INPUT (the board before routing, its fills as saved)
whose net is in one of the listed net classes, on F.Cu and B.Cu. A track of
ROUTED on the same layer, of another net, whose centreline enters such a fill
is a crossing: KiCad's refill would cut the pour there. A track whose
centreline stays outside but whose copper reaches over the fill's edge is a
graze, listed with how far it reaches (the fixed wires of `dsn_fixups`
approximate the fill with 0.25 mm strips, so a few tens of micrometres can
stay uncovered at slanted edges; the refill only pulls the edge back).
Prints `pour crossings: N, edge grazes: M (max X mm)` and one line per item;
exits 1 when there are crossings, like canary's `inner_tracks.py`.

Runs under a Python that imports pcbnew (WSL: the system python3 of KiCad
10.0.7). It never imports the plugin package.
"""

import math
import sys

import pcbnew


def pours(board, classes):
    out = []
    for zone in board.Zones():
        if zone.GetIsRuleArea() or not zone.IsFilled():
            continue
        if zone.GetNetClassName() not in classes:
            continue
        for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
            if zone.IsOnLayer(layer):
                out.append((zone.GetNetname(), layer, zone.GetFilledPolysList(layer)))
    return out


def crossings(input_path, routed_path, classes):
    fills = pours(pcbnew.LoadBoard(input_path), classes)
    routed = pcbnew.LoadBoard(routed_path)
    mm = pcbnew.ToMM
    hits, grazes = [], []
    for track in routed.GetTracks():
        if track.GetClass() == "PCB_VIA":
            continue
        seg = pcbnew.SEG(track.GetStart(), track.GetEnd())
        for net, layer, fill in fills:
            if track.GetLayer() != layer or track.GetNetname() == net:
                continue
            half = track.GetWidth() // 2
            if not fill.Collide(seg, half):
                continue
            a, b = track.GetStart(), track.GetEnd()
            where = f"{track.GetNetname()} on {routed.GetLayerName(layer)}, the {net} pour: ({mm(a.x):.2f}, {mm(a.y):.2f}) -> ({mm(b.x):.2f}, {mm(b.y):.2f})"
            if fill.Collide(seg, 0):
                hits.append("crosses " + where)
            else:
                reach = mm(half - math.sqrt(fill.SquaredDistanceToSeg(seg)))
                grazes.append((reach, f"grazes by {reach:.3f} mm " + where))
    return hits, grazes


def main(argv):
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    hits, grazes = crossings(argv[0], argv[1], set(filter(None, argv[2].split(","))))
    deepest = max((r for r, _ in grazes), default=0.0)
    print(f"pour crossings: {len(hits)}, edge grazes: {len(grazes)} (max {deepest:.3f} mm)")
    for h in hits + [g for _, g in grazes]:
        print("  " + h)
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
