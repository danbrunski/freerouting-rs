"""Q18.7: the pads' solder-mask apertures reach the router as
`--router.solder_mask_expansions` (kicad_board.mask_expansions,
options.solder_mask_args, export_flow.solder_mask_args)."""

from types import SimpleNamespace

import export_flow
import kicad_board
import options
from conftest import TESTS

TWO = (TESTS / "data" / "k09_two_layer.dsn").read_text(encoding="utf-8")  # clearance 200 um

PCBNEW = SimpleNamespace(F_Cu=0, B_Cu=31, F_Mask=39, B_Mask=38, PAD_ATTRIB_NPTH=3, PAD_ATTRIB_SMD=1, ToMM=lambda v: v / 1e6)


class Pad:
    def __init__(self, number, layers, expansion_nm, attr=1, per_layer=True):
        self.number, self.layers, self.e, self.attr, self.per_layer = number, set(layers), expansion_nm, attr, per_layer

    def GetAttribute(self):
        return self.attr

    def IsOnLayer(self, layer):
        return layer in self.layers

    def GetNumber(self):
        return self.number

    def GetSolderMaskExpansion(self, *layer):
        if layer and not self.per_layer:
            raise TypeError("KiCad 8 takes no layer")
        return self.e


class Footprint:
    def __init__(self, ref, pads):
        self.ref, self.pads = ref, pads

    def GetReference(self):
        return self.ref

    def Pads(self):
        return self.pads


class Board:
    def __init__(self, footprints):
        self.footprints = footprints

    def GetFootprints(self):
        return self.footprints


P = PCBNEW
FRONT = (P.F_Cu, P.F_Mask)
BACK = (P.B_Cu, P.B_Mask)


def test_only_apertures_past_the_clearance_are_listed_per_side():
    # pocketsdr-v3-0's TP9: a back test point, 0.1016 mm mask margin, 0.09 mm class
    tp9 = Board([Footprint("TP9", [Pad("1", BACK, 101600)])])
    assert kicad_board.mask_expansions(tp9, PCBNEW, 0.09) == [{"ref": "TP9", "pin": "1", "front_mm": 0.0, "back_mm": 0.1016}]
    assert kicad_board.mask_expansions(tp9, PCBNEW, 0.2) == []
    board = Board(
        [
            Footprint("TP9", [Pad("1", BACK, 101600)]),
            Footprint("R1", [Pad("1", FRONT, 50800), Pad("2", FRONT, 50800)]),  # within 0.2 mm
            Footprint("J1", [Pad("1", FRONT + BACK, 300000), Pad("2", (P.F_Cu, P.B_Cu, P.F_Mask), 300000)]),
            Footprint("H1", [Pad("", FRONT + BACK, 500000, attr=P.PAD_ATTRIB_NPTH)]),  # no pin
            Footprint("U1", [Pad("EP", FRONT, 250000), Pad("EP", FRONT, 250000), Pad("EP", (P.F_Cu,), 250000)]),
        ]
    )
    got = kicad_board.mask_expansions(board, PCBNEW, 0.2)
    assert got == [
        {"ref": "J1", "pin": "1", "front_mm": 0.3, "back_mm": 0.3},
        {"ref": "J1", "pin": "2", "front_mm": 0.3, "back_mm": 0.0},  # tented on the back
        {"ref": "U1", "pin": "EP", "front_mm": 0.25, "back_mm": 0.0},
        {"ref": "U1", "pin": "EP@1", "front_mm": 0.25, "back_mm": 0.0},  # KiCad's name for a repeat
    ]
    assert options.solder_mask_args(got) == [
        "--router.solder_mask_expansions=J1:1=300,300;J1:2=300,0;U1:EP=250,0;U1:EP@1=250,0"
    ]


def test_kicad_8_has_one_expansion_per_pad():
    board = Board([Footprint("TP1", [Pad("1", FRONT, 500000, per_layer=False)])])
    assert kicad_board.mask_expansions(board, PCBNEW, 0.2) == [{"ref": "TP1", "pin": "1", "front_mm": 0.5, "back_mm": 0.0}]


def test_no_aperture_past_the_clearance_passes_nothing(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(TWO, encoding="utf-8")
    board = Board([Footprint("R1", [Pad("1", FRONT, 150000)])])
    assert export_flow.solder_mask_args(board, PCBNEW, dsn) == ([], [])
    assert options.solder_mask_args([]) == []


def test_the_floor_is_the_dsns_smallest_class_clearance():
    import dsn_fixups

    assert dsn_fixups.Dsn(TWO).min_clearance_mm() == 0.2
    # the smd_smd 50 um is a typed rule, not a clearance the router keeps from a pad
    narrow = TWO.replace("(clearance 250)", "(clearance 90)", 1)
    assert dsn_fixups.Dsn(narrow).min_clearance_mm() == 0.09


def test_the_flow_logs_the_flag_and_never_raises(tmp_path):
    dsn = tmp_path / "b.dsn"
    dsn.write_text(TWO, encoding="utf-8")
    board = Board([Footprint("TP1", [Pad("1", FRONT, 500000)])])
    args, lines = export_flow.solder_mask_args(board, PCBNEW, dsn)
    assert args == ["--router.solder_mask_expansions=TP1:1=500,0"]
    assert lines == ["solder mask: 1 pad(s) whose aperture reaches past their clearance kept clear of other nets"]
    args, lines = export_flow.solder_mask_args(object(), PCBNEW, dsn)
    assert args == [] and lines[0].startswith("WARNING: solder-mask apertures skipped: AttributeError")
