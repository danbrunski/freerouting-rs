"""Q22.16: a footprint's own F.Mask / B.Mask shapes become keepouts on their
copper side (kicad_board.edge_graphics collects them with their footprint's
pads, footprint_masks names them, edge_graphics.apply writes them).

The fixture is `crates/freerouting/tests/data/quality/q22-fp-mask.kicad_pcb`
(mm, KiCad's frame): J1 at (20, 11) has an F.Mask polygon over (14..26,
11..19) and two 2 x 6 mm pads of nets A (16..18, 9..15) and B (22..24, 9..15),
which reach 2 mm above it; U1 at (32, 25) an F.Mask rectangle (31.3..32.7) over
its one 1 mm pad of net D. The keepouts are named `solder_mask_pads`; what
they let through is the router's. `q22-fp-mask-base.dsn` is that board's export
without the keepouts; `q22-fp-mask.dsn`, which the engine's test routes
(`crates/freerouting/tests/footprint_mask.rs`), is what the plugin makes of it."""

from types import SimpleNamespace

import edge_graphics
import footprint_masks
import kicad_board
from conftest import REPO

DATA = REPO / "crates" / "freerouting" / "tests" / "data" / "quality"
BASE = (DATA / "q22-fp-mask-base.dsn").read_text(encoding="utf-8")
ROUTED_INPUT = DATA / "q22-fp-mask.dsn"
BOARD_RING = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)]
J1_MASK = [(14.0, 11.0), (26.0, 11.0), (26.0, 19.0), (14.0, 19.0)]
U1_MASK = [(31.3, 24.3), (32.7, 24.3), (32.7, 25.7), (31.3, 25.7)]


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
    def __init__(self, rings=()):
        self.rings = [Chain(r) for r in rings]

    def OutlineCount(self):
        return len(self.rings)

    def Outline(self, i):
        return self.rings[i]

    def HoleCount(self, i):
        return 0


P = SimpleNamespace(
    Edge_Cuts=25,
    Margin=31,
    F_Mask=39,
    B_Mask=38,
    F_Cu=0,
    B_Cu=31,
    PAD_ATTRIB_NPTH=3,
    PAD_ATTRIB_SMD=1,
    SHAPE_T_SEGMENT=0,
    SHAPE_T_RECTANGLE=1,
    SHAPE_T_ARC=2,
    SHAPE_T_CIRCLE=3,
    SHAPE_T_POLY=4,
    SHAPE_T_BEZIER=5,
    ToMM=lambda v: v / 1e6,
    SHAPE_POLY_SET=PolySet,
)


class Angle:
    def __init__(self, deg):
        self.deg = deg

    def AsDegrees(self):
        return self.deg


class Pad:
    def __init__(self, at, size, net, layers=(0,), offset=(0.0, 0.0), angle=0.0, attr=1):
        self.at, self.size, self.net, self.layers = at, size, net, layers
        self.offset, self.angle, self.attr = offset, angle, attr

    def GetPosition(self):
        return nm(*self.at)

    def GetSize(self, layer):
        return nm(*self.size)

    def GetOffset(self, layer):
        return nm(*self.offset)

    def GetOrientation(self):
        return Angle(self.angle)

    def GetNetname(self):
        return self.net

    def IsOnLayer(self, layer):
        return layer in self.layers

    def GetAttribute(self):
        return self.attr


class MaskPoly:
    def __init__(self, ring, layer=39):
        self.ring, self.layer = ring, layer

    def GetLayer(self):
        return self.layer

    def GetShape(self):
        return P.SHAPE_T_POLY

    def GetWidth(self):
        return 0

    def GetPolyShape(self):
        return PolySet([[nm(*q) for q in self.ring]])

    def IsFilled(self):
        return True


class Footprint:
    def __init__(self, ref, items, pads, allow=False):
        self.ref, self.items, self.pads, self.allow = ref, items, pads, allow

    def GetReference(self):
        return self.ref

    def GraphicalItems(self):
        return self.items

    def Pads(self):
        return self.pads

    def AllowSolderMaskBridges(self):
        return self.allow


class Board:
    def __init__(self, footprints):
        self.footprints = footprints

    def GetDrawings(self):
        return []

    def GetFootprints(self):
        return self.footprints

    def GetDesignSettings(self):
        return SimpleNamespace(m_CopperEdgeClearance=500000, m_SolderMaskToCopperClearance=0)

    def GetBoardPolygonOutlines(self, outline, *args):
        outline.rings = [Chain([nm(*p) for p in BOARD_RING])]
        return True


