"""K15.24: the DSN export's planes cut where a higher-priority zone of another
net fills (`kicad_board.plane_cuts`, `plane_priority.apply`,
`export_flow.fix_plane_priorities`).

The fake pcbnew below does KiCad's polygon arithmetic for axis-parallel
rectangles only (a region is a set of cells between the corners' x and y
values), which is all these boards need. The last test runs the same flow
through KiCad's own pcbnew and its DSN export, and is skipped without it.
"""

from types import SimpleNamespace

import pytest

import export_flow
import kicad_board
import plane_priority
from dsn_fixups import Dsn

MM = 1_000_000
F_CU, B_CU, IN1_CU, IN2_CU = 0, 2, 4, 6
NAMES = {F_CU: "F.Cu", B_CU: "B.Cu", IN1_CU: "In1.Cu", IN2_CU: "In2.Cu"}


# --- a fake SHAPE_POLY_SET for rectangles -------------------------------------


class Point:
    def __init__(self, x, y):
        self.x, self.y = x, y


class Chain:
    def __init__(self, points):
        self.points = list(points)

    def PointCount(self):
        return len(self.points)

    def CPoint(self, k):
        return Point(*self.points[k])


def _inside(rings, x, y):
    """Even-odd over every ring."""
    hit = False
    for ring in rings:
        for (xa, ya), (xb, yb) in zip(ring, ring[1:] + ring[:1]):
            if (ya > y) != (yb > y) and x < xa + (y - ya) * (xb - xa) / (yb - ya):
                hit = not hit
    return hit


def _signed_area(ring):
    return sum(xa * yb - xb * ya for (xa, ya), (xb, yb) in zip(ring, ring[1:] + ring[:1])) / 2


class PolySet:
    """Outlines, each `[outline ring, hole ring, ...]` of (x, y) nm."""

    def __init__(self, polys=None):
        self.polys = [[list(r) for r in p] for p in (polys or [])]

    def CloneDropTriangulation(self):
        return PolySet(self.polys)

    def OutlineCount(self):
        return len(self.polys)

    def Outline(self, i):
        return Chain(self.polys[i][0])

    def HoleCount(self, i):
        return len(self.polys[i]) - 1

    def Hole(self, i, h):
        return Chain(self.polys[i][h + 1])

    def AddOutline(self, chain):
        self.polys.append([list(chain.points)])
        return len(self.polys) - 1

    def Append(self, other):
        self.polys += [[list(r) for r in p] for p in other.polys]

    def _rings(self):
        return [r for p in self.polys for r in p]

    def Area(self):
        xs, ys = self._grid(self)
        return sum(
            (xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j])
            for i, j in self._cells(self, xs, ys)
        )

    def Inflate(self, amount, strategy, max_error):
        assert strategy == "round" and max_error == 5000
        grown = []
        for p in self.polys:
            assert len(p) == 1 and len(p[0]) == 4, "the fake inflates rectangles only"
            xs, ys = [x for x, _ in p[0]], [y for _, y in p[0]]
            x0, y0, x1, y1 = min(xs) - amount, min(ys) - amount, max(xs) + amount, max(ys) + amount
            grown.append([[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]])
        self.polys = grown

    def BooleanSubtract(self, other):
        xs, ys = self._grid(self, other)
        cells = self._cells(self, xs, ys) - self._cells(other, xs, ys)
        self.polys = _trace(cells, xs, ys)

    @staticmethod
    def _grid(*sets):
        pts = [pt for s in sets for r in s._rings() for pt in r]
        return sorted({x for x, _ in pts}), sorted({y for _, y in pts})

    @staticmethod
    def _cells(s, xs, ys):
        rings = s._rings()
        return {
            (i, j)
            for i in range(len(xs) - 1)
            for j in range(len(ys) - 1)
            if _inside(rings, (xs[i] + xs[i + 1]) / 2, (ys[j] + ys[j + 1]) / 2)
        }


