"""The KiCad steps of the DRC oracle, run with a python3 that can import pcbnew.

    python3 kicad_steps.py version
    python3 kicad_steps.py strip IN.kicad_pcb OUT_UNROUTED.kicad_pcb [--keep-tracks]
    python3 kicad_steps.py export BOARD.kicad_pcb OUT.dsn AREAS.json
    python3 kicad_steps.py import UNROUTED.kicad_pcb SESSION.ses OUT.kicad_pcb
    python3 kicad_steps.py stats BOARD.kicad_pcb CLASS_WIDTHS.json OUT.json
    python3 kicad_steps.py pads BOARD.kicad_pcb OUT.json [--all]

Never imports the freerouting-rs plugin package. Started from the 2026-10-15
head-to-head scripts prep.py, import_ses.py and stats.py. Uses
BOARD.Tracks() (GetTracks() fails in KiCad 10.0.5).

`import` also applies the plugin's via-drill correction (Q22.2): KiCad's
ImportSpecctraSES takes a routed via's drill from its net class, not from the
session's padstack (`Via[0-3]_600:300_um` is 0.6 / 0.3), and the plugin sets it
back (`ses_vias.vias`, `kicad_board.fix_via_drills`). Both modules are plain
Python files loaded by path from kicad-plugin/freerouting_rs, never through the
plugin package.
"""

import json
import sys
from pathlib import Path

import pcbnew

PLUGIN_MODULES = Path(__file__).resolve().parent.parent.parent / "kicad-plugin" / "freerouting_rs"


def dsn_point(point):
    """A board point in the um, Y-negated coordinates of KiCad's DSN export."""
    return [point.x / 1000.0, -point.y / 1000.0]


def rule_areas(board):
    """Board-level rule areas with their flags and outlines (DSN um)."""
    areas = []
    for zone in board.Zones():
        if not zone.GetIsRuleArea():
            continue
        outline = zone.Outline().Outline(0)
        areas.append(
            {
                "name": zone.GetZoneName(),
                "layers": [board.GetLayerName(layer) for layer in zone.GetLayerSet().Seq()],
                "outline": [dsn_point(outline.CPoint(i)) for i in range(outline.PointCount())],
                "tracks": bool(zone.GetDoNotAllowTracks()),
                "vias": bool(zone.GetDoNotAllowVias()),
                "pads": bool(zone.GetDoNotAllowPads()),
                "footprints": bool(zone.GetDoNotAllowFootprints()),
                "copper_pour": bool(zone.GetDoNotAllowZoneFills()),
            }
        )
    return areas


def strip(source, unrouted, keep_tracks):
    """Saves `source` without its tracks and vias (zones kept) as `unrouted`."""
    board = pcbnew.LoadBoard(source)
    removed = 0
    if not keep_tracks:
        for track in list(board.Tracks()):
            board.Remove(track)
            removed += 1
    pcbnew.SaveBoard(unrouted, board)
    # stdout mixes with SWIG's leak messages at exit; leave the count in a file.
    with open(unrouted + ".strip.json", "w", encoding="utf-8") as f:
        json.dump({"removed_tracks_and_vias": removed}, f)
    return 0


def export(path, dsn, areas_json):
    """Exports the DSN of `path` and lists its rule areas. A separate process
    from `strip`: in KiCad 10.0.7 exporting right after removing tracks
    crashed, and a second LoadBoard in one process returned an unwrapped
    SwigPyObject."""
    board = pcbnew.LoadBoard(path)
    if not pcbnew.ExportSpecctraDSN(board, dsn):
        print("DSN export failed", file=sys.stderr)
        return 1
    with open(areas_json, "w", encoding="utf-8") as f:
        json.dump({"rule_areas": rule_areas(board)}, f)
    return 0


class _TracksBoard:
    """`fix_via_drills` calls GetTracks(), which fails in KiCad 10.0.5; Tracks() works."""

    def __init__(self, board):
        self._board = board

    def GetTracks(self):
        return self._board.Tracks()


def fix_via_drills(board, ses):
    """The plugin's correction of the vias ImportSpecctraSES drilled with the class's drill.
    Returns the number of vias changed."""
    sys.path.insert(0, str(PLUGIN_MODULES))
    try:
        import kicad_board
        import ses_vias
    finally:
        sys.path.remove(str(PLUGIN_MODULES))
    with open(ses, encoding="utf-8", errors="replace") as f:
        session_vias = ses_vias.vias(f.read())
    return kicad_board.fix_via_drills(_TracksBoard(board), session_vias)