def fixture_board(j1_allow=False, j1_layer=39, d_net="D"):
    j1 = Footprint(
        "J1",
        [MaskPoly(J1_MASK, j1_layer)],
        [Pad((17.0, 12.0), (2.0, 6.0), "A"), Pad((23.0, 12.0), (2.0, 6.0), "B")],
        allow=j1_allow,
    )
    u1 = Footprint("U1", [MaskPoly(U1_MASK)], [Pad((32.0, 25.0), (1.0, 1.0), d_net)])
    pads = [
        Footprint(ref, [], [Pad(at, (1.0, 1.0), net)])
        for ref, at, net in [
            ("PA", (17.0, 3.0), "A"),
            ("PB", (23.0, 3.0), "B"),
            ("PC1", (5.0, 17.0), "C"),
            ("PC2", (35.0, 17.0), "C"),
            ("PD", (22.0, 25.0), "D"),
        ]
    ]
    return Board([j1] + pads[:4] + [u1, pads[4]])


def mask_lines(text):
    return [line.strip() for line in text.splitlines() if '(keepout "solder_mask' in line]


def test_a_footprints_mask_shapes_become_keepouts_named_for_their_pads():
    facts = kicad_board.edge_graphics(fixture_board(), P)
    out, log = edge_graphics.apply(BASE, facts)
    # Q22.16: J1's, over pads of nets A and B, is shared (pad_trim); Q22.18:
    # both keep no clearance of their own
    # and Q22.18's strip within 1 um of each side: J1's top one, over the fingers, is shared too
    lines = mask_lines(out)
    assert len(lines) == 10
    assert lines[0] == (
        '(keepout "solder_mask_shared" (polygon F.Cu 0 14000 -11000 26000 -11000 26000 -19000 14000 -19000 '
        "14000 -11000) (clearance_class null))"
    )
    assert lines[1].startswith('(keepout "solder_mask_shared" (polygon F.Cu 0 13999 -11001 26001 -11001 ')
    assert lines[5] == (
        '(keepout "solder_mask_pads" (polygon F.Cu 0 31300 -24300 32700 -24300 32700 -25700 31300 -25700 '
        "31300 -24300) (clearance_class null))"
    )
    assert log == [
        "solder-mask graphics: 2 footprint F.Mask/B.Mask shape(s) kept clear on their copper side, but for "
        "their pads (10 keepouts)",
        "solder-mask graphics: 2 footprint aperture keepout(s) over pads of several nets keep every net out; "
        "2 pad(s) cut back to their part outside",
    ]
    # the engine's route test reads exactly this
    assert out == ROUTED_INPUT.read_text(encoding="utf-8")


def test_a_footprint_that_allows_mask_bridges_has_no_keepout():
    out, _ = edge_graphics.apply(BASE, kicad_board.edge_graphics(fixture_board(j1_allow=True), P))
    assert len(mask_lines(out)) == 5 and "31300 -24300" in mask_lines(out)[0]


def test_a_back_mask_shape_goes_on_the_bottom_copper():
    out, _ = edge_graphics.apply(BASE, kicad_board.edge_graphics(fixture_board(j1_layer=38), P))
    assert [line.split("(polygon ")[1].split()[0] for line in mask_lines(out)] == ["B.Cu"] * 5 + ["F.Cu"] * 5


def test_the_mask_to_copper_clearance_grows_the_footprint_apertures():
    facts = dict(kicad_board.edge_graphics(fixture_board(), P), mask_to_copper_mm=0.05)
    out, log = edge_graphics.apply(BASE, facts)
    assert len(mask_lines(out)) == 10 and log[0].endswith("(10 keepouts)")


def test_board_and_footprint_mask_shapes_log_apart():
    board_shape = {"layer": "F.Mask", "width_mm": 0.0, "kind": "ring", "points": [(1, 1), (2, 1), (2, 2)], "filled": True}
    facts = kicad_board.edge_graphics(fixture_board(), P)
    facts["shapes"].insert(0, board_shape)
    out, log = edge_graphics.apply(BASE, facts)
    names = [line.split('"')[1] for line in mask_lines(out)]
    assert names == ["solder_mask"] * 4 + ["solder_mask_shared"] * 2 + ["solder_mask_pads"] * 8
    assert log[0].endswith("(4 keepouts)") and log[1].endswith("(10 keepouts)")