def _trace(cells, xs, ys):
    """The rings round a set of grid cells: outlines and their holes."""
    nxt = {}
    for i, j in cells:
        x0, x1, y0, y1 = xs[i], xs[i + 1], ys[j], ys[j + 1]
        if (i, j - 1) not in cells:
            nxt[(x0, y0)] = (x1, y0)
        if (i + 1, j) not in cells:
            nxt[(x1, y0)] = (x1, y1)
        if (i, j + 1) not in cells:
            nxt[(x1, y1)] = (x0, y1)
        if (i - 1, j) not in cells:
            nxt[(x0, y1)] = (x0, y0)
    rings = []
    while nxt:
        start = min(nxt)
        ring, p = [], start
        while True:
            ring.append(p)
            p = nxt.pop(p)
            if p == start:
                break
        # drop the corners between collinear edges
        keep = [
            b
            for a, b, c in zip(ring[-1:] + ring[:-1], ring, ring[1:] + ring[:1])
            if (b[0] - a[0]) * (c[1] - b[1]) != (b[1] - a[1]) * (c[0] - b[0])
        ]
        rings.append(keep)
    outer = [r for r in rings if _signed_area(r) > 0]
    holes = [r for r in rings if _signed_area(r) < 0]
    return [[o] + [h for h in holes if _inside([o], *h[0])] for o in outer]


def rect(x0, y0, x1, y1):
    """A rectangle in mm as a fake SHAPE_POLY_SET in nm."""
    a, b, c, d = (round(v * MM) for v in (x0, y0, x1, y1))
    return PolySet([[[(a, b), (c, b), (c, d), (a, d)]]])


# --- a fake board ---------------------------------------------------------------


class Uuid:
    def __init__(self, text):
        self.text = text

    def AsString(self):
        return self.text


class Layers:
    def __init__(self, layers):
        self.layers = list(layers)

    def Seq(self):
        return sorted(self.layers)


class Zone:
    def __init__(self, net, code, layers, outline, priority, uuid, clearance=0, rule_area=False):
        self.net, self.code, self.layers, self.outline = net, code, layers, outline
        self.priority, self.m_Uuid, self.clearance, self.rule_area = priority, Uuid(uuid), clearance, rule_area

    def GetIsRuleArea(self):
        return self.rule_area

    def IsOnCopperLayer(self):
        return any(layer in NAMES for layer in self.layers)

    def GetLayerSet(self):
        return Layers(self.layers)

    def IsOnLayer(self, layer):
        return layer in self.layers

    def GetNetname(self):
        return self.net

    def GetNetCode(self):
        return self.code

    def GetAssignedPriority(self):
        return self.priority

    def GetLocalClearance(self):
        return self.clearance

    def HigherPriority(self, other):  # KiCad's ZONE::HigherPriority, without teardrops
        if self.priority != other.priority:
            return self.priority > other.priority
        return self.m_Uuid.text > other.m_Uuid.text

    def Outline(self):
        return self.outline


class Footprint:
    def __init__(self, zones=()):
        self.zones = list(zones)

    def Zones(self):
        return self.zones


class Board:
    def __init__(self, zones, footprints=()):
        self.zones, self.footprints = zones, list(footprints)

    def Zones(self):
        return self.zones

    def GetFootprints(self):
        return self.footprints

    def GetLayerName(self, layer):
        return NAMES[layer]

    def GetDesignSettings(self):
        return SimpleNamespace(m_MaxError=5000)


PCBNEW = SimpleNamespace(
    SHAPE_POLY_SET=PolySet,
    CORNER_STRATEGY_ROUND_ALL_CORNERS="round",
    IsCopperLayer=lambda layer: layer in NAMES,
    ToMM=lambda v: v / MM,
)


# --- the DSN as KiCad's export writes it ---------------------------------------


def _ring_text(ring_mm):
    pts = ring_mm + ring_mm[:1]
    return "  ".join(f"{x * 1000:g} {-y * 1000:g}" for x, y in pts)


def plane_text(net, layer, box):
    x0, y0, x1, y1 = box
    ring = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    return f"    (plane {net} (polygon {layer} 0  {_ring_text(ring)}))\n"


def dsn_text(planes):
    return (
        "(pcb test.dsn\n"
        '  (parser\n    (string_quote ")\n    (space_in_quoted_tokens on)\n'
        '    (host_cad "KiCad\'s Pcbnew")\n    (host_version "10.0.7")\n  )\n'
        "  (resolution um 10)\n  (unit um)\n  (structure\n"
        "    (layer F.Cu\n      (type signal)\n      (property\n        (index 0)\n      )\n    )\n"
        "    (layer In1.Cu\n      (type power)\n      (property\n        (index 1)\n      )\n    )\n"
        "    (layer In2.Cu\n      (type power)\n      (property\n        (index 2)\n      )\n    )\n"
        "    (layer B.Cu\n      (type signal)\n      (property\n        (index 3)\n      )\n    )\n"
        "    (boundary\n      (path pcb 0  0 0  40000 0  40000 -20000  0 -20000  0 0)\n    )\n"
        + "".join(plane_text(*p) for p in planes)
        + '    (via "Via[0-3]_600:300_um")\n    (rule\n      (width 200)\n      (clearance 200)\n    )\n'
        "  )\n  (network\n    (net LOW\n      (pins)\n    )\n    (net HIGH\n      (pins)\n    )\n  )\n)\n"
    )


