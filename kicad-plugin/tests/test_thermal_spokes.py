"""Q22.20: room for a zone's thermal spokes (kicad_board.thermal_facts collects
the pads and zones, thermal_spokes.apply writes one `thermal_spoke` keepout per
spoke).

The fixture is `crates/freerouting/tests/data/quality/q22-thermal.kicad_pcb`
(mm, KiCad's frame): a GND zone on F.Cu and B.Cu over (10..30, 5..25), thermal
gap 0.5, spoke 0.5, clearance 0.3, minimum width 0.2; J1 (20, 15) a 1.7 mm
through-hole GND pad; U2 (14, 9) a 2 x 1 mm F.Cu GND pad turned 20 degrees with
a 30 degree spoke angle; U3 (26, 21) a through-hole GND pad with a 30 degree
spoke angle; U4 (14, 21) a through-hole GND pad with `(zone_connect 2)`; U5
(24, 9) a 1 mm through-hole GND pad 1.27 mm from one of net A; P2
(35, 15) a GND pad outside the zone; PA1 (16.5, 15) and PA2 (23.5, 15) of net A.
`q22-thermal-base.dsn` is KiCad 10.0.7's export of it; `q22-thermal.dsn`, which
the engine's test routes (`crates/freerouting/tests/thermal_spokes.rs`) and
the reader's test compares (`crates/freerouting-kicad/tests/thermal_spokes.rs`),
is what the plugin makes of it."""

import math
from types import SimpleNamespace

import export_flow
import kicad_board
import pytest
import thermal_spokes
from conftest import REPO

DATA = REPO / "crates" / "freerouting" / "tests" / "data" / "quality"
PCB = DATA / "q22-thermal.kicad_pcb"
BASE = (DATA / "q22-thermal-base.dsn").read_text(encoding="utf-8")
ROUTED_INPUT = DATA / "q22-thermal.dsn"


def pad(net, centre, size, layers, circle, pth, angle, orient=0.0, zone_connect=None):
    return {
        "net": net,
        "centre_mm": centre,
        "size_mm": size,
        "orient": orient,
        "circle": circle,
        "custom": False,
        "pth": pth,
        "layers": layers,
        "zone_connect": zone_connect,
        "gap_mm": None,
        "spoke_mm": None,
        "angle": angle,
    }


BOTH = ["F.Cu", "B.Cu"]
ZONE = {
    "net": "GND",
    "layers": BOTH,
    "priority": 0,
    "connect": 1,
    "gap_mm": 0.5,
    "spoke_mm": 0.5,
    "clearance_mm": 0.3,
    "min_thickness_mm": 0.2,
    "outline_mm": [(10.0, 5.0), (30.0, 5.0), (30.0, 25.0), (10.0, 25.0)],
    "holes_mm": [],
    "rule_area": False,
    "no_fill": False,
}
FACTS = {
    "min_resolved_spokes": 2,
    "pads": [
        pad("GND", (20.0, 15.0), (1.7, 1.7), BOTH, True, True, 45.0),
        pad("GND", (35.0, 15.0), (1.0, 1.0), ["F.Cu"], False, False, 90.0),
        pad("A", (16.5, 15.0), (1.0, 1.0), ["F.Cu"], False, False, 90.0),
        pad("A", (23.5, 15.0), (1.0, 1.0), ["F.Cu"], False, False, 90.0),
        pad("GND", (14.0, 9.0), (2.0, 1.0), ["F.Cu"], False, False, 30.0, orient=20.0),
        pad("GND", (26.0, 21.0), (1.7, 1.7), BOTH, True, True, 30.0),
        pad("GND", (14.0, 21.0), (1.7, 1.7), BOTH, True, True, 45.0, zone_connect=2),
        pad("GND", (24.0, 9.0), (1.0, 1.0), BOTH, True, True, 45.0),
        pad("A", (25.27, 9.0), (1.0, 1.0), BOTH, True, True, 45.0),
    ],
    "zones": [ZONE],
}


def test_the_fixture_gives_the_routed_dsn():
    """Every thermal pad gets a keepout along each spoke: J1, U3 and U5 on
    both layers, U2 on F.Cu, none for U4 (solid) or P2 (outside the zone): 28,
    before the `(via ...)`, with `(clearance_class null)`."""
    text, log = thermal_spokes.apply(BASE, FACTS)
    assert log == ["thermal spokes: 28 spoke way(s) kept clear of other nets"]
    assert text.count('(keepout "thermal_spoke"') == 28
    assert text.count("(clearance_class null)") == 28
    assert text.index("thermal_spoke") < text.index("(via ")
    assert text == ROUTED_INPUT.read_text(encoding="utf-8")


