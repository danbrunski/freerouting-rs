"""Splits KiCad's dangling copper into what the DSN shows and the plane-fill limit (Q16.4).

    python3 dangling_visible.py ROUTED.kicad_pcb DRC.json BOARD.dsn OUT.json

Run with KiCad's python (imports `pcbnew`) on the board `kicad-cli pcb drc
--refill-zones --save-board` saved (its zones filled) and that DRC's JSON
report. The DSN is the one the router read.

A router sees a plane as its DSN outline (`(plane NET (polygon LAYER ...))`),
KiCad as the zone fill poured inside it. A trace may end, or a via land,
inside the outline where the fill does not reach: the router counts a plane
contact, KiCad reports `track_dangling` or `via_dangling`. Such a finding is
not visible from the DSN (Q16.3, CM5_MINIMA_3). This script counts a
`via_dangling` as that limit when the via's centre lies, on one of its
layers, inside a DSN plane polygon of its net and outside the KiCad fill of
its net there; a `track_dangling` when one of its ends does so on the
track's layer. Polygon borders count as inside.

Writes `{"via_dangling": V, "track_dangling": T, "via_dangling_dsn": V - v,
"track_dangling_dsn": T - t, "limit": [...]}`. Never starts the KiCad GUI.
"""

import json
import re
import sys

try:  # KiCad's python; the pure helpers below run without it (tests/test_dangling_visible.py)
    import pcbnew
except ImportError:  # pragma: no cover
    pcbnew = None

PLANE = re.compile(r'\(plane\s+("[^"]*"|\S+)\s+\(polygon\s+(\S+)\s+\S+\s+([-\d.\s]+)\)')


def dsn_planes(path):
    """{(net, layer): [polygon, ...]} with points in mm, KiCad's y axis."""
    text = open(path, encoding="utf-8", errors="replace").read()
    unit = re.search(r"\(unit\s+(\w+)\)", text) or re.search(r"\(resolution\s+(\w+)", text)
    scale = {"um": 0.001, "mm": 1.0, "mil": 0.0254, "inch": 25.4}.get(unit.group(1) if unit else "um", 0.001)
    planes = {}
    for net, layer, numbers in PLANE.findall(text):
        values = [float(v) for v in numbers.split()]
        points = [(values[i] * scale, -values[i + 1] * scale) for i in range(0, len(values) - 1, 2)]
        planes.setdefault((net.strip('"'), layer), []).append(points)
    return planes


def on_segment(p, a, b):
    (px, py), (ax, ay), (bx, by) = p, a, b
    cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    if abs(cross) > 1e-9:
        return False
    return min(ax, bx) - 1e-9 <= px <= max(ax, bx) + 1e-9 and min(ay, by) - 1e-9 <= py <= max(ay, by) + 1e-9


def inside(point, polygon):
    """Even-odd test, borders included."""
    x, y = point
    result = False
    for i, a in enumerate(polygon):
        b = polygon[(i + 1) % len(polygon)]
        if on_segment(point, a, b):
            return True
        if (a[1] > y) != (b[1] > y):
            if x < a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]):
                result = not result
    return result


def filled(board, net, layer_id, point):
    vector = pcbnew.VECTOR2I(pcbnew.FromMM(point[0]), pcbnew.FromMM(point[1]))
    return any(
        zone.GetNetname() == net and zone.IsOnLayer(layer_id) and zone.HitTestFilledArea(layer_id, vector)
        for zone in board.Zones()
    )


def limit_at(board, planes, net, layer_ids, point):
    """The first layer on which `point` is in a DSN plane of `net` and outside its fill."""
    for layer_id in layer_ids:
        name = board.GetLayerName(layer_id)
        if any(inside(point, poly) for poly in planes.get((net, name), [])) and not filled(board, net, layer_id, point):
            return name
    return None


def mm(vector):
    return (pcbnew.ToMM(vector.x), pcbnew.ToMM(vector.y))


def main(board_path, drc_path, dsn_path, out_path):
    board = pcbnew.LoadBoard(board_path)
    planes = dsn_planes(dsn_path)
    tracks = {t.m_Uuid.AsString(): t for t in board.GetTracks()}
    report = json.load(open(drc_path))
    counts = {"via_dangling": 0, "track_dangling": 0}
    limits = []
    for violation in report.get("violations", []):
        kind = violation.get("type")
        if kind not in counts:
            continue
        counts[kind] += 1
        item = tracks.get(violation["items"][0].get("uuid", ""))
        if item is None:
            continue
        net = item.GetNetname()
        if kind == "via_dangling":
            via = pcbnew.Cast_to_PCB_VIA(item) if hasattr(pcbnew, "Cast_to_PCB_VIA") else item
            layers = list(via.GetLayerSet().CuStack())
            points = [mm(via.GetPosition())]
        else:
            layers = [item.GetLayer()]
            points = [mm(item.GetStart()), mm(item.GetEnd())]
        for point in points:
            layer = limit_at(board, planes, net, layers, point)
            if layer:
                limits.append({"type": kind, "net": net, "layer": layer, "at_mm": [round(v, 4) for v in point]})
                break
    result = dict(counts)
    for kind in ("via_dangling", "track_dangling"):
        result[kind + "_dsn"] = counts[kind] - sum(1 for entry in limits if entry["type"] == kind)
    result["limit"] = limits
    json.dump(result, open(out_path, "w"), indent=1)
    print(json.dumps({k: v for k, v in result.items() if k != "limit"}))
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(*sys.argv[1:]))
