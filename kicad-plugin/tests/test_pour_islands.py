"""K15.29: the DSN input passes whether KiCad's filler removes the islands of the
board's signal-layer zones (`router.cleanup.pour_island_removal`) and the nets of the
zones that keep them (`pour_islands_kept_nets`); KiCad's DSN export drops both."""

import options
from test_pour_rules import B_CU, CLEARANCE, F_CU, IN1_CU, THICKNESS, Zone, run  # noqa: F401 (fixture)

REMOVAL = "--router.cleanup.pour_island_removal=true"
KEPT = "--router.cleanup.pour_islands_kept_nets="


class ModeZone(Zone):
    def __init__(self, layers, net, mode, thickness_mm=0.5, clearance_mm=0.5, rule_area=False):
        super().__init__(layers, thickness_mm, clearance_mm, rule_area)
        self.net, self.mode = net, mode

    def GetNetname(self):
        return self.net

    def GetIslandRemovalMode(self):
        return self.mode


def test_zones_that_all_remove_islands_pass_the_removal(run):
    args, lines = run([ModeZone([F_CU], "Net-(U24-OUT)", 0), ModeZone([B_CU], "Net-(U24-OUT)", 0)])
    assert args == [THICKNESS + "500", CLEARANCE + "500", REMOVAL]
    assert lines == [
        "signal-layer zones: 2 on F.Cu, B.Cu; the cleanup judges their pours by min thickness 500 um and "
        f"clearance 500 um, islands no pad reaches dropped (kept on 0 net(s)) ({' '.join(args)})"
    ]


def test_the_nets_of_zones_that_keep_islands_are_passed(run):
    # the pcu board: one F.Cu zone (BUCK_SW, Net-(U1-SW)) keeps its islands; mode 2 keeps the small ones
    zones = [
        ModeZone([F_CU], "Net-(U24-OUT)", 0),
        ModeZone([F_CU], "Net-(U1-SW)", 1),
        ModeZone([B_CU], "/USB PD Source Circuit/USB_GND", 2),
        ModeZone([IN1_CU], "BUS_GND", 1),  # a power layer: no signal-layer pour
        ModeZone([F_CU], "GND", 1, rule_area=True),
    ]
    args, _ = run(zones)
    assert args == [
        THICKNESS + "500",
        CLEARANCE + "500",
        REMOVAL,
        KEPT + "/USB PD Source Circuit/USB_GND,Net-(U1-SW)",
    ]


def test_an_unreadable_mode_passes_no_removal(run):
    # test_pour_rules' Zone has no GetIslandRemovalMode, as a pcbnew without it
    args, lines = run([ModeZone([F_CU], "A", 0), Zone([B_CU], 0.5, 0.5)])
    assert args == [THICKNESS + "500", CLEARANCE + "500"]
    assert "islands" not in lines[0]


def test_a_net_name_the_list_cannot_carry_passes_no_removal():
    for name in ("A,B", " A", ""):
        assert options.pour_rule_args(500_000, 500_000, True, [name]) == [THICKNESS + "500", CLEARANCE + "500"]
    assert options.pour_rule_args(0, 0, True, ["A", "B"]) == [REMOVAL, KEPT + "A,B"]
    assert options.pour_rule_args(500_000, 0, False, ["A"]) == [THICKNESS + "500"]
