"""Q22.4: copper graphics and copper text become obstacles (copper_graphics.py),
collected through pcbnew by kicad_board.copper_graphics and applied by
export_flow.fix_copper_graphics.

The fixture is crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb:
J1's two pads numbered 2 (net N2) at (12, 10) and (18, 10) with a 0.5 mm B.Cu
`fp_line` of no net between them, a `gr_text "GND"` on F.Cu over the same
gap, a filled F.Cu `gr_poly` of net FG, (3, 3)-(7, 5), under FG's pads P1 at
(4, 4) and P2 at (6, 4), and a 1 mm F.Cu `gr_line` of net FG from (20, 14) to
(28, 14) across the straight path between N3's pads J2-1 at (24, 11.5) and
J2-2 at (24, 16.5). tests/data has KiCad 10.0.7's DSN export of it, which
carries none of the four shapes. The line and the text become keepouts; the
two FG shapes, of a net the DSN lists, become conduction-area wires of FG with
`(attr obstacle)` (decided 2026-10-20, option (a))."""

from types import SimpleNamespace

import pytest

import copper_graphics
import edge_graphics
import export_flow
import kicad_board
from conftest import REPO, TESTS

DSN = (TESTS / "data" / "q22-copper-graphics.dsn").read_text(encoding="utf-8")
PCB = REPO / "crates" / "freerouting" / "tests" / "data" / "kicad" / "q22-copper-graphics.kicad_pcb"

LINE = {
    "layer": "B.Cu",
    "width_mm": 0.5,
    "kind": "polyline",
    "points": [(13.2, 10.0), (16.8, 10.0)],
    "filled": False,
    "net": "",
}
FG_POLY = {
    "layer": "F.Cu",
    "width_mm": 0.0,
    "kind": "ring",
    "points": [(3.0, 3.0), (7.0, 3.0), (7.0, 5.0), (3.0, 5.0)],
    "filled": True,
    "net": "FG",
}
FG_LINE = {
    "layer": "F.Cu",
    "width_mm": 1.0,
    "kind": "polyline",
    "points": [(20.0, 14.0), (28.0, 14.0)],
    "filled": False,
    "net": "FG",
}
# two glyph outlines, KiCad's frame (mm)
GLYPHS = [[(13.0, 9.2), (14.2, 9.2), (14.2, 10.8), (13.0, 10.8)], [(14.5, 9.2), (15.5, 9.2), (15.0, 10.8)]]


def facts(shapes=(), texts=(), ties=0):
    return {"shapes": list(shapes), "texts": list(texts), "net_tie_shapes": ties}


def keepouts(text, name):
    return [line.strip() for line in text.splitlines() if f'(keepout "{name}"' in line]


def corners(line):
    """The layer and polygon corners of a keepout line, in KiCad's frame (mm)."""
    words = line.split("(polygon ")[1].rstrip(")").split()
    nums = [float(v) for v in words[2:]]
    return words[0], [(x / 1000.0, -y / 1000.0) for x, y in zip(nums[::2], nums[1::2])]


def wires(text):
    """Every `(wire ...)` line."""
    return [line.strip() for line in text.splitlines() if line.strip().startswith("(wire ")]


def area_wires(text):
    """The layer, net and corners (KiCad's frame, mm) of every conduction-area
    wire with `(attr obstacle)`."""
    out = []
    for line in wires(text):
        if "(attr obstacle)" not in line:
            continue
        assert line.startswith("(wire (polygon ") and line.endswith(" (type fix) (attr obstacle))"), line
        poly, rest = line[len("(wire (polygon ") :].split(") (net ", 1)
        words = poly.split()
        nums = [float(v) for v in words[2:]]
        out.append((words[0], rest.split(")")[0], [(x / 1000.0, -y / 1000.0) for x, y in zip(nums[::2], nums[1::2])]))
    return out


def wiring_body(text):
    """The text between `(wiring` and the end of the file."""
    return text[text.index("(wiring") :]


def test_a_footprint_line_on_copper_becomes_a_keepout_on_its_layer():
    text, log = copper_graphics.apply(DSN, facts([LINE]))
    lines = keepouts(text, copper_graphics.KEEPOUT_NAME)
    assert len(lines) == 1
    layer, poly = corners(lines[0])
    assert layer == "B.Cu"
    # the line's copper: 0.25 mm on each side and past each end
    assert poly == [(12.95, 10.25), (17.05, 10.25), (17.05, 9.75), (12.95, 9.75), (12.95, 10.25)]
    assert log == ["copper graphics: 1 shape(s) and 0 text(s) on copper kept clear (1 keepouts)"]
    # before the structure's (via ...), and nothing else changes
    assert text.index('(keepout "copper_graphic"') < text.index('(via "Via[0-1]_600:300_um")')


