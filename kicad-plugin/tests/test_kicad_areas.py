import kicad_areas


class Pt:
    def __init__(self, x, y):
        self.x, self.y = x, y


class Chain:
    def __init__(self, pts):
        self.pts = pts

    def PointCount(self):
        return len(self.pts)

    def CPoint(self, i):
        return Pt(*self.pts[i])


class Outlines:
    def __init__(self, outer, holes=()):
        self.outer, self.holes = outer, holes

    def OutlineCount(self):
        return 1

    def Outline(self, k):
        return Chain(self.outer)

    def HoleCount(self, k):
        return len(self.holes)

    def Hole(self, k, h):
        return Chain(self.holes[h])


class LayerSet:
    def __init__(self, layers):
        self.layers = layers

    def Seq(self):
        return self.layers


class Zone:
    rule = True
    owner = None

    def __init__(self, pour_getter, **flags):
        self.flags = dict(dict(tracks=False, vias=False, pads=False, footprints=False, pour=False), **flags)
        if pour_getter:
            setattr(self, pour_getter, lambda: self.flags["pour"])

    def GetIsRuleArea(self):
        return self.rule

    def GetZoneName(self):
        return "zone"

    def GetLayerSet(self):
        return LayerSet([0, 31, 44])  # F.Cu, B.Cu, Edge.Cuts

    def Outline(self):
        return Outlines([(136500000, 53000000), (162500000, 53000000), (162500000, 79000000)], [[(1, 2)]])

    def GetDoNotAllowTracks(self):
        return self.flags["tracks"]

    def GetDoNotAllowVias(self):
        return self.flags["vias"]

    def GetDoNotAllowPads(self):
        return self.flags["pads"]

    def GetDoNotAllowFootprints(self):
        return self.flags["footprints"]

    def GetParentFootprint(self):
        return self.owner


class Footprint:
    def GetReference(self):
        return "U7"


class Board:
    def __init__(self, zones):
        self.zones = zones

    def Zones(self):
        return self.zones

    def GetLayerName(self, layer):
        return {0: "F.Cu", 31: "B.Cu", 44: "Edge.Cuts"}[layer]


class Pcbnew:
    @staticmethod
    def IsCopperLayer(layer):
        return layer in (0, 31)


def test_collect_converts_to_dsn_frame_and_filters_non_copper():
    z = Zone("GetDoNotAllowZoneFills", pads=True, footprints=True, pour=True)
    plain = Zone(None)
    plain.rule = False
    (a,) = kicad_areas.collect(Board([z, plain]), Pcbnew)
    assert a["layers"] == ["F.Cu", "B.Cu"]
    assert a["outline"] == [(136500.0, -53000.0), (162500.0, -53000.0), (162500.0, -79000.0)]
    assert a["holes"] == [[(0.001, -0.002)]]
    assert (a["tracks"], a["vias"], a["pads"], a["footprints"], a["copper_pour"]) == (False, False, True, True, True)
    assert "footprint" not in a


def test_pour_flag_falls_back_to_kicad9_name():
    z = Zone("GetDoNotAllowCopperPour", pour=True)
    assert kicad_areas.collect(Board([z]), Pcbnew)[0]["copper_pour"] is True
    assert kicad_areas.collect(Board([Zone(None)]), Pcbnew)[0]["copper_pour"] is False


def test_footprint_owned_area_carries_reference():
    z = Zone(None)
    z.owner = Footprint()
    assert kicad_areas.collect(Board([z]), Pcbnew)[0]["footprint"] == "U7"
