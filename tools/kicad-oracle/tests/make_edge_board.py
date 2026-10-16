"""KiCad python: writes a tiny unrouted board whose only DRC item is one pad
that crosses the board edge, for the oracle's pre-existing-item test.

    python3 make_edge_board.py OUT_DIR

OUT_DIR/edge.kicad_pcb: a 20 x 20 mm outline, net N1 on two SMD pads 10 mm
apart (one unconnected item), the second pad 0.2 mm from the right edge.
"""

import os
import sys

import pcbnew


def mm(x, y):
    return pcbnew.VECTOR2I(pcbnew.FromMM(x), pcbnew.FromMM(y))


def main(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    board = pcbnew.BOARD()
    corners = [(0, 0), (20, 0), (20, 20), (0, 20)]
    for i, start in enumerate(corners):
        end = corners[(i + 1) % len(corners)]
        edge = pcbnew.PCB_SHAPE(board)
        edge.SetShape(pcbnew.SHAPE_T_SEGMENT)
        edge.SetLayer(pcbnew.Edge_Cuts)
        edge.SetStart(mm(*start))
        edge.SetEnd(mm(*end))
        edge.SetWidth(pcbnew.FromMM(0.1))
        board.Add(edge)
    net = pcbnew.NETINFO_ITEM(board, "N1")
    board.Add(net)
    for ref, x in (("U1", 9.8), ("U2", 19.8)):
        footprint = pcbnew.FOOTPRINT(board)
        footprint.SetReference(ref)
        footprint.Reference().SetVisible(False)
        footprint.Value().SetVisible(False)
        footprint.SetPosition(mm(x, 10))
        pad = pcbnew.PAD(footprint)
        pad.SetAttribute(pcbnew.PAD_ATTRIB_SMD)
        pad.SetLayerSet(pad.SMDMask())
        pad.SetSize(pcbnew.F_Cu, mm(0.6, 0.6))
        pad.SetNumber("1")
        pad.SetPosition(mm(x, 10))
        footprint.Add(pad)
        board.Add(footprint)
        pad.SetNet(net)
    path = os.path.join(out_dir, "edge.kicad_pcb")
    pcbnew.SaveBoard(path, board)
    print(path)


if __name__ == "__main__":
    main(sys.argv[1])