def test_the_rest_of_the_dsn_stays_byte_for_byte():
    text, _ = copper_graphics.apply(DSN, facts([LINE]))
    removed = "\n".join(line for line in text.split("\n") if '(keepout "copper_graphic"' not in line)
    assert removed == DSN


def test_a_copper_text_becomes_keepouts_of_its_glyph_outlines():
    text, log = copper_graphics.apply(DSN, facts(texts=[{"layer": "F.Cu", "polygons": GLYPHS}]))
    lines = keepouts(text, copper_graphics.TEXT_KEEPOUT_NAME)
    assert [corners(line) for line in lines] == [("F.Cu", g + g[:1]) for g in GLYPHS]
    assert log == ["copper graphics: 0 shape(s) and 1 text(s) on copper kept clear (2 keepouts)"]


def test_a_filled_shape_of_a_net_becomes_a_conduction_area_of_its_net():
    text, log = copper_graphics.apply(DSN, facts([FG_POLY]))
    assert area_wires(text) == [("F.Cu", "FG", [(3.0, 3.0), (7.0, 3.0), (7.0, 5.0), (3.0, 5.0), (3.0, 3.0)])]
    assert log == ["copper graphics: 1 shape(s) of a net as fixed copper of their net (1 areas)"]
    # inside the wiring, and nothing else changes
    assert "(attr obstacle)" in wiring_body(text)
    removed = "\n".join(line for line in text.split("\n") if "(attr obstacle)" not in line)
    assert removed == DSN
    assert keepouts(text, copper_graphics.KEEPOUT_NAME) == []


def test_a_stroked_shape_of_a_net_becomes_the_area_of_its_stroke():
    text, log = copper_graphics.apply(DSN, facts([FG_LINE]))
    # 0.5 mm on each side and past each end
    assert area_wires(text) == [
        ("F.Cu", "FG", [(19.5, 14.5), (28.5, 14.5), (28.5, 13.5), (19.5, 13.5), (19.5, 14.5)])
    ]
    assert log == ["copper graphics: 1 shape(s) of a net as fixed copper of their net (1 areas)"]


def test_a_circle_of_a_net_becomes_a_polygon_area():
    circle = {"layer": "B.Cu", "width_mm": 0.0, "kind": "circle", "points": [(10.0, 10.0), (11.0, 10.0)]}
    text, _ = copper_graphics.apply(DSN, facts([dict(circle, filled=True, net="FG")]))
    ((layer, net, poly),) = area_wires(text)
    assert (layer, net) == ("B.Cu", "FG")
    body, _ = edge_graphics.shape_polygons(dict(circle, filled=True))
    assert len(poly) == len(body) + 1 and len(body) >= 8
    # the polygon holds the circle: every corner at least the radius out
    assert min(((x - 10.0) ** 2 + (y - 10.0) ** 2) ** 0.5 for x, y in poly) >= 1.0 - 1e-3


def test_a_net_line_becomes_an_area_beside_the_keepouts():
    text, log = copper_graphics.apply(DSN, facts([dict(LINE, net="N2"), LINE]))
    (keepout,) = keepouts(text, copper_graphics.KEEPOUT_NAME)
    ((layer, net, poly),) = area_wires(text)
    # the same polygon as the keepout of the same line with no net
    assert (layer, net) == ("B.Cu", "N2") and (layer, poly) == corners(keepout)
    assert log == [
        "copper graphics: 1 shape(s) and 0 text(s) on copper kept clear (1 keepouts)",
        "copper graphics: 1 shape(s) of a net as fixed copper of their net (1 areas)",
    ]
    assert text.index('(keepout "copper_graphic"') < text.index("(wiring") < text.index("(attr obstacle)")


def test_net_and_layer_names_keep_the_dsn_s_quoting():
    quoted = DSN.replace("(net FG", '(net "F G"').replace(" FG N2 N3", ' "F G" N2 N3')
    quoted = quoted.replace("F.Cu", '"Top Layer, Top"')
    text, _ = copper_graphics.apply(quoted, facts([dict(FG_POLY, net="F G", layer="Top Layer, Top")]))
    (line,) = [w for w in wires(text) if "(attr obstacle)" in w]
    assert line.startswith('(wire (polygon "Top Layer, Top" 0 3000 -3000 ') and '(net "F G")' in line


