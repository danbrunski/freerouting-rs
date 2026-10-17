"""Counts the vias whose centre lies inside a pad (D8: never a via in a pad).

    python3 via_in_pad.py BOARD.kicad_pcb OUT.json

Run with KiCad's python (imports `pcbnew`). Any pad counts, of any net and any
layer, including the hole of a through-hole pad. Writes `{"vias": N, "pads": M,
"vias_in_pads": K, "items": [...first 20...]}`. Never starts the KiCad GUI.
"""

import json
import sys

import pcbnew

CELL = 2_000_000  # 2 mm in nm


def cells(box):
    for gx in range(box.GetLeft() // CELL, box.GetRight() // CELL + 1):
        for gy in range(box.GetTop() // CELL, box.GetBottom() // CELL + 1):
            yield gx, gy


def main(path, out):
    board = pcbnew.LoadBoard(path)
    grid = {}
    pads = list(board.GetPads())
    for pad in pads:
        for cell in cells(pad.GetBoundingBox()):
            grid.setdefault(cell, []).append(pad)
    vias = 0
    hits = []
    for t in board.Tracks():
        if t.GetClass() != "PCB_VIA":
            continue
        vias += 1
        pos = t.GetPosition()
        for pad in grid.get((pos.x // CELL, pos.y // CELL), []):
            if pad.HitTest(pos):
                hits.append(
                    {
                        "via_net": t.GetNetname(),
                        "pad": f"{pad.GetParentFootprint().GetReference()}-{pad.GetNumber()}",
                        "pad_net": pad.GetNetname(),
                        "at_mm": [round(pos.x / 1e6, 3), round(pos.y / 1e6, 3)],
                    }
                )
                break
    with open(out, "w", encoding="utf-8") as f:
        json.dump({"vias": vias, "pads": len(pads), "vias_in_pads": len(hits), "items": hits[:20]}, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
