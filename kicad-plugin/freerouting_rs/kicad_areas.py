"""Reads the board's rule areas through pcbnew into the plain dicts that
`rule_areas.rewrite` takes.

The pcbnew module is passed in rather than imported, so this file imports
nothing outside the standard library and a fake can stand in for it in
tests. Call it on KiCad's UI thread only.

Accessor names (verified by probe on KiCad 10.0.7, see kicad-plugin.md):
`GetIsRuleArea`, `GetDoNotAllowTracks`, `GetDoNotAllowVias`,
`GetDoNotAllowPads`, `GetDoNotAllowFootprints`, and for the copper pour
`GetDoNotAllowZoneFills` (10.0) or `GetDoNotAllowCopperPour` (9.0).
"""

NM_PER_UM = 1000.0


def _ring(chain):
    # DSN frame: um, Y negated, no offset (KiCad's export frame).
    return [(chain.CPoint(i).x / NM_PER_UM, -chain.CPoint(i).y / NM_PER_UM) for i in range(chain.PointCount())]


def _pour_flag(zone):
    for name in ("GetDoNotAllowZoneFills", "GetDoNotAllowCopperPour"):
        getter = getattr(zone, name, None)
        if getter is not None:
            return bool(getter())
    return False


def collect(board, pcbnew):
    """One dict per outline of every rule area on the board (keys: see
    `rule_areas`). Footprint-owned areas carry `footprint` (the reference)."""
    areas = []
    is_copper = getattr(pcbnew, "IsCopperLayer", lambda _layer: True)
    for zone in board.Zones():
        if not zone.GetIsRuleArea():
            continue
        parent = zone.GetParentFootprint() if hasattr(zone, "GetParentFootprint") else None
        layers = [board.GetLayerName(layer) for layer in zone.GetLayerSet().Seq() if is_copper(layer)]
        flags = {
            "tracks": bool(zone.GetDoNotAllowTracks()),
            "vias": bool(zone.GetDoNotAllowVias()),
            "pads": bool(zone.GetDoNotAllowPads()),
            "footprints": bool(zone.GetDoNotAllowFootprints()),
            "copper_pour": _pour_flag(zone),
        }
        outlines = zone.Outline()
        for k in range(outlines.OutlineCount()):
            area = {
                "name": zone.GetZoneName(),
                "layers": layers,
                "outline": _ring(outlines.Outline(k)),
                "holes": [_ring(outlines.Hole(k, h)) for h in range(outlines.HoleCount(k))],
                **flags,
            }
            if parent is not None:
                area["footprint"] = parent.GetReference()
            areas.append(area)
    return areas