LOW_BOX = (0, 0, 20, 10)  # mm, KiCad frame


def two_zone_board(high_box, high_net="HIGH", high_code=2, low_priority=1, high_priority=41, clearance=0):
    low = Zone("LOW", 1, [IN2_CU], rect(*LOW_BOX), low_priority, "aaaa")
    high = Zone(high_net, high_code, [IN2_CU], rect(*high_box), high_priority, "bbbb", clearance)
    board = Board([low, high])
    text = dsn_text([("LOW", "In2.Cu", LOW_BOX), (high_net, "In2.Cu", high_box)])
    return board, text


def plane_regions(text):
    """{net: [rings of each of its planes, mm in KiCad's frame]}: border first,
    then windows."""
    dsn = Dsn(text)
    out = {}
    for node, net, _layer in dsn.planes():
        rings = []
        for shape in [c for c in node.children[2:] if c.is_list]:
            poly = shape if shape.head == "polygon" else shape.children[1]
            vals = [float(t.text) for t in poly.children[3:]]
            rings.append([(vals[k] / 1000, -vals[k + 1] / 1000) for k in range(0, len(vals), 2)])
        out.setdefault(net, []).append(rings)
    return out


def covered(regions, x, y):
    return any(_inside(rings, x, y) for rings in regions)


def cut(board, text):
    return plane_priority.apply(text, kicad_board.plane_cuts(board, PCBNEW))


# --- the tests -------------------------------------------------------------------


def test_the_lower_plane_leaves_out_the_higher_zones_area():
    """V_BUS_12V (priority 1) under V_BUS_24V (41) on the pcu's In2.Cu, as two
    rectangles: the higher zone reaches 5 mm into the lower one's right edge.
    The lower plane leaves out the higher zone's area grown by its clearance
    (0.5 mm, plus KiCad's 5 um arc error outside); the higher plane is kept as
    exported."""
    board, text = two_zone_board((15, 3, 25, 7), clearance=500_000)
    new, log = cut(board, text)
    regions = plane_regions(new)
    assert len(regions["LOW"]) == 1
    low = regions["LOW"]
    # the higher zone's area and its clearance band are out of the lower plane
    for x, y in ((17, 5), (15.1, 3.1), (14.6, 5), (19.9, 6.9), (16, 2.6)):
        assert not covered(low, x, y), (x, y)
    # the rest of the lower outline is still in
    for x, y in ((5, 5), (14.4, 5), (17, 1), (17, 9), (19.9, 2.4), (0.1, 9.9)):
        assert covered(low, x, y), (x, y)
    border = sorted(low[0][0][:-1])
    assert border == sorted(
        [(0, 0), (20, 0), (20, 2.495), (14.495, 2.495), (14.495, 7.505), (20, 7.505), (20, 10), (0, 10)]
    )
    # the higher plane's text is KiCad's
    assert plane_text("HIGH", "In2.Cu", (15, 3, 25, 7)) in new
    assert new.count("(plane ") == 2
    assert log == [
        "plane LOW on In2.Cu: cut where 1 higher-priority zone(s) of another net fill (HIGH), "
        "200.00 -> 172.42 mm2, 1 plane(s), 0 window(s)"
    ]
    # a zone without a clearance of its own: 0.5 mm, as the `.kicad_pcb` reader takes it
    board, text = two_zone_board((15, 3, 25, 7), clearance=None)
    assert cut(board, text) == (new, log)


def test_a_higher_zone_inside_the_lower_one_becomes_a_window():
    board, text = two_zone_board((8, 3, 12, 7))
    new, log = cut(board, text)
    (low,) = plane_regions(new)["LOW"]
    assert len(low) == 2  # the border and one window
    assert not covered([low], 10, 5) and covered([low], 7.9, 5) and covered([low], 10, 2.9)
    assert sorted(low[1][:-1]) == sorted([(8, 3), (12, 3), (12, 7), (8, 7)])  # no clearance: no growth
    assert log[0].endswith("1 plane(s), 1 window(s)")


