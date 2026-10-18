"""Reads what the DSN fix-ups of `dsn_fixups` need from the board, through
pcbnew, into plain dicts (K09).

The pcbnew module is passed in rather than imported, so this file imports
nothing outside the standard library and a fake can stand in for it in
tests. Call it on KiCad's UI thread only.

The values are in KiCad's frame and millimetres (`pcbnew.ToMM`, Y down), as
canary's `hardware/tools/dsn_export.py` computes them; `dsn_fixups` turns
them into DSN micrometres with Y negated, with the same arithmetic, so the
plugin writes the same numbers as that script.

`lock_tracks` and `restore_locks` serve the add-only second run (K09.6,
`fix_existing`): every track and via is locked in memory before the export,
so KiCad writes it as fixed copper and `ImportSpecctraSES` keeps it, and the
original lock states come back after the import. The board file is not
saved by either.
"""


def _mm(pcbnew, value):
    return pcbnew.ToMM(value)


def _layer_name(board, layer):
    return board.GetLayerName(layer)


def _chains(pcbnew, sps):
    """The rings (outlines, then their holes) of a SHAPE_POLY_SET, in mm."""
    out = []
    for i in range(sps.OutlineCount()):
        rings = [sps.Outline(i)] + [sps.Hole(i, h) for h in range(sps.HoleCount(i))]
        for ch in rings:
            out.append([(_mm(pcbnew, ch.CPoint(k).x), _mm(pcbnew, ch.CPoint(k).y)) for k in range(ch.PointCount())])
    return out


def _edge(board, pcbnew):
    """(edge clearance mm, outline bounding box (x0, y0, x1, y1) mm, outline
    rings mm), or (0, None, [])."""
    try:
        edge = _mm(pcbnew, board.GetDesignSettings().m_CopperEdgeClearance)
    except AttributeError:
        return 0.0, None, []
    if edge <= 0:
        return edge, None, []
    outline = pcbnew.SHAPE_POLY_SET()
    try:
        board.GetBoardPolygonOutlines(outline, False)  # centreline, which edge clearance is from
    except TypeError:
        board.GetBoardPolygonOutlines(outline)
    if outline.OutlineCount() == 0:
        return edge, None, []
    bb = outline.BBox()
    bbox = (_mm(pcbnew, bb.GetX()), _mm(pcbnew, bb.GetY()), _mm(pcbnew, bb.GetRight()), _mm(pcbnew, bb.GetBottom()))
    return edge, bbox, _chains(pcbnew, outline)


def _pads(board, pcbnew):
    """The pads with a local clearance of their own (fiducial mask openings)."""
    out = []
    outer = ((pcbnew.F_Cu, "F.Cu"), (pcbnew.B_Cu, "B.Cu"))
    for fp in board.GetFootprints():
        for pad in fp.Pads():
            local = pad.GetLocalClearance()
            if not local:  # None (KiCad 10: unset) or 0
                continue
            try:
                size = pad.GetSize(pcbnew.F_Cu)
            except TypeError:  # KiCad 9: one size
                size = pad.GetSize()
            pos = pad.GetPosition()
            out.append(
                {
                    "ref": fp.GetReference(),
                    "local_clearance_mm": _mm(pcbnew, local),
                    "size_mm": (_mm(pcbnew, size.x), _mm(pcbnew, size.y)),
                    "at_mm": (_mm(pcbnew, pos.x), _mm(pcbnew, pos.y)),
                    "layers": [_layer_name(board, lid) for lid, _ in outer if pad.IsOnLayer(lid)],
                }
            )
    return out


def _smd_pad_min(board, pcbnew):
    """{net: the smaller side of its narrowest SMD pad, mm}."""
    out = {}
    smd = getattr(pcbnew, "PAD_ATTRIB_SMD", None)
    for fp in board.GetFootprints():
        for pad in fp.Pads():
            net = pad.GetNetname()
            if not net or pad.GetAttribute() != smd:
                continue
            try:
                size = pad.GetSize(pcbnew.F_Cu)
            except TypeError:
                size = pad.GetSize()
            side = min(_mm(pcbnew, size.x), _mm(pcbnew, size.y))
            if side > 0:
                out[net] = min(out.get(net, side), side)
    return out


def _min_track(board, pcbnew):
    try:
        return _mm(pcbnew, board.GetDesignSettings().m_TrackMinWidth)
    except AttributeError:
        return 0.0


def _zones(board, pcbnew):
    """One dict per copper zone and copper layer: net, layer, whether the layer
    is outer, the zone priority and the fill (rings in mm)."""
    out = []
    outer_ids = (pcbnew.F_Cu, pcbnew.B_Cu)
    is_copper = getattr(pcbnew, "IsCopperLayer", lambda _layer: True)
    for zone in board.Zones():
        if zone.GetIsRuleArea():
            continue
        priority = zone.GetAssignedPriority() if hasattr(zone, "GetAssignedPriority") else zone.GetPriority()
        filled = bool(zone.IsFilled())
        for layer in zone.GetLayerSet().Seq():
            if not is_copper(layer):
                continue
            out.append(
                {
                    "net": zone.GetNetname(),
                    "layer": _layer_name(board, layer),
                    "outer": layer in outer_ids,
                    "priority": int(priority),
                    "filled": filled,
                    "fill": _chains(pcbnew, zone.GetFilledPolysList(layer)) if filled else [],
                }
            )
    return out


def collect(board, pcbnew):
    """The board facts `dsn_fixups.apply` reads (see its module docs)."""
    edge, bbox, rings = _edge(board, pcbnew)
    return {
        "edge_clearance_mm": edge,
        "outline_bbox_mm": bbox,
        "outline_rings_mm": rings,
        "pads": _pads(board, pcbnew),
        "zones": _zones(board, pcbnew),
        "smd_pad_min_mm": _smd_pad_min(board, pcbnew),
        "min_track_mm": _min_track(board, pcbnew),
    }


def lock_tracks(board):
    """Locks every unlocked track and via in memory; returns them so
    `restore_locks` can unlock them again."""
    changed = []
    for track in board.GetTracks():
        if not track.IsLocked():
            track.SetLocked(True)
            changed.append(track)
    return changed


def restore_locks(tracks):
    for track in tracks:
        track.SetLocked(False)


def fix_via_drills(board, session_vias, tolerance_nm=50):
    """Sets the drill of each unlocked via that sits on a session via of the
    same net (`ses_vias.vias`) to the session's drill, where KiCad's import
    gave it the net class's. Returns the number of vias changed."""
    by_net = {}
    for net, x, y, _dia, drill in session_vias:
        by_net.setdefault(net, []).append((x, y, drill))
    changed = 0
    for via in board.GetTracks():
        if via.GetClass() != "PCB_VIA" or via.IsLocked():
            continue
        pos = via.GetPosition()
        for x, y, drill in by_net.get(via.GetNetname(), ()):
            if abs(pos.x - x) <= tolerance_nm and abs(pos.y - y) <= tolerance_nm:
                if via.GetDrill() != drill:
                    via.SetDrill(drill)
                    changed += 1
                break
    return changed
