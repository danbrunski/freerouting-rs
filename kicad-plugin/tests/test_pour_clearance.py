"""K15.32: the pour clearance the plugin passes is the smallest clearance KiCad's zone filler can
apply: the zones' own, every `clearance` constraint of the `.kicad_dru` (it replaces the zone's
value in KiCad 10.0.7, K15.29). The net classes' are not read. An unreadable `.kicad_dru` passes none."""

import pytest

import export_flow
import kicad_rules
import options
from test_pour_rules import B_CU, F_CU, NAMES, PCBNEW, STACK, Zone, dsn_text

MM = 1_000_000
CLEARANCE = "--router.cleanup.pour_clearance_um="
THICKNESS = "--router.cleanup.pour_min_thickness_um="


class Board:
    def __init__(self, path, zones):
        self.path, self.zones = path, zones

    def Zones(self):
        return self.zones

    def GetLayerName(self, layer):
        return NAMES[layer]

    def GetFileName(self):
        return str(self.path)


RULES = '''(version 1)
# a comment with a (paren
(rule "Minimum copper-to-copper spacing (5 mil)"
    (constraint clearance (min 0.127mm))
    (severity error))
(rule "Drill-to-copper clearance"
    (constraint hole_clearance (min 0.05mm)))
'''


@pytest.fixture
def run(tmp_path, monkeypatch):
    """`prepare` on a stub pcbnew with the other fix-ups off: `go(zones, dru=None, **board)` gives (args, lines)."""
    for name in ("fix_plane_priorities", "fix_rule_areas", "fix_edge_graphics", "fix_copper_graphics",
                 "fix_thermal_spokes", "fix_track_arcs"):
        monkeypatch.setattr(export_flow, name, lambda *_: [])
    monkeypatch.setattr(export_flow, "refill", lambda *_: [])
    monkeypatch.setattr(export_flow, "solder_mask_args", lambda *_: ([], []))
    monkeypatch.setattr(export_flow, "fix_dsn", lambda *_: ([], []))

    def go(zones, dru=None):
        pcb = tmp_path / "b.kicad_pcb"
        pcb.write_text("(kicad_pcb)", encoding="utf-8")
        dru_file = tmp_path / "b.kicad_dru"
        if dru is not None:
            dru_file.write_text(dru, encoding="utf-8")
        elif dru_file.exists():
            dru_file.unlink()
        dsn = tmp_path / "board.dsn"

        def export(_board, path):
            path.write_text(dsn_text(STACK), encoding="utf-8")
            return True

        ok, args, lines, _ = export_flow.prepare(Board(pcb, zones), PCBNEW, dsn, options.RouteOptions(), export)
        assert ok
        return args, lines

    return go


ZONES = [Zone([F_CU], 0.5, 0.5), Zone([B_CU], 0.5, 0.5)]


def test_a_kicad_dru_clearance_below_the_zones_is_the_pour_clearance(run):
    args, lines = run(ZONES, RULES)
    assert args == [THICKNESS + "500", CLEARANCE + "127"]
    assert any("b.kicad_dru: 1 clearance rule(s), smallest 127 um" in line for line in lines)
    assert any("min thickness 500 um and clearance 127 um" in line for line in lines)


def test_without_a_kicad_dru_rule_the_zones_smallest_clearance_stays(run):
    assert run(ZONES)[0] == [THICKNESS + "500", CLEARANCE + "500"]
    # a file with no clearance rule says nothing about the gap
    assert run(ZONES, "(version 1)\n(rule x (constraint track_width (min 0.1mm)))\n")[0][-1] == CLEARANCE + "500"
    # a rule larger than the zones' does not raise it
    assert run(ZONES, "(version 1)\n(rule x (constraint clearance (min 0.9mm)))\n")[0][-1] == CLEARANCE + "500"


def test_an_unreadable_kicad_dru_passes_no_clearance(run):
    for text in ('(version 1)\n(rule "a" (constraint clearance (min 0.127mm))', "(version 1))", '(rule "a',
                 "(rule x (constraint clearance (min wide)))"):
        args, lines = run(ZONES, text)
        assert args == [THICKNESS + "500"], text
        assert any(line.startswith("WARNING: b.kicad_dru unreadable") and "no pour clearance passed" in line
                   for line in lines), lines


def test_a_conditional_rule_counts_and_other_constraints_do_not(run):
    text = '''(version 1)
(rule "wide" (condition "A.NetName == 'X'") (constraint clearance (min 0.3mm)))
(rule "narrow" (layer outer) (condition "A.Type == 'Track' && B.Type == 'Zone'") (constraint clearance (min 8mil)))
(rule "physical" (constraint physical_clearance (min 0.01mm)))
(rule "holes" (constraint hole_clearance (min 0.01mm)))
'''
    assert run(ZONES, text)[0][-1] == CLEARANCE + "203.2"


def test_a_clearance_rule_of_zero_passes_no_floor(run):
    args, _ = run(ZONES, "(version 1)\n(rule z (constraint clearance (min 0)))\n")
    assert args == [THICKNESS + "500"]
    args, _ = run(ZONES, "(version 1)\n(rule z (constraint clearance (max 1mm)))\n")  # no minimum
    assert args == [THICKNESS + "500"]


def test_rules_parser_units_comments_and_quotes():
    text = '''(version 1)
# (rule "commented" (constraint clearance (min 1mm)))
(rule "a (b) \\"q\\"" (constraint clearance (min 5mil) (opt 0.3)) (constraint clearance (min .1)))
(rule b (constraint clearance (min 1in)) (constraint clearance (min 20um)))
'''
    got = kicad_rules.clearance_minimums_nm(text)
    assert got == [('a (b) "q"', 127_000), ('a (b) "q"', 100_000), ("b", 25_400_000), ("b", 20_000)]
    assert kicad_rules.smallest_clearance_nm(text) == 20_000
    assert kicad_rules.smallest_clearance_nm("(version 1)") is None
    for bad in ("(a", "a)", '(a "b', "(rule x (constraint clearance (min)))"):
        with pytest.raises(kicad_rules.RulesError):
            kicad_rules.clearance_minimums_nm(bad)
