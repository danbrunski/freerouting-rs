"""Prepares an X01 input board, run with KiCad's python (pcbnew), on a copy.

    python3 prep_board.py strip IN.kicad_pcb OUT.kicad_pcb COUNTS.json
    python3 prep_board.py refill BOARD.kicad_pcb COUNTS.json
    python3 prep_board.py count BOARD.kicad_pcb COUNTS.json

`strip` removes every track, arc and via and keeps zones (and layer types);
`refill` refills every zone and saves the board in place; `count` writes the
board's tracks, arcs, vias and zones. Each step is its own process: in KiCad
10.0.7 exporting right after removing tracks in one process crashed, and a
second LoadBoard in one process returned an unwrapped SwigPyObject (see
tools/kicad-oracle/README.md). SWIG's leak messages mix with stdout, so the
counts go to a file. Uses BOARD.Tracks(), never GetTracks() (fails in 10.0.5).
Never imports the freerouting-rs plugin package. `prep.sh` drives it.
"""

import json
import sys

import pcbnew


def counts(board):
    tracks = arcs = vias = 0
    for t in board.Tracks():
        kind = t.GetClass()
        if kind == "PCB_VIA":
            vias += 1
        elif kind == "PCB_ARC":
            arcs += 1
        elif kind == "PCB_TRACK":
            tracks += 1
    zones = list(board.Zones())
    return {
        "tracks": tracks,
        "arcs": arcs,
        "vias": vias,
        "zones": len(zones),
        "copper_zones": sum(1 for z in zones if not z.GetIsRuleArea()),
        "rule_areas": sum(1 for z in zones if z.GetIsRuleArea()),
        "layer_types": {
            board.GetLayerName(layer): pcbnew.LAYER.ShowType(board.GetLayerType(layer))
            for layer in board.GetEnabledLayers().CuStack()
        },
        "footprints": len(board.GetFootprints()),
    }


def write(path, data):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)


def strip(source, out, counts_json):
    board = pcbnew.LoadBoard(source)
    before = counts(board)
    for track in list(board.Tracks()):
        board.Remove(track)
    pcbnew.SaveBoard(out, board)
    write(counts_json, {"before": before})
    return 0


def refill(path, counts_json):
    board = pcbnew.LoadBoard(path)
    filler = pcbnew.ZONE_FILLER(board)
    filled = filler.Fill(board.Zones())
    pcbnew.SaveBoard(path, board)
    write(counts_json, {"refilled": bool(filled)})
    return 0


def count(path, counts_json):
    write(counts_json, counts(pcbnew.LoadBoard(path)))
    return 0


def main(argv):
    if len(argv) == 4 and argv[0] == "strip":
        return strip(*argv[1:])
    if len(argv) == 3 and argv[0] == "refill":
        return refill(*argv[1:])
    if len(argv) == 3 and argv[0] == "count":
        return count(*argv[1:])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