def test_a_wiring_on_one_line_or_none_takes_the_areas():
    one_line = DSN.replace("  (wiring\n  )\n", "  (wiring)\n")
    assert one_line != DSN
    text, _ = copper_graphics.apply(one_line, facts([FG_POLY]))
    assert len(area_wires(text)) == 1 and text.index("(wiring") < text.index("(attr obstacle)")
    none = DSN.replace("  (wiring\n  )\n", "")
    text, _ = copper_graphics.apply(none, facts([FG_POLY]))
    assert len(area_wires(text)) == 1 and text.index("(wiring") < text.index("(attr obstacle)")
    assert text.rstrip().endswith(")")


def test_a_net_the_dsn_does_not_list_is_kept_out_like_no_net():
    text, _ = copper_graphics.apply(DSN, facts([dict(LINE, net="NOT_IN_DSN")]))
    assert len(keepouts(text, copper_graphics.KEEPOUT_NAME)) == 1
    assert wires(text) == wires(DSN)


def test_shapes_off_the_dsn_layers_and_nothing_on_copper_change_nothing():
    assert copper_graphics.apply(DSN, facts()) == (DSN, [])
    assert copper_graphics.apply(DSN, facts([dict(LINE, layer="In1.Cu")])) == (DSN, [])


def test_net_tie_shapes_are_logged():
    _text, log = copper_graphics.apply(DSN, facts([LINE], ties=1))
    assert log[-1] == "copper graphics: 1 net-tie shape(s) kept clear as keepouts, their nets left out"


# --- the pcbnew side ----------------------------------------------------------


class Pt:
    def __init__(self, x, y):
        self.x, self.y = x, y


def nm(x, y):
    return Pt(round(x * 1e6), round(y * 1e6))


class Chain:
    def __init__(self, pts):
        self.pts = pts

    def PointCount(self):
        return len(self.pts)

    def CPoint(self, k):
        return self.pts[k]


class PolySet:
    def __init__(self):
        self.rings = []

    def OutlineCount(self):
        return len(self.rings)

    def Outline(self, i):
        return self.rings[i]

    def Simplify(self):
        pass


class Segment:
    def __init__(self, layer, net, start, end, width=500000):
        self.layer, self.net, self.start, self.end, self.width = layer, net, start, end, width

    def GetLayer(self):
        return self.layer

    def GetShape(self):
        return 0

    def GetWidth(self):
        return self.width

    def GetStart(self):
        return self.start

    def GetEnd(self):
        return self.end

    def GetNetname(self):
        return self.net

    def IsFilled(self):
        return False


class Text:
    def __init__(self, layer, glyphs, visible=True):
        self.layer, self.glyphs, self.visible = layer, glyphs, visible

    def GetLayer(self):
        return self.layer

    def GetText(self):
        return "GND"

    def IsVisible(self):
        return self.visible

    def TransformTextToPolySet(self, sps, clearance, max_error, loc):
        sps.rings = [Chain([nm(*p) for p in g]) for g in self.glyphs]


F_CU, B_CU, F_SILKS = 0, 2, 5
FAKE_PCBNEW = SimpleNamespace(
    SHAPE_T_SEGMENT=0,
    SHAPE_T_RECTANGLE=1,
    SHAPE_T_ARC=2,
    SHAPE_T_CIRCLE=3,
    SHAPE_T_POLY=4,
    SHAPE_T_BEZIER=5,
    ERROR_OUTSIDE=1,
    ToMM=lambda v: v / 1e6,
    SHAPE_POLY_SET=PolySet,
    IsCopperLayer=lambda layer: layer in (F_CU, B_CU),
)
NAMES = {F_CU: "F.Cu", B_CU: "B.Cu", F_SILKS: "F.SilkS"}


class Footprint:
    def __init__(self, items, tie=False):
        self.items, self.tie = items, tie

    def GraphicalItems(self):
        return self.items

    def IsNetTie(self):
        return self.tie


class Board:
    def __init__(self, drawings, footprints=()):
        self.drawings, self.footprints = drawings, list(footprints)

    def GetDrawings(self):
        return self.drawings

    def GetFootprints(self):
        return self.footprints

    def GetDesignSettings(self):
        return SimpleNamespace(m_MaxError=5000)

    def GetLayerName(self, layer):
        return NAMES[layer]


