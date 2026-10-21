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


def _pt(pcbnew, p):
    return (_mm(pcbnew, p.x), _mm(pcbnew, p.y))


def _filled(item):
    """Whether a PCB_SHAPE is filled: `IsFilled` up to KiCad 9, `IsAnyFill`
    (solid or hatched) from KiCad 10."""
    for name in ("IsFilled", "IsAnyFill", "IsSolidFill"):
        method = getattr(item, name, None)
        if method is not None:
            return bool(method())
    return False


def _edge_shapes(pcbnew, item, layer):
    """The `edge_graphics` dicts of one PCB_SHAPE (a polygon gives one per ring)."""
    kind = item.GetShape()
    width = _mm(pcbnew, item.GetWidth())
    filled = _filled(item)

    def mk(name, points):
        return {"layer": layer, "width_mm": width, "kind": name, "points": points, "filled": filled}

    if kind == pcbnew.SHAPE_T_SEGMENT:
        return [mk("polyline", [_pt(pcbnew, item.GetStart()), _pt(pcbnew, item.GetEnd())])]
    if kind == getattr(pcbnew, "SHAPE_T_RECTANGLE", getattr(pcbnew, "SHAPE_T_RECT", None)):
        return [mk("ring", [_pt(pcbnew, c) for c in item.GetRectCorners()])]
    if kind == pcbnew.SHAPE_T_ARC:
        return [mk("arc", [_pt(pcbnew, item.GetStart()), _pt(pcbnew, item.GetArcMid()), _pt(pcbnew, item.GetEnd())])]
    if kind == pcbnew.SHAPE_T_CIRCLE:
        x, y = _pt(pcbnew, item.GetCenter())
        return [mk("circle", [(x, y), (x + _mm(pcbnew, item.GetRadius()), y)])]
    if kind == pcbnew.SHAPE_T_POLY:
        return [mk("ring", ring) for ring in _chains(pcbnew, item.GetPolyShape()) if ring]
    if kind == pcbnew.SHAPE_T_BEZIER:
        rebuild = getattr(item, "RebuildBezierToSegmentsPointsList", None)
        if rebuild is not None:
            try:
                rebuild(5000)  # 5 um, edge_graphics.SAGITTA_MM
            except TypeError:  # an older pcbnew takes the width
                rebuild(item.GetWidth())
        return [mk("polyline", [_pt(pcbnew, p) for p in item.GetBezierPoints()])]
    return []


def edge_graphics(board, pcbnew):
    """Q18.5: what `edge_graphics.apply` reads. The edge clearance
    (`m_CopperEdgeClearance`, mm), the outer contours of the outline KiCad's
    DSN export writes (`GetBoardPolygonOutlines` with the bounding box
    inferred, as the export does), and every Edge.Cuts and Margin shape of the
    board and its footprints, in board order then footprint order, and every
    F.Mask and B.Mask shape of the board itself (Q18.7)."""
    edge = _mm(pcbnew, board.GetDesignSettings().m_CopperEdgeClearance)
    outline = pcbnew.SHAPE_POLY_SET()
    try:
        board.GetBoardPolygonOutlines(outline, True)
    except TypeError:  # an older pcbnew
        board.GetBoardPolygonOutlines(outline)
    rings = [
        [_pt(pcbnew, ch.CPoint(k)) for k in range(ch.PointCount())]
        for ch in (outline.Outline(i) for i in range(outline.OutlineCount()))
    ]
    layers = ((pcbnew.Edge_Cuts, "Edge.Cuts"), (pcbnew.Margin, "Margin"))
    # Q18.7: the board's own solder-mask shapes (apertures no pad owns)
    masks = ((getattr(pcbnew, "F_Mask", None), "F.Mask"), (getattr(pcbnew, "B_Mask", None), "B.Mask"))
    items = [(item, True) for item in board.GetDrawings()]
    for fp in board.GetFootprints():
        items += [(item, False) for item in fp.GraphicalItems()]
    shapes = []
    for item, on_board in items:
        item = item.Cast() if hasattr(item, "Cast") else item
        if not hasattr(item, "GetShape"):  # texts
            continue
        layer = item.GetLayer()
        name = next((n for lid, n in layers if layer == lid), None)
        if name is None and on_board:
            name = next((n for lid, n in masks if lid is not None and layer == lid), None)
        if name is not None:
            shapes += _edge_shapes(pcbnew, item, name)
    return {"edge_clearance_mm": edge, "outline_mm": rings, "shapes": shapes, "mask_to_copper_mm": _mm(pcbnew, mask_to_copper_nm(board))}


def _cast(item):
    """The item's own class; a PCB_FIELD (which `Cast` refuses) as it is."""
    try:
        return item.Cast() if hasattr(item, "Cast") else item
    except TypeError:
        return item


