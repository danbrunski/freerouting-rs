"""K15.33: the DSN input passes each net's own zone thickness where it is above the
board's smallest (`router.cleanup.pour_min_thickness_nets`): KiCad's filler deletes
a blob smaller than the zone's own min_thickness, and the DSN export drops it."""

from test_pour_rules import B_CU, CLEARANCE, F_CU, IN1_CU, THICKNESS, Zone, run  # noqa: F401 (fixture)

NETS = "--router.cleanup.pour_min_thickness_nets="


class NetZone(Zone):
    def __init__(self, layers, net, thickness_mm, clearance_mm=0.5):
        super().__init__(layers, thickness_mm, clearance_mm)
        self.net = net

    def GetNetname(self):
        return self.net


def test_a_net_whose_zones_are_thicker_passes_its_own(run):
    # the pcu board: EFUSE_NO_5 (/CH5_NO) has 0.5 mm, two F.Cu zones 0.4 mm
    zones = [
        NetZone([F_CU], "/CH5_NO", 0.5),
        NetZone([B_CU], "/CH5_NO", 0.6),
        NetZone([F_CU], "Net-(U24-SW2)", 0.4),
        NetZone([F_CU], "/BUCK_6V", 0.4),
        NetZone([B_CU], "/BUCK_6V", 0.5),  # its smallest is the board's: no entry
        NetZone([F_CU], "BUS_GND", 0.45),
        NetZone([IN1_CU], "GND", 0.8),  # a power layer: no signal-layer pour
    ]
    args, lines = run(zones)
    assert args == [THICKNESS + "400", CLEARANCE + "500", NETS + "500:/CH5_NO,450:BUS_GND"]
    assert lines == [
        "signal-layer zones: 6 on F.Cu, B.Cu; the cleanup judges their pours by min thickness 400 um and "
        f"clearance 500 um, 2 net(s) by their own thicker zones ({' '.join(args)})"
    ]


def test_a_name_the_list_cannot_carry_keeps_the_boards_smallest(run):
    zones = [
        NetZone([F_CU], "A,B", 0.5),
        NetZone([F_CU], " padded", 0.5),
        NetZone([F_CU], "", 0.5),
        NetZone([F_CU], "x:y", 0.6),  # the router splits at the first colon, after the number
        NetZone([F_CU], "THIN", 0.3),
    ]
    args, _ = run(zones)
    assert args == [THICKNESS + "300", CLEARANCE + "500", NETS + "600:x:y"]


def test_one_thickness_on_every_zone_passes_no_net_entry(run):
    args, _ = run([NetZone([F_CU], "A", 0.5), NetZone([B_CU], "B", 0.5)])
    assert args == [THICKNESS + "500", CLEARANCE + "500"]