def test_a_footprint_whose_bridge_flag_cannot_be_read_is_kept_clear():
    class Broken(Footprint):
        def AllowSolderMaskBridges(self):
            raise RuntimeError("no such attribute here")

    facts = kicad_board.footprint_mask_facts(P, Broken("J9", [], []))
    assert facts == {"ref": "J9", "allow_bridges": False}
    assert footprint_masks.keepout_name(facts) == "solder_mask_pads"


# --- Q22.16 (option (b)): pads cut back to their part outside a shared aperture


def pin_lines(text, image):
    start = text.index(f"(image {image}\n")
    end = text.index("\n    )", start)
    return [line.strip() for line in text[start:end].splitlines() if line.strip().startswith("(pin ")]


def test_a_pad_partly_inside_a_shared_aperture_is_cut_back_to_its_part_outside():
    """J1's fingers (KiCad 16..18 x 9..15 for A) go to the router as their
    part above the aperture (9..10.999, the aperture grown by Q22.18's 1 um):
    a 2 x 1.999 mm padstack with the pin's anchor at its centre (rounded to
    1 um), as the `.kicad_pcb` reader gives them
    (`crates/freerouting-kicad/tests/footprint_masks.rs`)."""
    out, _ = edge_graphics.apply(BASE, kicad_board.edge_graphics(fixture_board(), P))
    cut = "Rect[T]Pad_2000.000000x6000.000000_um_outside_mask_1"
    assert pin_lines(out, "l:edge2") == [f"(pin {cut} 1 -3000 1001)", f"(pin {cut} 2 3000 1001)"]
    assert (
        f"    (padstack {cut}\n      (shape (polygon F.Cu 0 1000 -1000 1000 999 -1000 999 -1000 -1000 1000 -1000))\n"
        "      (attach off)\n    )\n  )\n  (network" in out
    )
    assert pin_lines(out, "l:land") == ["(pin Rect[T]Pad_1000.000000x1000.000000_um 1 0 0)"]


def test_a_pad_wholly_inside_a_shared_aperture_is_named_in_the_log():
    base = BASE.replace(
        "(pin Rect[T]Pad_2000.000000x6000.000000_um 2 3000 -1000)",
        "(pin Rect[T]Pad_1000.000000x1000.000000_um 2 3000 -4000)",
    )
    assert base != BASE
    out, log = edge_graphics.apply(base, kicad_board.edge_graphics(fixture_board(), P))
    assert pin_lines(out, "l:edge2")[1] == "(pin Rect[T]Pad_1000.000000x1000.000000_um 2 3000 -4000)"
    named = [line for line in log if "lies inside" in line]
    assert len(named) == 1 and named[0].startswith("WARNING: pad J1-2 (net B) lies inside a solder-mask aperture")


def test_a_shared_image_is_copied_for_the_component_whose_pads_are_cut():
    base = BASE.replace(
        "      (place J1 20000 -11000 front 0 (PN x))\n",
        "      (place J1 20000 -11000 front 0 (PN x))\n      (place J2 8000 -24000 front 0 (PN x))\n",
    )
    assert base != BASE
    out, _ = edge_graphics.apply(base, kicad_board.edge_graphics(fixture_board(), P))
    assert (
        "    (component l:edge2\n      (place J2 8000 -24000 front 0 (PN x))\n    )\n"
        "    (component l:edge2_J1\n      (place J1 20000 -11000 front 0 (PN x))\n    )\n" in out
    )
    assert pin_lines(out, "l:edge2")[0] == "(pin Rect[T]Pad_2000.000000x6000.000000_um 1 -3000 -1000)"
    assert pin_lines(out, "l:edge2_J1")[0] == "(pin Rect[T]Pad_2000.000000x6000.000000_um_outside_mask_1 1 -3000 1001)"


def test_the_placement_frame_inverts_for_both_sides_and_flip_styles():
    import pad_trim

    for rotation in (0.0, 90.0, -270.0, 30.0):
        for mirrored in (False, True):
            for rotate_first in (False, True):
                frame = pad_trim.Frame((1000.0, -2000.0), rotation, mirrored, rotate_first)
                p = (123.0, -456.0)
                q = frame.to_image(frame.to_board(p))
                assert abs(q[0] - p[0]) < 1e-9 and abs(q[1] - p[1]) < 1e-9
    # the engine's convention: the back side mirrors x before the rotation
    back = pad_trim.Frame((0.0, 0.0), 90.0, True, False)
    assert back.to_board((1.0, 0.0)) == (0.0, -1.0)
