"""K07.4 step 3: import Specctra sessions into KiCad and dump what lands.

    python3 -I frame_check.py dump BOARD.kicad_pcb SESSION.ses OUT.json
    python3 -I frame_check.py compare A.json B.json [TOLERANCE_NM]

`dump` runs under a KiCad interpreter (WSL: the system python3; Windows:
KiCad's python.exe). It loads the board with pcbnew.LoadBoard, imports the
session with pcbnew.ImportSpecctraSES (the call the plugin makes) and writes
every track, arc and via in nanometres as JSON, sorted, with the build version.
`compare` needs no KiCad: it pairs the items of two dumps (same kind, layer, net
and width, in sorted order) and prints the largest endpoint distance; exit
status 1 when an item is unpaired or farther apart than TOLERANCE_NM (default
100, the 0.1 um board unit of a session in `(resolution um 10)`).

Never imports the freerouting-rs plugin package: register() outside the editor
raises a wxWidgets dialog on the desktop. frame-check.sh runs `dump` on copies.
"""

import json
import sys


def dump(board_path, ses_path, out_path):
    import pcbnew

    board = pcbnew.LoadBoard(board_path)
    ok = pcbnew.ImportSpecctraSES(board, ses_path)
    items = []
    for t in board.GetTracks():
        kind = t.GetClass()
        if kind == "PCB_VIA":
            p = t.GetPosition()
            items.append({"kind": "via", "layer": "%s-%s" % (
                board.GetLayerName(t.TopLayer()), board.GetLayerName(t.BottomLayer())),
                "net": t.GetNetname(), "width": t.GetWidth(),
                "start": [p.x, p.y], "end": [p.x, p.y]})
        else:
            s, e = t.GetStart(), t.GetEnd()
            items.append({"kind": "arc" if kind == "PCB_ARC" else "track",
                          "layer": t.GetLayerName(), "net": t.GetNetname(),
                          "width": t.GetWidth(), "start": [s.x, s.y], "end": [e.x, e.y]})
    for it in items:
        # a track has no direction: store the smaller end first
        if it["end"] < it["start"]:
            it["start"], it["end"] = it["end"], it["start"]
    items.sort(key=lambda i: (i["kind"], i["layer"], i["net"], i["width"], i["start"], i["end"]))
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump({"kicad": pcbnew.GetBuildVersion(), "import_ok": bool(ok), "items": items},
                  f, indent=1, sort_keys=True)
    print("KICAD_VERSION " + pcbnew.GetBuildVersion())
    print("IMPORTED %d items" % len(items))
    return 0 if ok else 1


def compare(a_path, b_path, tolerance=100):
    a = json.load(open(a_path, encoding="utf-8"))["items"]
    b = json.load(open(b_path, encoding="utf-8"))["items"]
    worst = 0.0
    bad = 0
    if len(a) != len(b):
        print("item counts differ: %d against %d" % (len(a), len(b)))
        bad += 1
    for x, y in zip(a, b):
        if (x["kind"], x["layer"], x["net"], x["width"]) != (y["kind"], y["layer"], y["net"], y["width"]):
            print("unpaired: %s against %s" % (x, y))
            bad += 1
            continue
        for k in ("start", "end"):
            d = ((x[k][0] - y[k][0]) ** 2 + (x[k][1] - y[k][1]) ** 2) ** 0.5
            worst = max(worst, d)
            if d > tolerance:
                bad += 1
                print("%s %s: %.1f nm apart (%s against %s)" % (x["kind"], k, d, x[k], y[k]))
    print("COMPARED %d items, largest endpoint distance %.1f nm, tolerance %d nm" % (min(len(a), len(b)), worst, tolerance))
    return 1 if bad else 0


if __name__ == "__main__":
    if len(sys.argv) == 5 and sys.argv[1] == "dump":
        sys.exit(dump(sys.argv[2], sys.argv[3], sys.argv[4]))
    if len(sys.argv) in (4, 5) and sys.argv[1] == "compare":
        sys.exit(compare(sys.argv[2], sys.argv[3], int(sys.argv[4]) if len(sys.argv) == 5 else 100))
    print(__doc__, file=sys.stderr)
    sys.exit(2)
