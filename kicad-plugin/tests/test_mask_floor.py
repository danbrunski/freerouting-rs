"""K13.1: the plugin's DSN path and the `.kicad_pcb` reader filter the pads'
mask expansions at the smallest class clearance the same way: each side
alone, a side at or under the floor is 0, a pad with no side left is not
listed. The pads are those of
`crates/freerouting-kicad/tests/data/k13-mask-floor.kicad_pcb`, whose reader
output `k13_inputs.rs::mask_expansions_are_filtered_per_side_at_the_smallest_class_clearance`
pins: F1 (300, 0), T1 (250, 250), T2 (300, 0), B1 (0, 300) µm."""

from types import SimpleNamespace

import kicad_board

PCBNEW = SimpleNamespace(F_Cu=0, B_Cu=31, F_Mask=39, B_Mask=38, PAD_ATTRIB_NPTH=3, PAD_ATTRIB_SMD=1, ToMM=lambda v: v / 1e6)
P = PCBNEW
FRONT = (P.F_Cu, P.F_Mask)
BACK = (P.B_Cu, P.B_Mask)
BOTH = FRONT + BACK


class Pad:
    """A pad with one expansion per side (KiCad 9+ `GetSolderMaskExpansion(layer)`)."""

    def __init__(self, layers, front_nm, back_nm=None):
        self.layers = set(layers)
        self.e = {P.F_Cu: front_nm, P.B_Cu: front_nm if back_nm is None else back_nm}

    def GetAttribute(self):
        return P.PAD_ATTRIB_SMD

    def IsOnLayer(self, layer):
        return layer in self.layers

    def GetNumber(self):
        return "1"

    def GetSolderMaskExpansion(self, layer):
        return self.e[layer]


class Footprint:
    def __init__(self, ref, pad):
        self.ref, self.pad = ref, pad

    def GetReference(self):
        return self.ref

    def Pads(self):
        return [self.pad]


class Board:
    def __init__(self, footprints):
        self.footprints = footprints

    def GetFootprints(self):
        return self.footprints


def listed(board, floor_mm=0.2):
    return [(e["ref"], e["front_mm"], e["back_mm"]) for e in kicad_board.mask_expansions(board, PCBNEW, floor_mm)]


def test_the_readers_fixture_pads_give_the_readers_list():
    board = Board(
        [
            Footprint("F1", Pad(FRONT, 300000)),
            Footprint("F2", Pad(FRONT, 200000)),  # at the floor
            Footprint("T1", Pad(BOTH, 250000)),
            Footprint("T2", Pad((P.F_Cu, P.B_Cu, P.F_Mask), 300000)),  # tented on the back
            Footprint("T3", Pad(BOTH, 100000)),
            Footprint("B1", Pad(BACK, 300000)),
        ]
    )
    assert listed(board) == [("F1", 0.3, 0.0), ("T1", 0.25, 0.25), ("T2", 0.3, 0.0), ("B1", 0.0, 0.3)]


def test_each_side_is_judged_alone():
    # the reader's `mask::keep_above_floor` on (300, 100) and (200, 200.001) µm
    board = Board([Footprint("U1", Pad(BOTH, 300000, 100000)), Footprint("U2", Pad(BOTH, 200000, 200001))])
    assert listed(board) == [("U1", 0.3, 0.0), ("U2", 0.0, 0.200001)]


# --- K13.1: pads' own clearances, the same circles on both inputs --------------


def test_pad_clearance_circles_match_the_readers():
    """The pads of `crates/freerouting-kicad/tests/data/k13-pad-clearance.kicad_pcb`,
    as `kicad_board._pads` reports them (absolute positions from pcbnew), give
    the circles `k13_inputs.rs::pads_own_clearances_become_circle_keepouts` pins."""
    import dsn_fixups
    from conftest import TESTS

    dsn = dsn_fixups.Dsn((TESTS / "data" / "k09_two_layer.dsn").read_text(encoding="utf-8"))
    assert dsn.default_clearance_mm() == 0.2
    board = {
        "pads": [
            {"ref": "FID1", "local_clearance_mm": 0.6, "size_mm": (1.0, 1.0), "at_mm": (10.0, 10.0), "layers": ["F.Cu"]},
            {"ref": "J1", "local_clearance_mm": 0.5, "size_mm": (1.5, 1.0), "at_mm": (20.0, 8.0), "layers": ["F.Cu", "B.Cu"]},
            {"ref": "R1", "local_clearance_mm": 0.1, "size_mm": (0.6, 0.6), "at_mm": (25.0, 10.0), "layers": ["F.Cu"]},
            {"ref": "B1", "local_clearance_mm": 0.4, "size_mm": (0.8, 0.8), "at_mm": (30.0, 10.0), "layers": ["B.Cu"]},
        ]
    }
    got = [line.strip() for line in dsn_fixups._pad_keepouts(board, dsn, ["F.Cu", "B.Cu"])]
    assert got == [
        '(keepout "" (circle F.Cu 1800 10000 -10000))',
        '(keepout "" (circle F.Cu 2100 20000 -8000))',
        '(keepout "" (circle B.Cu 2100 20000 -8000))',
        '(keepout "" (circle B.Cu 1200 30000 -10000))',
    ]