def test_j1s_spokes_run_along_the_diagonals():
    """J1's first spoke way: from the centre up and right at 45 degrees (DSN, Y
    up) to 0.85 + 0.5 + 0.1 + 0.5 + 0.3 = 2.25 mm, 0.5 + 2 x 0.3 = 1.1 mm wide."""
    (layer, poly), *_ = thermal_spokes.corridors(FACTS, {"F.Cu", "B.Cu"})
    assert layer == "F.Cu"
    far = ((poly[1][0] + poly[2][0]) / 2, (poly[1][1] + poly[2][1]) / 2)
    assert far[0] - 20.0 == pytest.approx(2.25 / math.sqrt(2))
    assert far[1] - 15.0 == pytest.approx(-2.25 / math.sqrt(2))
    assert math.dist(poly[1], poly[2]) == pytest.approx(1.1)


def test_nothing_without_min_resolved_spokes_or_on_power_layers():
    assert thermal_spokes.apply(BASE, dict(FACTS, min_resolved_spokes=0)) == (BASE, [])
    assert thermal_spokes.corridors(FACTS, {"In1.Cu"}) == []


def test_no_keepout_where_the_zone_does_not_reach_the_spoke():
    """Solid zones, a higher-priority zone of another net over J1's F.Cu spoke
    ends, and a rule area that keeps fill out there: J1's F.Cu ones go."""
    assert thermal_spokes.corridors(dict(FACTS, zones=[dict(ZONE, connect=2)]), set(BOTH)) == []
    box = [(17.0, 12.0), (23.0, 12.0), (23.0, 18.0), (17.0, 18.0)]
    other = dict(ZONE, net="A", layers=["F.Cu"], priority=1, outline_mm=box)
    assert len(thermal_spokes.corridors(dict(FACTS, zones=[ZONE, other]), set(BOTH))) == 24
    rule = dict(ZONE, net="", layers=["F.Cu"], rule_area=True, no_fill=True, outline_mm=box)
    assert len(thermal_spokes.corridors(dict(FACTS, zones=[ZONE, rule]), set(BOTH))) == 24


def test_a_through_hole_only_zone_skips_smd_pads():
    got = thermal_spokes.corridors(dict(FACTS, zones=[dict(ZONE, connect=3)]), set(BOTH))
    assert len(got) == 24  # U2 is SMD


def test_the_fix_up_logs_a_failure_and_leaves_the_dsn(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(BASE, encoding="utf-8")
    lines = export_flow.fix_thermal_spokes(object(), SimpleNamespace(), str(dsn))
    assert lines[0].startswith("WARNING: thermal-spoke keepouts skipped")
    assert dsn.read_text(encoding="utf-8") == BASE


# --- the fixture, through KiCad's own pcbnew (skipped without it) --------------


def test_the_fixture_through_pcbnew_gives_the_facts_and_kicads_spokes():
    """With KiCad's pcbnew: `thermal_facts` reads the fixture as `FACTS`, and
    after KiCad's own fill each keepout holds a spoke: the point half the gap
    past the pad's edge on its axis is filled copper."""
    pcbnew = pytest.importorskip("pcbnew")
    board = pcbnew.LoadBoard(str(PCB))
    got = kicad_board.thermal_facts(board, pcbnew)
    assert got["min_resolved_spokes"] == 2
    assert len(got["pads"]) == len(FACTS["pads"])
    points = ("centre_mm", "size_mm", "outline_mm")
    for a, b in zip(got["pads"] + got["zones"], FACTS["pads"] + FACTS["zones"]):
        assert {k: v for k, v in a.items() if k not in points} == {k: v for k, v in b.items() if k not in points}
        for k in points:
            if k in b:
                assert [tuple(v) for v in a[k]] == pytest.approx(b[k]) if k == "outline_mm" else a[k] == pytest.approx(b[k])
    assert len(got["zones"]) == 1
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
    zone = list(board.Zones())[0]
    layers = {"F.Cu": pcbnew.F_Cu, "B.Cu": pcbnew.B_Cu}
    pads = [p for fp in board.GetFootprints() for p in fp.Pads()]
    for layer, poly in thermal_spokes.corridors(got, set(BOTH)):
        c = ((poly[0][0] + poly[3][0]) / 2, (poly[0][1] + poly[3][1]) / 2)
        f = ((poly[1][0] + poly[2][0]) / 2, (poly[1][1] + poly[2][1]) / 2)
        d = ((f[0] - c[0]) / math.dist(c, f), (f[1] - c[1]) / math.dist(c, f))
        owner = next(p for p in pads if pcbnew.ToMM(p.GetPosition().x) == pytest.approx(c[0]) and pcbnew.ToMM(p.GetPosition().y) == pytest.approx(c[1]))
        t = 0.0
        while owner.HitTest(pcbnew.VECTOR2I(pcbnew.FromMM(c[0] + d[0] * t), pcbnew.FromMM(c[1] + d[1] * t))):
            t += 0.005
        probe = pcbnew.VECTOR2I(pcbnew.FromMM(c[0] + d[0] * (t + 0.25)), pcbnew.FromMM(c[1] + d[1] * (t + 0.25)))
        assert zone.GetFilledPolysList(layers[layer]).Contains(probe), (layer, c, d)