def import_session(unrouted, ses, out):
    board = pcbnew.LoadBoard(unrouted)
    if not pcbnew.ImportSpecctraSES(board, ses):
        print("SES import failed", file=sys.stderr)
        return 1
    changed = fix_via_drills(board, ses)
    pcbnew.SaveBoard(out, board)
    print("SES imported, fix_via_drills reported %d" % changed)
    return 0


def stats(path, class_widths_json, out):
    """Copper statistics counted from the geometry, not from the DRC (whose
    per-type counts were capped at 199 in KiCad 10): tracks, vias, length,
    copper narrower than its net class (KiCad's DRC never checks that), and
    tracks, vias and drills below the board minimums."""
    board = pcbnew.LoadBoard(path)
    ds = board.GetDesignSettings()
    with open(class_widths_json, encoding="utf-8") as f:
        class_widths = json.load(f)
    tracks = vias = 0
    length = 0.0
    under_n = 0
    under_len = 0.0
    worst = None
    under_nets = {}
    below = {"track_width": 0, "via_diameter": 0, "via_drill": 0}
    for t in board.Tracks():
        if t.GetClass() == "PCB_VIA":
            vias += 1
            if t.GetWidth(pcbnew.F_Cu) < ds.m_ViasMinSize:
                below["via_diameter"] += 1
            if t.GetDrillValue() < ds.m_MinThroughDrill:
                below["via_drill"] += 1
            continue
        if t.GetClass() not in ("PCB_TRACK", "PCB_ARC"):
            continue
        tracks += 1
        track_length = t.GetLength()
        length += track_length
        width = t.GetWidth()
        if width < ds.m_TrackMinWidth:
            below["track_width"] += 1
        class_mm = class_widths.get(t.GetNetname())
        class_width = pcbnew.FromMM(class_mm) if class_mm else None
        if class_width and width < class_width - 1:  # 1 nm tolerance
            under_n += 1
            under_len += track_length
            net = t.GetNetname()
            under_nets[net] = under_nets.get(net, 0.0) + pcbnew.ToMM(track_length)
            shortfall = pcbnew.ToMM(class_width - width)
            if worst is None or shortfall > worst["shortfall_mm"]:
                worst = {
                    "shortfall_mm": round(shortfall, 3),
                    "net": net,
                    "width_mm": round(pcbnew.ToMM(width), 3),
                    "class_width_mm": round(class_mm, 3),
                }
    top = sorted(under_nets.items(), key=lambda kv: -kv[1])[:5]
    result = {
        "tracks": tracks,
        "vias": vias,
        "track_length_mm": round(pcbnew.ToMM(length), 1),
        "under_class_width_tracks": under_n,
        "under_class_width_mm": round(pcbnew.ToMM(under_len), 1),
        "worst_under_class_width": worst,
        "top_under_class_width_nets_mm": [[n, round(v, 1)] for n, v in top],
        "below_minimum": below,
        "board_minimums_mm": {
            "track_width": pcbnew.ToMM(ds.m_TrackMinWidth),
            "via_diameter": pcbnew.ToMM(ds.m_ViasMinSize),
            "via_drill": pcbnew.ToMM(ds.m_MinThroughDrill),
        },
    }
    with open(out, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2)
    return 0


def pads(path, out, all_pads=False):
    """SMD pads (all pads with --all) with net, DSN position and bounding
    box, for the tests."""
    board = pcbnew.LoadBoard(path)
    result = []
    for pad in board.GetPads():
        if not all_pads and pad.GetAttribute() != pcbnew.PAD_ATTRIB_SMD:
            continue
        box = pad.GetBoundingBox()
        result.append(
            {
                "net": pad.GetNetname(),
                "footprint": pad.GetParentFootprint().GetReference(),
                "number": pad.GetNumber(),
                "layer": board.GetLayerName(pad.GetLayer()),
                "smd": pad.GetAttribute() == pcbnew.PAD_ATTRIB_SMD,
                "at": dsn_point(pad.GetPosition()),
                "right": box.GetRight() / 1000.0,
                "size": [pad.GetSizeX() / 1000.0, pad.GetSizeY() / 1000.0],
                "orientation": pad.GetOrientationDegrees(),
            }
        )
    with open(out, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2)
    return 0


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    command, args = argv[0], argv[1:]
    if command == "version":
        print(pcbnew.Version())
        return 0
    if command == "strip" and len(args) in (2, 3):
        return strip(*args[:2], keep_tracks="--keep-tracks" in args[2:])
    if command == "export" and len(args) == 3:
        return export(*args)
    if command == "import" and len(args) == 3:
        return import_session(*args)
    if command == "stats" and len(args) == 3:
        return stats(*args)
    if command == "pads" and len(args) in (2, 3):
        return pads(*args[:2], all_pads="--all" in args[2:])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