def test_a_cut_through_the_lower_zone_leaves_two_planes():
    """KiCad fills the two sides as two islands; the router gets one plane each."""
    board, text = two_zone_board((8, -1, 12, 11))
    new, log = cut(board, text)
    low = plane_regions(new)["LOW"]
    assert len(low) == 2
    assert covered(low, 5, 5) and covered(low, 15, 5) and not covered(low, 10, 5)
    assert log[0].endswith("2 plane(s), 0 window(s)")


def test_a_lower_zone_under_a_higher_one_is_left_out():
    board, text = two_zone_board((-1, -1, 21, 11))
    new, log = cut(board, text)
    assert "LOW" not in plane_regions(new)
    assert new.count("(plane ") == 1
    assert log == [
        "plane LOW on In2.Cu: left out, every part of it lies where 1 higher-priority zone(s) "
        "of another net fill (HIGH)"
    ]


@pytest.mark.parametrize(
    "case",
    [
        dict(high_box=(25, 0, 35, 10)),  # apart
        dict(high_box=(20.6, 0, 30, 10), clearance=500_000),  # apart by more than the clearance
        dict(high_box=(15, 3, 25, 7), high_net="LOW", high_code=1),  # the same net
    ],
)
def test_without_an_overlap_of_a_higher_zone_the_dsn_is_byte_identical(case):
    board, text = two_zone_board(**case)
    assert cut(board, text) == (text, [])


def test_the_order_is_kicads_priority_then_uuid():
    """With the priorities swapped the other plane is cut; with equal
    priorities the larger UUID (HIGH's "bbbb") wins."""
    board, text = two_zone_board((15, 3, 25, 7), low_priority=50)
    new, log = cut(board, text)
    assert plane_text("LOW", "In2.Cu", LOW_BOX) in new
    assert not covered(plane_regions(new)["HIGH"], 17, 5) and covered(plane_regions(new)["HIGH"], 22, 5)
    assert len(log) == 1 and log[0].startswith("plane HIGH on In2.Cu:")
    board, text = two_zone_board((15, 3, 25, 7), low_priority=7, high_priority=7)
    new, log = cut(board, text)
    assert not covered(plane_regions(new)["LOW"], 17, 5)
    assert len(log) == 1 and log[0].startswith("plane LOW on In2.Cu:")


def test_the_higher_zone_cuts_only_its_own_layer():
    low = Zone("LOW", 1, [IN1_CU, IN2_CU], rect(*LOW_BOX), 1, "aaaa")
    high = Zone("HIGH", 2, [IN2_CU], rect(15, 3, 25, 7), 41, "bbbb")
    board = Board([low, high])
    planes = [("LOW", "In1.Cu", LOW_BOX), ("LOW", "In2.Cu", LOW_BOX), ("HIGH", "In2.Cu", (15, 3, 25, 7))]
    text = dsn_text(planes)
    new, log = cut(board, text)
    assert plane_text("LOW", "In1.Cu", LOW_BOX) in new
    assert plane_text("LOW", "In2.Cu", LOW_BOX) not in new
    assert len(log) == 1 and log[0].startswith("plane LOW on In2.Cu:")


def test_a_footprint_zone_cuts_and_a_rule_area_does_not():
    low = Zone("LOW", 1, [IN2_CU], rect(*LOW_BOX), 1, "aaaa")
    keepout = Zone("", 0, [IN2_CU], rect(1, 1, 3, 3), 99, "cccc", rule_area=True)
    owned = Zone("HIGH", 2, [IN2_CU], rect(15, 3, 25, 7), 41, "bbbb")
    board = Board([low, keepout], [Footprint([owned])])
    text = dsn_text([("LOW", "In2.Cu", LOW_BOX)])
    new, log = cut(board, text)
    (low_rings,) = plane_regions(new)["LOW"]
    assert covered([low_rings], 2, 2) and not covered([low_rings], 17, 5)
    assert len(log) == 1


def test_a_plane_list_that_does_not_match_the_board_is_left_as_it_is():
    board, text = two_zone_board((15, 3, 25, 7))
    text = text.replace(plane_text("HIGH", "In2.Cu", (15, 3, 25, 7)), "")
    new, log = cut(board, text)
    assert new == text
    assert log == [
        "WARNING: planes not cut by zone priority: the DSN's planes do not match the board's "
        "zones (In2.Cu HIGH: 0 in the DSN, 1 on the board)"
    ]


