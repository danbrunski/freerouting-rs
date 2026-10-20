"""Q18.5: the Edge.Cuts and Margin shapes KiCad's DSN export leaves out become
copper keepouts (edge_graphics.py), collected through pcbnew by
kicad_board.edge_graphics and applied by export_flow.fix_edge_graphics."""

from types import SimpleNamespace

import edge_graphics
import export_flow
import kicad_board
from conftest import TESTS

TWO = (TESTS / "data" / "k09_two_layer.dsn").read_text(encoding="utf-8")
# the outline of k09_two_layer.dsn, KiCad's frame (mm, Y down)
RING = [(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (100.0, 90.0)]


def facts(*shapes, outline=(RING,), edge=0.5):
    return {"edge_clearance_mm": edge, "outline_mm": [list(r) for r in outline], "shapes": list(shapes)}


def shape(kind, points, layer="Margin", width=0.1):
    return {"layer": layer, "width_mm": width, "kind": kind, "points": points}


def keepouts(text):
    return [line.strip() for line in text.splitlines() if '(keepout "edge_clearance"' in line]


def corners(line):
    """The polygon corners of a keepout line, in KiCad's frame (mm)."""
    nums = [float(v) for v in line.split("(polygon ")[1].rstrip(")").split()[2:]]
    return [(x / 1000.0, -y / 1000.0) for x, y in zip(nums[::2], nums[1::2])]


def inside(poly, p):
    """Point in convex polygon (either orientation)."""
    signs = set()
    for a, b in zip(poly, poly[1:] + poly[:1]):
        cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        if abs(cross) > 1e-9:
            signs.add(cross > 0)
    return len(signs) <= 1


def test_a_margin_line_becomes_a_keepout_on_every_copper_layer():
    # slice-dcmt-bread-slice's kind of shape: a Margin line inside the board
    out, log = edge_graphics.apply(TWO, facts(shape("polyline", [(120.0, 60.0), (120.0, 80.0)])))
    ks = keepouts(out)
    assert len(ks) == 2 and "F.Cu" in ks[0] and "B.Cu" in ks[1]
    # w = 0.5 edge + 0.05 half stroke - 0.2 DSN default clearance = 0.35 mm
    assert ks[0] == (
        '(keepout "edge_clearance" (polygon F.Cu 0 119650 -59650 119650 -80350 120350 -80350 120350 -59650 '
        "119650 -59650))"
    )
    assert log == ["edge graphics: 1 Edge.Cuts/Margin shape(s) off the board outline kept 0.5 mm clear (2 keepouts)"]
    # inserted in the structure, before its (via ...), the rest byte for byte
    at = out.index('    (keepout "edge_clearance"')
    assert out.index("(via ") > at > out.index("(boundary")


def test_the_rest_of_the_dsn_stays_byte_for_byte():
    out, _ = edge_graphics.apply(TWO, facts(shape("polyline", [(120.0, 60.0), (120.0, 80.0)])))
    lines = [line for line in out.split("\n") if '(keepout "edge_clearance"' not in line]
    assert "\n".join(lines) == TWO


def test_edge_cuts_on_the_outline_add_nothing():
    on = [shape("polyline", [RING[i], RING[(i + 1) % 4]], layer="Edge.Cuts", width=0.05) for i in range(4)]
    out, log = edge_graphics.apply(TWO, facts(*on))
    assert out == TWO and log == []


def test_an_edge_cuts_line_off_the_outline_is_kept_clear_from_its_centreline():
    # an inner slot line that closes nothing: Edge.Cuts counts no stroke (KiCad's DRC)
    out, _ = edge_graphics.apply(
        TWO, facts(shape("polyline", [(130.0, 70.0), (140.0, 70.0)], layer="Edge.Cuts", width=0.4))
    )
    ks = keepouts(out)
    assert len(ks) == 2
    # w = 0.5 - 0.2 = 0.3 mm, the stroke width does not count
    assert ks[0].startswith('(keepout "edge_clearance" (polygon F.Cu 0 129700 -70300 140300 -70300 ')


def test_an_arc_is_covered_by_its_chords():
    arc = shape("arc", [(127.6, 129.15), (131.595154, 130.804846), (133.25, 134.8)], width=0.1)
    out, _ = edge_graphics.apply(TWO, facts(arc, edge=0.025))
    ks = [corners(k) for k in keepouts(out) if "F.Cu" in k]
    assert len(ks) > 4
    # every point of the arc (centre (127.6, 134.8), r 5.65 mm) is inside some keepout
    import math

    for i in range(101):
        a = -math.pi / 2 + (math.pi / 2) * i / 100
        p = (127.6 + 5.65 * math.cos(a), 134.8 + 5.65 * math.sin(a))
        assert any(inside(k, p) for k in ks), p


def test_a_circle_and_a_closed_ring_are_closed_chains():
    circle = shape("circle", [(120.0, 70.0), (121.0, 70.0)], layer="Edge.Cuts")
    ring = shape("ring", [(130.0, 60.0), (132.0, 60.0), (132.0, 62.0), (130.0, 62.0)], layer="Margin")
    out, log = edge_graphics.apply(TWO, facts(circle, ring))
    ks = keepouts(out)
    assert len([k for k in ks if "F.Cu" in k]) > 4 + 4  # the circle's chords plus the ring's 4 sides
    assert log[0].startswith("edge graphics: 2 Edge.Cuts/Margin shape(s)")


def test_a_malformed_outline_exports_the_bounding_box_so_every_line_inside_is_kept():
    # d3c-0496's case: Edge.Cuts does not close, the DSN boundary is the bounding box
    lines = [shape("polyline", [(110.0, 60.0), (140.0, 60.0)], layer="Edge.Cuts", width=0.15)]
    out, _ = edge_graphics.apply(TWO, facts(*lines))
    assert len(keepouts(out)) == 2


def test_hole_contours_get_the_edge_clearance_too():
    """The export makes outline holes keepouts the router keeps only its default
    clearance from; their Edge.Cuts lines are not on an outer contour."""
    hole = shape("ring", [(120.0, 60.0), (125.0, 60.0), (125.0, 65.0), (120.0, 65.0)], layer="Edge.Cuts")
    out, _ = edge_graphics.apply(TWO, facts(hole))
    assert len(keepouts(out)) == 8


def test_the_minimum_half_width_when_the_edge_clearance_is_within_the_default():
    out, _ = edge_graphics.apply(TWO, facts(shape("polyline", [(120.0, 60.0), (120.0, 80.0)], width=0.0), edge=0.1))
    assert keepouts(out)[0].startswith('(keepout "edge_clearance" (polygon F.Cu 0 119999 -59999 119999 -80001 ')


def test_nothing_to_do_changes_nothing():
    assert edge_graphics.apply(TWO, facts()) == (TWO, [])


def test_crlf_files_get_crlf_insertions():
    crlf = TWO.replace("\n", "\r\n")
    out, _ = edge_graphics.apply(crlf, facts(shape("polyline", [(120.0, 60.0), (120.0, 80.0)])))
    assert "\r\n" in out and "\n" not in out.replace("\r\n", "")
    assert out.replace("\r\n", "\n") == edge_graphics.apply(TWO, facts(shape("polyline", [(120.0, 60.0), (120.0, 80.0)])))[0]


# --- the pcbnew side ----------------------------------------------------------


class Pt:
    def __init__(self, x, y):
        self.x, self.y = x, y


def nm(x, y):
    return Pt(round(x * 1e6), round(y * 1e6))


class Shape:
    def __init__(self, kind, layer, width, **geo):
        self.kind, self.layer, self.width, self.geo = kind, layer, width, geo

    def GetLayer(self):
        return self.layer

    def GetShape(self):
        return self.kind

    def GetWidth(self):
        return self.width

    def GetStart(self):
        return self.geo["start"]

    def GetEnd(self):
        return self.geo["end"]

    def GetArcMid(self):
        return self.geo["mid"]

    def GetCenter(self):
        return self.geo["start"]

    def GetRadius(self):
        return self.geo["radius"]

    def GetRectCorners(self):
        return self.geo["corners"]

    def GetPolyShape(self):
        return self.geo["poly"]

    def GetBezierPoints(self):
        return self.geo["bezier"]

    def IsFilled(self):
        return self.geo.get("filled", False)


class Chain:
    def __init__(self, pts):
        self.pts = pts

    def PointCount(self):
        return len(self.pts)

    def CPoint(self, k):
        return self.pts[k]


class PolySet:
    def __init__(self, rings=()):
        self.rings = [Chain(r) for r in rings]

    def OutlineCount(self):
        return len(self.rings)

    def Outline(self, i):
        return self.rings[i]

    def HoleCount(self, i):
        return 0


FAKE_PCBNEW = SimpleNamespace(
    Edge_Cuts=25,
    Margin=31,
    F_SilkS=5,
    F_Mask=39,
    B_Mask=38,
    SHAPE_T_SEGMENT=0,
    SHAPE_T_RECTANGLE=1,
    SHAPE_T_ARC=2,
    SHAPE_T_CIRCLE=3,
    SHAPE_T_POLY=4,
    SHAPE_T_BEZIER=5,
    ToMM=lambda v: v / 1e6,
    SHAPE_POLY_SET=PolySet,
)


class Footprint:
    def __init__(self, items):
        self.items = items

    def GraphicalItems(self):
        return self.items


class Board:
    def __init__(self, drawings, footprints=(), outline=(), edge_nm=500000):
        self.drawings, self.footprints, self.outline, self.edge_nm = drawings, list(footprints), outline, edge_nm

    def GetDrawings(self):
        return self.drawings

    def GetFootprints(self):
        return self.footprints

    def GetDesignSettings(self):
        return SimpleNamespace(m_CopperEdgeClearance=self.edge_nm)

    def GetBoardPolygonOutlines(self, outline, *args):
        outline.rings = [Chain([nm(*p) for p in r]) for r in self.outline]
        return True


def test_kicad_board_collects_edge_cuts_and_margin_shapes_of_the_board_and_its_footprints():
    p = FAKE_PCBNEW
    board = Board(
        [
            Shape(p.SHAPE_T_SEGMENT, p.Margin, 100000, start=nm(120, 60), end=nm(120, 80)),
            Shape(p.SHAPE_T_SEGMENT, p.F_SilkS, 100000, start=nm(1, 1), end=nm(2, 2)),
            Shape(p.SHAPE_T_ARC, p.Edge_Cuts, 50000, start=nm(1, 0), mid=nm(0.7071, 0.7071), end=nm(0, 1)),
            Shape(p.SHAPE_T_CIRCLE, p.Edge_Cuts, 50000, start=nm(5, 5), radius=1000000),
            Shape(p.SHAPE_T_RECTANGLE, p.Margin, 0, corners=[nm(0, 0), nm(1, 0), nm(1, 1), nm(0, 1)]),
            Shape(p.SHAPE_T_POLY, p.Margin, 0, poly=PolySet([[nm(0, 0), nm(2, 0), nm(2, 2)]])),
            Shape(p.SHAPE_T_BEZIER, p.Margin, 0, bezier=[nm(0, 0), nm(1, 1), nm(2, 0)]),
        ],
        [Footprint([Shape(p.SHAPE_T_SEGMENT, p.Edge_Cuts, 0, start=nm(130, 70), end=nm(140, 70))])],
        outline=[RING],
    )
    got = kicad_board.edge_graphics(board, p)
    assert got["edge_clearance_mm"] == 0.5
    assert got["outline_mm"] == [RING]
    kinds = [(s["layer"], s["kind"]) for s in got["shapes"]]
    assert kinds == [
        ("Margin", "polyline"),
        ("Edge.Cuts", "arc"),
        ("Edge.Cuts", "circle"),
        ("Margin", "ring"),
        ("Margin", "ring"),
        ("Margin", "polyline"),
        ("Edge.Cuts", "polyline"),
    ]
    assert got["shapes"][0] == {
        "layer": "Margin",
        "width_mm": 0.1,
        "kind": "polyline",
        "points": [(120, 60), (120, 80)],
        "filled": False,
    }
    assert got["shapes"][2]["points"] == [(5, 5), (6, 5)]


def test_fix_edge_graphics_writes_the_keepouts(tmp_path):
    p = FAKE_PCBNEW
    board = Board([Shape(p.SHAPE_T_SEGMENT, p.Margin, 100000, start=nm(120, 60), end=nm(120, 80))], outline=[RING])
    dsn = tmp_path / "b.dsn"
    dsn.write_text(TWO, encoding="utf-8")
    lines = export_flow.fix_edge_graphics(board, p, dsn)
    assert lines == ["edge graphics: 1 Edge.Cuts/Margin shape(s) off the board outline kept 0.5 mm clear (2 keepouts)"]
    assert len(keepouts(dsn.read_text(encoding="utf-8"))) == 2


def test_fix_edge_graphics_never_blocks_routing(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(TWO, encoding="utf-8")
    lines = export_flow.fix_edge_graphics(object(), FAKE_PCBNEW, dsn)
    assert len(lines) == 1 and lines[0].startswith("WARNING: edge-graphic keepouts skipped: AttributeError")
    assert dsn.read_text(encoding="utf-8") == TWO


# Q18.7: the board's own F.Mask/B.Mask shapes (exposed copper no pad owns)

# slice-dcmt-bread-slice's first aperture, a filled gr_poly on F.Mask
APERTURE = [(130.0, 60.0), (140.0, 60.0), (140.0, 70.0), (130.0, 70.0)]


def mask_keepouts(text):
    return [line.strip() for line in text.splitlines() if '(keepout "solder_mask"' in line]


def mask_shape(kind, points, layer="F.Mask", width=0.0, filled=True):
    return {"layer": layer, "width_mm": width, "kind": kind, "points": points, "filled": filled}


def test_a_filled_front_mask_polygon_keeps_the_top_copper_out_of_it():
    out, log = edge_graphics.apply(TWO, facts(mask_shape("ring", APERTURE)))
    assert keepouts(out) == []
    ks = mask_keepouts(out)
    assert ks == [
        '(keepout "solder_mask" (polygon F.Cu 0 130000 -60000 140000 -60000 140000 -70000 130000 -70000 '
        "130000 -60000))"
    ]
    assert log == ["solder-mask graphics: 1 board F.Mask/B.Mask shape(s) kept clear on their copper side (1 keepouts)"]
    assert out.index("(via ") > out.index('(keepout "solder_mask"') > out.index("(boundary")


def test_a_back_mask_shape_goes_on_the_bottom_copper():
    out, _ = edge_graphics.apply(TWO, facts(mask_shape("ring", APERTURE, layer="B.Mask")))
    ks = mask_keepouts(out)
    assert len(ks) == 1 and "(polygon B.Cu 0 " in ks[0]


def test_an_unfilled_mask_shape_keeps_its_stroke_and_a_filled_one_its_stroke_too():
    line = mask_shape("polyline", [(120.0, 60.0), (120.0, 80.0)], width=0.4, filled=False)
    out, _ = edge_graphics.apply(TWO, facts(line))
    (k,) = mask_keepouts(out)
    assert corners(k)[:4] == [(119.8, 59.8), (119.8, 80.2), (120.2, 80.2), (120.2, 59.8)]
    stroked = mask_shape("ring", APERTURE, width=0.2)
    out, _ = edge_graphics.apply(TWO, facts(stroked))
    assert len(mask_keepouts(out)) == 1 + 4  # the body and its four sides


def test_a_filled_mask_circle_is_covered_by_its_polygon():
    out, _ = edge_graphics.apply(TWO, facts(mask_shape("circle", [(135.0, 65.0), (137.0, 65.0)])))
    (k,) = mask_keepouts(out)
    poly = corners(k)[:-1]
    import math

    for i in range(360):
        a = math.radians(i)
        assert inside(poly, (135.0 + 2.0 * math.cos(a), 65.0 + 2.0 * math.sin(a)))


def test_edge_and_mask_keepouts_together():
    margin = shape("polyline", [(120.0, 60.0), (120.0, 80.0)])
    out, log = edge_graphics.apply(TWO, facts(margin, mask_shape("ring", APERTURE)))
    assert len(keepouts(out)) == 2 and len(mask_keepouts(out)) == 1 and len(log) == 2
    # the edge keepouts never count the mask shape
    assert log[0].startswith("edge graphics: 1 Edge.Cuts/Margin shape(s)")


def test_kicad_board_collects_the_boards_mask_shapes_but_not_the_footprints():
    p = FAKE_PCBNEW
    ring = [nm(*q) for q in APERTURE]
    board = Board(
        [
            Shape(p.SHAPE_T_POLY, p.F_Mask, 0, poly=PolySet([ring]), filled=True),
            Shape(p.SHAPE_T_SEGMENT, p.B_Mask, 200000, start=nm(1, 1), end=nm(2, 2)),
        ],
        # a footprint's mask shapes belong to its pads' apertures (the pads' expansion)
        [Footprint([Shape(p.SHAPE_T_POLY, p.F_Mask, 0, poly=PolySet([ring]), filled=True)])],
        outline=[RING],
    )
    got = kicad_board.edge_graphics(board, p)
    assert [(s["layer"], s["kind"], s["filled"]) for s in got["shapes"]] == [
        ("F.Mask", "ring", True),
        ("B.Mask", "polyline", False),
    ]
    assert got["shapes"][0]["points"] == APERTURE


def test_kicad_10_says_any_fill_instead_of_filled():
    class Kicad10Shape(Shape):
        IsFilled = None  # removed in KiCad 10

        def IsAnyFill(self):
            return self.geo.get("filled", False)

    p = FAKE_PCBNEW
    ring = [nm(*q) for q in APERTURE]
    board = Board([Kicad10Shape(p.SHAPE_T_POLY, p.F_Mask, 0, poly=PolySet([ring]), filled=True)], outline=[RING])
    assert [s["filled"] for s in kicad_board.edge_graphics(board, p)["shapes"]] == [True]
