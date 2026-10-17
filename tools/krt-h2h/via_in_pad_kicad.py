"""Vias in SMD pads, counted on a routed .kicad_pcb with KiCad's python (X01.6).

    python3 via_in_pad_kicad.py BOARD.kicad_pcb [OUT.json]
    python3 via_in_pad_kicad.py plant BOARD.kicad_pcb OUT.kicad_pcb   (for the test)

A via is in a pad when its centre hits an SMD pad of any net on one of the
via's layers (`PAD.HitTest`, the pad's real shape). D8 makes every such via a
hard failure. The board-level counterpart of the Java fork's
`docs/development/tools/via_in_pad.py`, which reads a DSN and a SES; KRT writes
a board. Uses BOARD.Tracks(), never GetTracks() (fails in KiCad 10.0.5).
Prints `via_in_pad=<n>` and writes the sites to OUT.json. `plant` moves the
first via onto the centre of the first SMD pad on F.Cu or B.Cu and saves a
copy (the test of this script). Never imports the freerouting-rs plugin.
"""

import json
import sys

import pcbnew


def smd_pads(board):
    return [p for p in board.GetPads() if p.GetAttribute() == pcbnew.PAD_ATTRIB_SMD]


def vias(board):
    return [t for t in board.Tracks() if t.GetClass() == "PCB_VIA"]


def sites(board):
    pads = smd_pads(board)
    found = []
    for via in vias(board):
        at = via.GetPosition()
        for pad in pads:
            if not any(via.IsOnLayer(layer) for layer in pad.GetLayerSet().CuStack()):
                continue
            if pad.HitTest(at):
                found.append(
                    {
                        "via_at_mm": [pcbnew.ToMM(at.x), pcbnew.ToMM(at.y)],
                        "via_net": via.GetNetname(),
                        "pad": f"{pad.GetParentFootprint().GetReference()}.{pad.GetNumber()}",
                        "pad_net": pad.GetNetname(),
                    }
                )
                break
    return found


def count(path, out=None):
    board = pcbnew.LoadBoard(path)
    found = sites(board)
    result = {"board": path, "vias": len(vias(board)), "via_in_pad": len(found), "sites": found}
    if out:
        with open(out, "w", encoding="utf-8") as f:
            json.dump(result, f, indent=2)
    print(f"via_in_pad={len(found)}")
    return 0


def plant(path, out):
    board = pcbnew.LoadBoard(path)
    pads = [p for p in smd_pads(board) if p.IsOnLayer(pcbnew.F_Cu) or p.IsOnLayer(pcbnew.B_Cu)]
    all_vias = vias(board)
    if not pads or not all_vias:
        print("no SMD pad or no via to plant", file=sys.stderr)
        return 1
    all_vias[0].SetPosition(pads[0].GetPosition())
    pcbnew.SaveBoard(out, board)
    return 0


def main(argv):
    if len(argv) == 3 and argv[0] == "plant":
        return plant(argv[1], argv[2])
    if len(argv) in (1, 2) and argv[0] != "plant":
        return count(*argv)
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