def test_kicad_board_collects_copper_shapes_and_visible_texts():
    board = Board(
        [
            Text(F_CU, GLYPHS),
            Text(F_CU, GLYPHS, visible=False),
            Segment(F_SILKS, "", nm(0, 0), nm(1, 0)),
            Segment(F_CU, "FG", nm(1, 1), nm(2, 1)),
        ],
        [
            Footprint([Segment(B_CU, "", nm(13.2, 10), nm(16.8, 10))]),
            Footprint([Segment(F_CU, "N2", nm(5, 5), nm(6, 5))], tie=True),
        ],
    )
    got = kicad_board.copper_graphics(board, FAKE_PCBNEW)
    assert got["texts"] == [{"layer": "F.Cu", "polygons": GLYPHS}]
    assert [(s["layer"], s["net"], s["points"]) for s in got["shapes"]] == [
        ("F.Cu", "FG", [(1, 1), (2, 1)]),
        ("B.Cu", "", [(13.2, 10), (16.8, 10)]),
        ("F.Cu", "", [(5, 5), (6, 5)]),  # a net-tie footprint keeps its ties: no net
    ]
    assert got["shapes"][1] == LINE
    assert got["net_tie_shapes"] == 1


def test_fix_copper_graphics_writes_the_obstacles(tmp_path):
    board = Board([Text(F_CU, GLYPHS)], [Footprint([Segment(B_CU, "", nm(13.2, 10), nm(16.8, 10))])])
    dsn = tmp_path / "b.dsn"
    dsn.write_text(DSN, encoding="utf-8")
    lines = export_flow.fix_copper_graphics(board, FAKE_PCBNEW, dsn)
    assert lines == ["copper graphics: 1 shape(s) and 1 text(s) on copper kept clear (3 keepouts)"]
    text = dsn.read_text(encoding="utf-8")
    assert len(keepouts(text, "copper_graphic")) == 1 and len(keepouts(text, "copper_text")) == 2


def test_fix_copper_graphics_never_blocks_routing(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(DSN, encoding="utf-8")
    lines = export_flow.fix_copper_graphics(object(), FAKE_PCBNEW, dsn)
    assert len(lines) == 1 and lines[0].startswith("WARNING: copper-graphic obstacles skipped: AttributeError")
    assert dsn.read_text(encoding="utf-8") == DSN


# --- the fixture, through KiCad's own pcbnew (skipped without it) --------------


def test_the_fixture_through_pcbnew_gives_the_reader_s_obstacles():
    """With KiCad's pcbnew: the B.Cu line and the two FG shapes come out as
    above (the FG shapes as areas of FG), and the text's glyph outlines lie
    inside the box the `.kicad_pcb` reader keeps out
    (`crates/freerouting-kicad/tests/copper_graphics.rs`: 12.0375..17.9625 x
    8.5..11.5 mm)."""
    pcbnew = pytest.importorskip("pcbnew")
    board = pcbnew.LoadBoard(str(PCB))
    got = kicad_board.copper_graphics(board, pcbnew)
    assert [s for s in got["shapes"] if s["layer"] == "B.Cu"] == [LINE]
    assert [s for s in got["shapes"] if s["net"] == "FG"] == [FG_POLY, FG_LINE]
    (text,) = got["texts"]
    assert text["layer"] == "F.Cu" and len(text["polygons"]) == 3
    xs = [x for poly in text["polygons"] for x, _ in poly]
    ys = [y for poly in text["polygons"] for _, y in poly]
    assert 12.0375 <= min(xs) and max(xs) <= 17.9625, (min(xs), max(xs))
    assert 8.5 <= min(ys) and max(ys) <= 11.5, (min(ys), max(ys))
    out, log = copper_graphics.apply(DSN, got)
    assert len(keepouts(out, "copper_graphic")) == 1 and len(keepouts(out, "copper_text")) == 3
    # the reader's areas (crates/freerouting-kicad/tests/copper_graphics.rs)
    assert [(layer, net, poly) for layer, net, poly in area_wires(out)] == [
        ("F.Cu", "FG", [(3.0, 3.0), (7.0, 3.0), (7.0, 5.0), (3.0, 5.0), (3.0, 3.0)]),
        ("F.Cu", "FG", [(19.5, 14.5), (28.5, 14.5), (28.5, 13.5), (19.5, 13.5), (19.5, 14.5)]),
    ]
    assert log[-1] == "copper graphics: 2 shape(s) of a net as fixed copper of their net (2 areas)"
