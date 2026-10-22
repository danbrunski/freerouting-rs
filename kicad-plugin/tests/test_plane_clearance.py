"""K15.34: a plane is cut where a higher-priority zone fills with the clearance KiCad's filler
applies there. KiCad 10.0.7 resolves a zone-to-zone gap as it does zone-to-track (K15.32): the larger
of the two zones' own clearances, unless a `.kicad_dru` clearance rule matches, which replaces it.
Measured with `kicad-cli pcb drc --refill-zones --save-board` on two zones of 0.5 mm, priority 1 and
2: no rule, the lower fill ends 0.5055 mm from the higher (0.5 plus the 5 um arc error); a rule of
0.127 mm, 0.1325 mm; 0.4 mm, 0.4055 mm; a rule on `A.Type == 'Zone'`, 0.1325 mm; a rule whose
condition matches nothing, 0.5055 mm; a conditional rule that matches, its value; zones of 0.2 and
0.5 mm, 0.5055 mm either way. The plugin cuts with the smaller of the zones' value and the
`.kicad_dru`'s smallest clearance rule, as K15.32's pour clearance."""

import export_flow
import pytest
from test_plane_priority import (
    LOW_BOX,
    NAMES,
    IN2_CU,
    Board,
    PCBNEW,
    Zone,
    covered,
    dsn_text,
    plane_regions,
    rect,
)

HIGH_BOX = (15, 3, 25, 7)  # mm; it reaches 5 mm into the lower plane's right edge


class FileBoard(Board):
    def __init__(self, zones, path):
        super().__init__(zones)
        self.path = path

    def GetFileName(self):
        return str(self.path)


@pytest.fixture
def cut(tmp_path):
    """`go(dru=None, zones=0.5 mm)` runs `fix_plane_priorities` on the two-zone board and gives
    `(regions of LOW, log lines)`; `dru` is the text of the board's `.kicad_dru`."""

    def go(dru=None, low_mm=0.5, high_mm=0.5):
        pcb = tmp_path / "b.kicad_pcb"
        pcb.write_text("(kicad_pcb)", encoding="utf-8")
        dru_file = tmp_path / "b.kicad_dru"
        if dru is not None:
            dru_file.write_bytes(dru if isinstance(dru, bytes) else dru.encode("utf-8"))
        low = Zone("LOW", 1, [IN2_CU], rect(*LOW_BOX), 1, "aaaa", round(low_mm * 1e6))
        high = Zone("HIGH", 2, [IN2_CU], rect(*HIGH_BOX), 41, "bbbb", round(high_mm * 1e6))
        dsn = tmp_path / "board.dsn"
        dsn.write_text(dsn_text([("LOW", "In2.Cu", LOW_BOX), ("HIGH", "In2.Cu", HIGH_BOX)]), encoding="utf-8")
        lines = export_flow.fix_plane_priorities(FileBoard([low, high], pcb), PCBNEW, dsn)
        return plane_regions(dsn.read_text(encoding="utf-8"))["LOW"], lines

    return go


def edge(regions):
    """x (mm) of the lower plane's right edge at y = 5, the first uncovered 1 um step from 12."""
    x = 12.0
    while covered(regions, x, 5):
        x += 0.001
    return round(x, 3)


RULE_127 = '(version 1)\n(rule "5 mil" (constraint clearance (min 0.127mm)))\n'


def test_the_kicad_dru_clearance_cuts_the_lower_plane_closer(cut):
    regions, lines = cut(RULE_127)
    assert edge(regions) == pytest.approx(15 - 0.127 - 0.005, abs=0.002)
    assert covered(regions, 14.8, 5) and not covered(regions, 14.9, 5)
    assert any("b.kicad_dru" in line and "127" in line for line in lines), lines
    assert any(line.startswith("plane LOW on In2.Cu: cut where") for line in lines)


def test_without_a_kicad_dru_rule_the_zones_clearance_cuts(cut, tmp_path):
    regions, lines = cut()
    assert edge(regions) == pytest.approx(15 - 0.5 - 0.005, abs=0.002)
    assert not any("kicad_dru" in line for line in lines), lines
    # a file with no clearance rule is no rule
    regions, lines = cut('(version 1)\n(rule "d" (constraint hole_clearance (min 0.05mm)))\n')
    assert edge(regions) == pytest.approx(15 - 0.505, abs=0.002)


def test_the_larger_of_the_two_zones_clearances_stays_without_a_rule(cut):
    assert edge(cut(low_mm=0.2, high_mm=0.5)[0]) == pytest.approx(14.495, abs=0.002)
    assert edge(cut(low_mm=0.5, high_mm=0.2)[0]) == pytest.approx(14.495, abs=0.002)


def test_an_unreadable_kicad_dru_keeps_the_zones_value_and_says_why(cut):
    for bad in ("(version 1\n(rule a (constraint clearance (min 0.1mm)", b"\xff\xfe(rule", "(rule a (constraint clearance (min abc)))"):
        regions, lines = cut(bad)
        assert edge(regions) == pytest.approx(14.495, abs=0.002), bad
        warn = [line for line in lines if line.startswith("WARNING") and "b.kicad_dru unreadable" in line]
        assert warn and "zones' clearance" in warn[0], lines


def test_a_conditional_rule_counts_and_one_above_the_zones_does_not_widen_the_cut(cut):
    conditional = '(version 1)\n(rule a (condition "A.NetName == \'LOW\'") (constraint clearance (min 0.3mm)))\n'
    assert edge(cut(conditional)[0]) == pytest.approx(14.695, abs=0.002)
    # KiCad keeps 0.8 mm there; the plugin cuts at the smaller of the zones' and the rules' values
    assert edge(cut('(version 1)\n(rule a (constraint clearance (min 0.8mm)))\n')[0]) == pytest.approx(14.495, abs=0.002)
    # a rule of 0 (or without a min) sets no gap
    assert edge(cut('(version 1)\n(rule a (constraint clearance (min 0mm)))\n')[0]) == pytest.approx(14.495, abs=0.002)


def test_nothing_to_cut_logs_nothing_and_leaves_the_file(tmp_path):
    pcb = tmp_path / "b.kicad_pcb"
    pcb.write_text("(kicad_pcb)", encoding="utf-8")
    (tmp_path / "b.kicad_dru").write_text(RULE_127, encoding="utf-8")
    low = Zone("LOW", 1, [IN2_CU], rect(*LOW_BOX), 1, "aaaa", 500_000)
    high = Zone("HIGH", 2, [IN2_CU], rect(25, 0, 35, 10), 41, "bbbb", 500_000)
    dsn = tmp_path / "board.dsn"
    text = dsn_text([("LOW", "In2.Cu", LOW_BOX), ("HIGH", "In2.Cu", (25, 0, 35, 10))])
    dsn.write_text(text, encoding="utf-8")
    assert export_flow.fix_plane_priorities(FileBoard([low, high], pcb), PCBNEW, dsn) == []
    assert dsn.read_text(encoding="utf-8") == text
