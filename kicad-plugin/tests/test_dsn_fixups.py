"""K09.1-K09.5: the DSN fix-ups of dsn_fixups.py (canary's dsn_export.py steps 2-6) on
small KiCad-shaped fixtures, and the board facts that drive their "auto" lists."""

import pytest

import dsn_fixups
import export_flow
import options
import rule_areas
from conftest import TESTS

FOUR = (TESTS / "data" / "k09_four_layer.dsn").read_text(encoding="utf-8")
TWO = (TESTS / "data" / "k09_two_layer.dsn").read_text(encoding="utf-8")


def zone(net, layer, priority=0, fill=None, outer=None):
    return {
        "net": net,
        "layer": layer,
        "outer": layer in ("F.Cu", "B.Cu") if outer is None else outer,
        "priority": priority,
        "filled": fill is not None,
        "fill": fill or [],
    }


RECT_5V = [[(100.0, 50.0), (110.0, 50.0), (110.0, 52.0), (100.0, 52.0)]]
BOARD = {
    "edge_clearance_mm": 0.5,
    "outline_bbox_mm": (100.0, 50.0, 150.0, 90.0),
    "pads": [
        {"ref": "FID1", "local_clearance_mm": 0.6, "size_mm": (1.0, 1.0), "at_mm": (120.0, 60.0), "layers": ["F.Cu"]},
        {"ref": "R1", "local_clearance_mm": 0.1, "size_mm": (1.0, 0.5), "at_mm": (130.0, 60.0), "layers": ["F.Cu"]},
    ],
    "zones": [
        zone("GND", "F.Cu", 0, [[(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (100.0, 90.0)]]),
        zone("+5V", "F.Cu", 2, RECT_5V),
        zone("GND", "B.Cu", 0, [[(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (100.0, 90.0)]]),
        zone("GND", "In1.Cu", 0, [[(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (100.0, 90.0)]]),
        zone("+5V", "In2.Cu", 1, [[(100.0, 50.0), (120.0, 50.0), (120.0, 70.0), (100.0, 70.0)]]),
        zone("GND", "In2.Cu", 0, [[(120.0, 50.0), (150.0, 50.0), (150.0, 90.0), (120.0, 90.0)]]),
    ],
    "smd_pad_min_mm": {"+5V": 0.25, "GND": 0.3, "SIG": 0.2},
    "min_track_mm": 0.15,
}


def outside(text, start, end):
    return text[:start], text[end:]


def test_one_class_width_is_rewritten_and_the_rest_stays_byte_for_byte():
    out, log = dsn_fixups.apply(FOUR, BOARD, {"class_widths": {"Power": 0.3}})
    i = FOUR.index("(width 1500)")
    assert out[:i] == FOUR[:i]
    assert out[i:].startswith("(width 300)")
    assert out[i + len("(width 300)") :] == FOUR[i + len("(width 1500)") :]
    assert log == ["class Power: width 1500 -> 300 um"]


def test_an_unknown_class_is_refused():
    with pytest.raises(ValueError, match="no class Nope"):
        dsn_fixups.apply(FOUR, BOARD, {"class_widths": {"Nope": 0.3}})


def test_drop_pour_nets_drops_exactly_the_outer_planes_of_the_listed_nets():
    out, _ = dsn_fixups.apply(FOUR, BOARD, {"drop_pour_nets": ["GND"]})
    planes = [(net, layer) for _node, net, layer in dsn_fixups.Dsn(out).planes()]
    assert planes == [("+5V", "F.Cu"), ("GND", "In1.Cu"), ("+5V", "In2.Cu"), ("GND", "In2.Cu")]
    # Only the two removed blocks differ: their text is gone, the lines stay (as dsn_export.py does).
    removed = [n for n, net, layer in dsn_fixups.Dsn(FOUR).planes() if net == "GND" and layer in ("F.Cu", "B.Cu")]
    expect = FOUR
    for node in sorted(removed, key=lambda n: n.start, reverse=True):
        expect = expect[: node.start] + expect[node.end :]
    assert out == expect


def test_fixed_pour_classes_cover_the_fill_with_fixed_wires_and_drop_its_plane():
    out, log = dsn_fixups.apply(FOUR, BOARD, {"fixed_pour_classes": ["Power"]})
    planes = [(net, layer) for _node, net, layer in dsn_fixups.Dsn(out).planes()]
    assert ("+5V", "F.Cu") not in planes and ("+5V", "In2.Cu") in planes
    # The 10 x 2 mm fill is one rectangle: a 2 mm path along its middle, shortened by half its width.
    wire = "    (wire (path F.Cu 2000 101000 -51000 109000 -51000) (net +5V) (type fix))\n"
    assert "(wiring\n" + wire + "\n    (wire (path F.Cu 200" in out
    assert log[0].startswith("pours: 1 fixed wires for 1 pour(s) of Power; 1 outer plane(s) removed")


def test_fill_rects_cuts_an_l_shape_into_merged_strips():
    l_shape = [[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]]
    rects = [tuple(round(v, 6) for v in r) for r in dsn_fixups.fill_rects(l_shape)]
    assert rects == [(0.0, 0.0, 1.0, 2.0), (1.0, 0.0, 2.0, 1.0)]
    # a hole splits the strips that cross it
    ring = [[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)], [(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]]
    rects = [tuple(round(v, 6) for v in r) for r in dsn_fixups.fill_rects(ring)]
    assert rects == [(0.0, 0.0, 1.0, 3.0), (1.0, 0.0, 2.0, 1.0), (1.0, 2.0, 2.0, 3.0), (2.0, 0.0, 3.0, 3.0)]


def test_edge_and_pad_keepouts_on_a_two_layer_board():
    out, log = dsn_fixups.apply(TWO, BOARD, {"edge_and_pad_keepouts": True})
    # 0.5 mm edge clearance, 0.2 mm default clearance: 0.3 mm strips, reaching 1 mm past the bbox.
    strips = [
        '    (keepout "" (rect F.Cu 99000 -50300 151000 -49000))',
        '    (keepout "" (rect F.Cu 99000 -91000 151000 -89700))',
        '    (keepout "" (rect F.Cu 99000 -91000 100300 -49000))',
        '    (keepout "" (rect F.Cu 149700 -91000 151000 -49000))',
    ]
    strips += [s.replace("F.Cu", "B.Cu") for s in strips]
    # FID1's own 0.6 mm clearance: a circle 1 + 2 x 0.4 mm across; R1 (0.1 mm) gets none.
    circle = '    (keepout "" (circle F.Cu 1800 120000 -60000))'
    block = "\n".join(strips + [circle])
    via = TWO.index("\n    (via ")
    assert out == TWO[:via] + "\n" + block + TWO[via:]
    assert log == ["keepouts: 8 edge strip(s) (edge clearance 0.5 mm), 1 pad circle(s)"]
    # inserted after the layer definitions, and the result still parses
    assert out.index("(layer B.Cu") < out.index("(keepout") < out.index("(via ")
    rule_areas.parse(out)


def test_no_edge_strips_when_the_edge_clearance_is_within_the_default():
    board = dict(BOARD, edge_clearance_mm=0.2, pads=[])
    out, log = dsn_fixups.apply(TWO, board, {"edge_and_pad_keepouts": True})
    assert out == TWO and log == []


def test_crlf_files_get_crlf_insertions():
    crlf = TWO.replace("\n", "\r\n")
    out, _ = dsn_fixups.apply(crlf, BOARD, {"edge_and_pad_keepouts": True})
    assert "\n" not in out.replace("\r\n", "")
    assert out.replace("\r\n", "\n") == dsn_fixups.apply(TWO, BOARD, {"edge_and_pad_keepouts": True})[0]


def test_route_on_plane_layers_opens_only_the_named_layer():
    out, log = dsn_fixups.apply(FOUR, BOARD, {"route_on_plane_layers": ["In2.Cu"]})
    layers = [(name, kind) for name, kind, _ in dsn_fixups.Dsn(out).layers]
    assert layers == [("F.Cu", "signal"), ("In1.Cu", "power"), ("In2.Cu", "signal"), ("B.Cu", "signal")]
    assert len(out) == len(FOUR) + 1  # "power" -> "signal"
    assert log == ["layer In2.Cu: power -> signal"]
    with pytest.raises(ValueError, match="no power layer F.Cu"):
        dsn_fixups.apply(FOUR, BOARD, {"route_on_plane_layers": ["F.Cu"]})


def test_nothing_asked_changes_nothing():
    assert dsn_fixups.apply(FOUR, BOARD, {}) == (FOUR, [])
    assert not dsn_fixups.wanted(options.dsn_settings(options.RouteOptions(edge_and_pad_keepouts=False)))


def test_auto_lists_come_from_the_board():
    dsn = dsn_fixups.Dsn(FOUR)
    # +5V's F.Cu pour has priority 2 over GND's flood fill (0): its class is fixed.
    assert dsn_fixups.auto_fixed_pour_classes(BOARD, dsn) == ["Power"]
    # GND is alone on In1.Cu (a full plane); In2.Cu is split between +5V and GND.
    assert dsn_fixups.auto_drop_pour_nets(BOARD) == ["GND"]
    # Power (1.5 mm) and GND (0.5 mm) are wider than the 0.2 mm default and have narrower SMD pads.
    assert dsn_fixups.auto_class_widths(BOARD, dsn) == {"GND": 0.3, "Power": 0.25}
    # never below the board minimum
    assert dsn_fixups.auto_class_widths(dict(BOARD, min_track_mm=0.28), dsn) == {"GND": 0.3, "Power": 0.28}
    settings = {"fixed_pour_classes": "auto", "drop_pour_nets": "auto", "class_widths": "auto"}
    resolved = dsn_fixups.resolve(settings, BOARD, dsn)
    assert resolved == {"fixed_pour_classes": ["Power"], "drop_pour_nets": ["GND"], "class_widths": {"GND": 0.3, "Power": 0.25}}


def test_auto_on_a_board_with_flood_fills_only_changes_nothing_but_the_widths():
    board = dict(BOARD, zones=[zone("GND", "F.Cu", 0, RECT_5V), zone("+5V", "F.Cu", 0, RECT_5V)])
    dsn = dsn_fixups.Dsn(TWO)
    assert dsn_fixups.auto_fixed_pour_classes(board, dsn) == []
    assert dsn_fixups.auto_drop_pour_nets(board) == []


def test_the_engine_mode_passes_the_classes_to_the_router():
    assert dsn_fixups.engine_plane_obstacle_args(["Power_High", "Switch_Node"]) == [
        "--router.plane_obstacle_net_classes=Power_High,Switch_Node"
    ]
    assert dsn_fixups.engine_plane_obstacle_args([]) == []


class FakePcbnew:
    F_Cu, B_Cu = 0, 2

    @staticmethod
    def ToMM(nm):
        return nm / 1e6


class FailingBoard:
    def GetDesignSettings(self):
        raise RuntimeError("boom")


def test_a_failing_fix_up_is_logged_and_leaves_the_dsn(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(TWO, encoding="utf-8")
    opts = options.RouteOptions(edge_and_pad_keepouts=True)
    args, lines = export_flow.fix_dsn(FailingBoard(), FakePcbnew, dsn, opts)
    assert args == [] and len(lines) == 1 and lines[0].startswith("WARNING: DSN fix-ups skipped: RuntimeError")
    assert dsn.read_text(encoding="utf-8") == TWO


def test_the_preset_reproduces_dsn_export_on_the_fixture():
    """The canary preset's DSN settings are dsn_export.py's command line."""
    s = options.dsn_settings(options.with_preset(options.RouteOptions()))
    assert s == {
        "class_widths": {"Power_High": 0.3, "Switch_Node": 0.3, "Power": 0.3, "GND": 0.4},
        "fixed_pour_classes": ["Power_High", "Switch_Node"],
        "drop_pour_nets": ["GND"],
        "edge_and_pad_keepouts": True,
        "route_on_plane_layers": [],
    }


SES = """(session "b.ses"
  (base_design "b.dsn")
  (routes
    (resolution um 10)
    (parser
      (host_cad "KiCad's Pcbnew")
    )
    (library_out
      (padstack "Via[0-3]_600:300_um"
        (shape
          (circle F.Cu 6000 0 0)
        )
      )
    )
    (network_out
      (net "+12V_BUS"
        (wire
          (path F.Cu 3000
            1263679 -607500  1270000 -607500
          )
        )
        (via "Via[0-3]_600:300_um" 1263679 -607500)
      )
      (net GND
        (via "Via[0-3]_800:400_um" 1300000 -700000)
        (via "custom" 1310000 -700000)
      )
    )
  )
)
"""


def test_the_session_vias_carry_their_padstack_sizes_in_kicads_frame():
    import ses_vias

    assert ses_vias.vias(SES) == [
        ("+12V_BUS", 126367900, 60750000, 600000, 300000),
        ("GND", 130000000, 70000000, 800000, 400000),
    ]
    assert ses_vias.vias("(session x)") == []


class FakeVia:
    def __init__(self, net, x, y, drill, locked=False, kind="PCB_VIA"):
        self.net, self.pos, self.drill, self.locked, self.kind = net, type("P", (), {"x": x, "y": y})(), drill, locked, kind

    def GetClass(self):
        return self.kind

    def IsLocked(self):
        return self.locked

    def GetPosition(self):
        return self.pos

    def GetNetname(self):
        return self.net

    def GetDrill(self):
        return self.drill

    def SetDrill(self, value):
        self.drill = value


def test_imported_vias_get_the_sessions_drill_not_the_class_drill():
    """KiCad's import drilled a 0.6 mm +12V_BUS via 0.4 mm (the Power_High class drill)."""
    import kicad_board
    import ses_vias

    wrong = FakeVia("+12V_BUS", 126367920, 60750000, 400000)  # 20 nm off: KiCad's rounding
    right = FakeVia("GND", 130000000, 70000000, 400000)
    locked = FakeVia("+12V_BUS", 126367900, 60750000, 400000, locked=True)
    other_net = FakeVia("SIG", 126367900, 60750000, 400000)
    track = FakeVia("+12V_BUS", 126367900, 60750000, 0, kind="PCB_TRACK")
    board = type("B", (), {"GetTracks": lambda self: [wrong, right, locked, other_net, track]})()
    assert kicad_board.fix_via_drills(board, ses_vias.vias(SES)) == 1
    assert (wrong.drill, right.drill, locked.drill, other_net.drill) == (300000, 400000, 400000, 400000)


def test_outline_segments_off_the_bounding_box_get_their_own_keepouts():
    """A notched outline: the two segments of the notch are not on a side of the box."""
    rings = [[(100.0, 50.0), (150.0, 50.0), (150.0, 90.0), (120.0, 90.0), (120.0, 80.0), (100.0, 80.0)]]
    board = dict(BOARD, pads=[], outline_rings_mm=rings)
    out, _ = dsn_fixups.apply(TWO, board, {"edge_and_pad_keepouts": True})
    quads = [line.strip() for line in out.splitlines() if "(keepout" in line and "polygon" in line]
    assert len(quads) == 4  # two segments on two signal layers
    assert quads[0] == (
        '(keepout "" (polygon F.Cu 0 120300 -90300 120300 -79700 119700 -79700 119700 -90300 120300 -90300))'
    )
    assert all("B.Cu" in q for q in quads[2:])
