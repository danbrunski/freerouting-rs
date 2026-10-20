"""Q22.4: copper graphics and copper text become obstacles (copper_graphics.py),
collected through pcbnew by kicad_board.copper_graphics and applied by
export_flow.fix_copper_graphics.

The fixture is crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb:
J1's two pads numbered 2 (net N2) at (12, 10) and (18, 10) with a 0.5 mm B.Cu
`fp_line` of no net between them, a `gr_text "GND"` on F.Cu over the same
gap, and a filled F.Cu `gr_poly` of net FG under P1. tests/data has KiCad
10.0.7's DSN export of it, which carries none of the three. The line and the
text become keepouts; the FG polygon, a shape of a net the DSN lists, is left
out and counted in the log (decided 2026-10-20, the Q22.4 remainder)."""

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


def test_a_shape_of_a_net_is_left_out_and_logged():
    text, log = copper_graphics.apply(DSN, facts([FG_POLY]))
    assert text == DSN
    assert log == ["copper graphics: 1 shape(s) of a net left out (Q22.4 remainder)"]


def test_a_net_line_is_left_out_beside_the_keepouts():
    text, log = copper_graphics.apply(DSN, facts([dict(LINE, net="N2"), LINE]))
    assert len(keepouts(text, copper_graphics.KEEPOUT_NAME)) == 1
    assert wires(text) == wires(DSN)
    assert log == [
        "copper graphics: 1 shape(s) and 0 text(s) on copper kept clear (1 keepouts)",
        "copper graphics: 1 shape(s) of a net left out (Q22.4 remainder)",
    ]


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
    """With KiCad's pcbnew: the B.Cu line and the FG polygon come out as above
    (the polygon left out), and the text's glyph outlines lie inside the box the `.kicad_pcb` reader
    keeps out (`crates/freerouting-kicad/tests/copper_graphics.rs`:
    12.0375..17.9625 x 8.5..11.5 mm)."""
    pcbnew = pytest.importorskip("pcbnew")
    board = pcbnew.LoadBoard(str(PCB))
    got = kicad_board.copper_graphics(board, pcbnew)
    assert [s for s in got["shapes"] if s["layer"] == "B.Cu"] == [LINE]
    assert [s for s in got["shapes"] if s["net"] == "FG"] == [FG_POLY]
    (text,) = got["texts"]
    assert text["layer"] == "F.Cu" and len(text["polygons"]) == 3
    xs = [x for poly in text["polygons"] for x, _ in poly]
    ys = [y for poly in text["polygons"] for _, y in poly]
    assert 12.0375 <= min(xs) and max(xs) <= 17.9625, (min(xs), max(xs))
    assert 8.5 <= min(ys) and max(ys) <= 11.5, (min(ys), max(ys))
    out, log = copper_graphics.apply(DSN, got)
    assert len(keepouts(out, "copper_graphic")) == 1 and len(keepouts(out, "copper_text")) == 3
    assert wires(out) == wires(DSN)
    assert log[-1] == "copper graphics: 1 shape(s) of a net left out (Q22.4 remainder)"