def test_netless_zones_take_the_exports_names():
    low = Zone("", 0, [IN2_CU], rect(*LOW_BOX), 1, "aaaa")
    high = Zone("HIGH", 2, [IN2_CU], rect(15, 3, 25, 7), 41, "bbbb")
    board = Board([low, high])
    text = dsn_text([("@:no_net_0", "In2.Cu", LOW_BOX), ("HIGH", "In2.Cu", (15, 3, 25, 7))])
    new, log = cut(board, text)
    assert not covered(plane_regions(new)["@:no_net_0"], 17, 5)
    assert log[0].startswith("plane @:no_net_0 on In2.Cu:")


def test_fix_plane_priorities_rewrites_the_file_and_never_raises(tmp_path):
    board, text = two_zone_board((15, 3, 25, 7))
    dsn = tmp_path / "board.dsn"
    dsn.write_bytes(text.encode("utf-8"))
    lines = export_flow.fix_plane_priorities(board, PCBNEW, dsn)
    assert len(lines) == 1 and lines[0].startswith("plane LOW on In2.Cu:")
    assert not covered(plane_regions(dsn.read_text(encoding="utf-8"))["LOW"], 17, 5)
    # nothing to cut: the file is not written
    board, text = two_zone_board((25, 0, 35, 10))
    dsn.write_bytes(text.encode("utf-8"))
    before = dsn.stat().st_mtime_ns
    assert export_flow.fix_plane_priorities(board, PCBNEW, dsn) == []
    assert dsn.read_bytes() == text.encode("utf-8") and dsn.stat().st_mtime_ns == before
    # a failure is a warning, and the file stays
    broken = SimpleNamespace(Zones=lambda: 1 / 0, GetDesignSettings=board.GetDesignSettings)
    lines = export_flow.fix_plane_priorities(broken, PCBNEW, dsn)
    assert lines[0].startswith("WARNING: planes not cut by zone priority: ZeroDivisionError")
    assert dsn.read_bytes() == text.encode("utf-8")


# --- through KiCad's own pcbnew (skipped without it) ----------------------------


def _kicad_zone(pcbnew, board, net, box_mm, priority, clearance_mm=None):
    zone = pcbnew.ZONE(board)
    zone.SetLayer(pcbnew.In2_Cu)
    if net is not None:
        zone.SetNet(net)
    x0, y0, x1, y1 = (pcbnew.FromMM(v) for v in box_mm)
    outline = zone.Outline()
    outline.NewOutline()
    for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)):
        outline.Append(x, y)
    zone.SetAssignedPriority(priority)
    if clearance_mm is not None:
        zone.SetLocalClearance(pcbnew.FromMM(clearance_mm))
    board.Add(zone)
    return zone


def test_kicads_export_of_two_overlapping_zones_through_pcbnew(tmp_path):
    """KiCad 10's own geometry and DSN export: the lower plane leaves out the
    higher zone grown by the clearance, with round corners; a board whose
    zones are apart exports byte-identically."""
    pcbnew = pytest.importorskip("pcbnew")
    board = pcbnew.BOARD()
    board.SetCopperLayerCount(4)
    nets = {}
    for name in ("LOW", "HIGH"):
        nets[name] = pcbnew.NETINFO_ITEM(board, name)
        board.Add(nets[name])
    _kicad_zone(pcbnew, board, nets["LOW"], (100, 100, 120, 110), 1)
    high = _kicad_zone(pcbnew, board, nets["HIGH"], (115, 103, 125, 107), 41, 0.5)
    dsn = tmp_path / "board.dsn"
    assert pcbnew.ExportSpecctraDSN(board, str(dsn))
    exported = dsn.read_text(encoding="utf-8")
    lines = export_flow.fix_plane_priorities(board, pcbnew, dsn)
    assert len(lines) == 1 and lines[0].startswith("plane LOW on "), lines
    regions = plane_regions(dsn.read_text(encoding="utf-8"))
    assert covered(regions["LOW"], 105, 105) and covered(regions["LOW"], 117, 101)
    assert not covered(regions["LOW"], 117, 105) and not covered(regions["LOW"], 114.6, 105)
    # a corner of the grown zone is round: 0.5 mm out on the diagonal is still in the lower plane
    assert covered(regions["LOW"], 115 - 0.45, 103 - 0.45)
    assert regions["HIGH"] == plane_regions(exported)["HIGH"]
    # apart: nothing changes
    high.Move(pcbnew.VECTOR2I(pcbnew.FromMM(20), 0))
    assert pcbnew.ExportSpecctraDSN(board, str(dsn))
    exported = dsn.read_bytes()
    assert export_flow.fix_plane_priorities(board, pcbnew, dsn) == []
    assert dsn.read_bytes() == exported