def _text_polygons(pcbnew, item, max_error):
    """Q22.4: the outer outlines (mm) of a text's copper: KiCad's own glyph
    strokes (`TransformTextToPolySet`), or the box `TransformShapeToPolygon`
    gives a knockout text or a text box."""
    sps = pcbnew.SHAPE_POLY_SET()
    knockout = bool(getattr(item, "IsKnockout", lambda: False)())
    if knockout or hasattr(item, "GetShape") or not hasattr(item, "TransformTextToPolySet"):
        item.TransformShapeToPolygon(sps, item.GetLayer(), 0, max_error, pcbnew.ERROR_OUTSIDE)
    else:
        item.TransformTextToPolySet(sps, 0, max_error, pcbnew.ERROR_OUTSIDE)
    if hasattr(sps, "Simplify"):
        try:
            sps.Simplify()
        except TypeError:  # KiCad 9 takes the mode
            sps.Simplify(pcbnew.SHAPE_POLY_SET.PM_FAST)
    out = []
    for i in range(sps.OutlineCount()):
        ch = sps.Outline(i)
        out.append([_pt(pcbnew, ch.CPoint(k)) for k in range(ch.PointCount())])
    return out


def copper_graphics(board, pcbnew):
    """Q22.4: what `copper_graphics.apply` reads. Every shape on a copper layer
    of the board and its footprints (the `edge_graphics` dicts plus `net`:
    its net name, empty for none or in a net-tie footprint), every visible
    text on a copper layer (`layer`, `polygons`), in board order then
    footprint order, and the number of net-tie shapes."""
    is_copper = getattr(pcbnew, "IsCopperLayer", None)
    try:
        max_error = board.GetDesignSettings().m_MaxError
    except AttributeError:
        max_error = 5000
    items = [(item, False) for item in board.GetDrawings()]
    for fp in board.GetFootprints():
        tie = bool(getattr(fp, "IsNetTie", lambda: False)())
        fields = list(fp.GetFields()) if hasattr(fp, "GetFields") else []
        items += [(item, tie) for item in list(fp.GraphicalItems()) + fields]
    shapes, texts, ties = [], [], 0
    for item, tie in items:
        item = _cast(item)
        layer = item.GetLayer()
        if is_copper is not None and not is_copper(layer):
            continue
        name = _layer_name(board, layer)
        if is_copper is None and not name.endswith(".Cu"):
            continue
        if hasattr(item, "GetText"):  # PCB_TEXT, PCB_FIELD, PCB_TEXTBOX
            if hasattr(item, "IsVisible") and not item.IsVisible():
                continue
            texts.append({"layer": name, "polygons": _text_polygons(pcbnew, item, max_error)})
        elif hasattr(item, "GetShape"):
            net = "" if tie else (item.GetNetname() if hasattr(item, "GetNetname") else "")
            ties += 1 if tie else 0
            for shape in _edge_shapes(pcbnew, item, name):
                shape["net"] = net
                shapes.append(shape)
    return {"shapes": shapes, "texts": texts, "net_tie_shapes": ties}


def _expansion(pad, layer):
    try:
        return pad.GetSolderMaskExpansion(layer)
    except TypeError:  # KiCad 8: one expansion for the pad
        return pad.GetSolderMaskExpansion()


def mask_to_copper_nm(board):
    """Q22.12: the board's `m_SolderMaskToCopperClearance` (nm, at least 0; 0
    when this pcbnew has none, KiCad's default)."""
    value = getattr(board.GetDesignSettings(), "m_SolderMaskToCopperClearance", 0) or 0
    return max(int(value), 0)


def mask_expansions(board, pcbnew, floor_mm, clearance_nm=0):
    """Q18.7: the pads whose solder-mask aperture reaches past `floor_mm` (the
    smallest clearance the router keeps, `Dsn.min_clearance_mm`), as `{"ref",
    "pin", "front_mm", "back_mm"}` in footprint and pad order (0 for a side
    without an aperture or an expansion within the floor). `pin` is the DSN pin name of KiCad's export: the pad number,
    `<number>@<n>` for its n-th repeat; non-plated holes are not pins.
    Q22.12: a side with an aperture expands by `GetSolderMaskExpansion` +
    `clearance_nm` (the board's `m_SolderMaskToCopperClearance`, which KiCad's
    DRC keeps other nets' copper from the aperture), as the `.kicad_pcb`
    reader's `mask::expansion` does; 0 leaves the expansions as they were."""
    npth = getattr(pcbnew, "PAD_ATTRIB_NPTH", None)
    sides = ((pcbnew.F_Cu, pcbnew.F_Mask), (pcbnew.B_Cu, pcbnew.B_Mask))
    out = []
    for fp in board.GetFootprints():
        seen = {}
        for pad in fp.Pads():
            if pad.GetAttribute() == npth or not any(pad.IsOnLayer(cu) for cu, _ in sides):
                continue
            number = pad.GetNumber()
            if number and number not in seen:
                seen[number] = 0
                pin = number
            else:
                seen[number] = seen.get(number, 0) + 1
                pin = f"{number}@{seen[number]}"
            per_side = []
            for cu, mask in sides:
                e = _expansion(pad, cu) + clearance_nm if pad.IsOnLayer(cu) and pad.IsOnLayer(mask) else 0
                per_side.append(_mm(pcbnew, e) if _mm(pcbnew, e) > floor_mm else 0.0)
            if any(per_side):
                out.append({"ref": fp.GetReference(), "pin": pin, "front_mm": per_side[0], "back_mm": per_side[1]})
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
