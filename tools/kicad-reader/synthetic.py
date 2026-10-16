"""K05.8: generate the synthetic test boards of fixtures/kicad/synthetic/ with pcbnew.

    python3 -I tools/kicad-reader/synthetic.py OUT_DIR [BOARD ...]

Run under a KiCad interpreter (WSL: the system python3, KiCad 10.0.7). Writes
pads.kicad_pcb, outline.kicad_pcb, backside.kicad_pcb, classes.kicad_pcb and
classes6.kicad_pcb with their .kicad_pro files (K05), and rule-areas.kicad_pcb, arcs.kicad_pcb and
vias.kicad_pcb (K06). Name boards on the command line to write only those: the files hold random
UUIDs, so regenerating a committed board changes it. The exports next to them
(`*.kicad10.dsn`) come from tools/kicad-reader/export.sh. Coordinates are
integer nanometres (pcbnew.FromMM truncates).
"""

import json
import os
import sys

import pcbnew

V2 = pcbnew.VECTOR2I
CHAMFER_TOP_LEFT, CHAMFER_TOP_RIGHT, CHAMFER_BOTTOM_LEFT, CHAMFER_BOTTOM_RIGHT = 1, 2, 4, 8


def nm(mm):
    return int(round(mm * 1e6))


def pt(x, y):
    return V2(nm(x), nm(y))


def new_board(layers=2):
    b = pcbnew.BOARD()
    b.SetCopperLayerCount(layers)
    return b


def add_net(b, name):
    n = pcbnew.NETINFO_ITEM(b, name)
    b.Add(n)
    return n


def shape(b, kind, layer=pcbnew.Edge_Cuts, width=0.1, parent=None):
    s = pcbnew.PCB_SHAPE(parent if parent is not None else b)
    s.SetShape(kind)
    s.SetLayer(layer)
    s.SetWidth(nm(width))
    return s


def line(b, x1, y1, x2, y2, **kw):
    s = shape(b, pcbnew.SHAPE_T_SEGMENT, **kw)
    s.SetStart(pt(x1, y1))
    s.SetEnd(pt(x2, y2))
    return s


def arc(b, start, mid, end, **kw):
    s = shape(b, pcbnew.SHAPE_T_ARC, **kw)
    s.SetArcGeometry(pt(*start), pt(*mid), pt(*end))
    return s


def footprint(b, ref, fpid, x, y, angle=0.0, value="V"):
    fp = pcbnew.FOOTPRINT(b)
    fp.SetFPIDAsString(fpid)
    fp.SetReference(ref)
    fp.SetValue(value)
    fp.SetPosition(pt(x, y))
    fp.SetOrientationDegrees(angle)
    return fp


def pad(fp, number, kind, attr, x, y, w, h, angle=0.0, drill=None, net=None, layers=None):
    p = pcbnew.PAD(fp)
    p.SetNumber(number)
    p.SetAttribute(attr)
    p.SetShape(kind)
    p.SetSize(V2(nm(w), nm(h)))
    p.SetFPRelativePosition(pt(x, y))
    p.SetOrientationDegrees(angle + fp.GetOrientationDegrees())
    if drill:
        p.SetDrillSize(V2(nm(drill), nm(drill)))
    if layers is None:
        layers = pcbnew.LSET()
        if attr == pcbnew.PAD_ATTRIB_SMD:
            for l in (pcbnew.F_Cu, pcbnew.F_Paste, pcbnew.F_Mask):
                layers.AddLayer(l)
        elif attr == pcbnew.PAD_ATTRIB_NPTH:
            for l in (pcbnew.F_Mask, pcbnew.B_Mask):
                layers.AddLayer(l)
        else:
            layers = pcbnew.LSET.AllCuMask().AddLayer(pcbnew.F_Mask).AddLayer(pcbnew.B_Mask)
    p.SetLayerSet(layers)
    if net is not None:
        p.SetNet(net)
    fp.Add(p)
    return p


def courtyard(b, fp, x0, y0, x1, y1):
    """A rectangle on F.CrtYd, in absolute board coordinates of an unrotated footprint at the origin."""
    for (a, c, d, e) in ((x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)):
        s = shape(b, pcbnew.SHAPE_T_SEGMENT, layer=pcbnew.F_CrtYd, width=0.05, parent=fp)
        s.SetStart(pt(a, c))
        s.SetEnd(pt(d, e))
        fp.Add(s)


def outline_rect(b, x0, y0, x1, y1):
    for (a, c, d, e) in ((x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)):
        b.Add(line(b, a, c, d, e))


def pads_board(path):
    b = new_board(2)
    n1, n2, n3 = add_net(b, "N1"), add_net(b, "N2"), add_net(b, "N3")
    for ref, x, y, ang in (("U1", 50.0, 50.0, 0.0), ("U2", 50.0, 80.0, 90.0), ("U3", 50.0, 110.0, 30.0)):
        fp = footprint(b, ref, "syn:Pads", x, y, ang, "pads")
        smd, pth, npth = pcbnew.PAD_ATTRIB_SMD, pcbnew.PAD_ATTRIB_PTH, pcbnew.PAD_ATTRIB_NPTH
        pad(fp, "1", pcbnew.PAD_SHAPE_CIRCLE, pth, 0, 0, 1.6, 1.6, drill=0.8, net=n1)
        pad(fp, "2", pcbnew.PAD_SHAPE_RECT, smd, 3, 0, 2.0, 1.0, net=n2)
        p = pad(fp, "3", pcbnew.PAD_SHAPE_OVAL, smd, 6, 0, 2.0, 1.0, angle=30.0, net=n3)
        p = pad(fp, "4", pcbnew.PAD_SHAPE_TRAPEZOID, smd, 9, 0, 2.0, 1.2, angle=90.0, net=n1)
        p.SetDelta(V2(0, nm(0.4)))
        p = pad(fp, "5", pcbnew.PAD_SHAPE_ROUNDRECT, smd, 12, 0, 2.0, 1.0, net=n2)
        p.SetRoundRectRadiusRatio(0.25)
        p.SetOffset(V2(nm(0.3), nm(0.2)))
        p = pad(fp, "6", pcbnew.PAD_SHAPE_CHAMFERED_RECT, smd, 15, 0, 2.0, 1.5, angle=30.0, net=n3)
        p.SetRoundRectRadiusRatio(0.0)
        p.SetChamferRectRatio(0.25)
        p.SetChamferPositions(CHAMFER_TOP_LEFT | CHAMFER_BOTTOM_RIGHT)
        p = pad(fp, "7", pcbnew.PAD_SHAPE_CUSTOM, smd, 19, 0, 1.0, 1.0, net=n1)
        p.SetAnchorPadShape(pcbnew.F_Cu, pcbnew.PAD_SHAPE_CIRCLE)
        poly = pcbnew.PCB_SHAPE(fp, pcbnew.SHAPE_T_POLY)
        poly.SetFilled(True)
        poly.SetWidth(0)
        ch = pcbnew.SHAPE_LINE_CHAIN()
        for (a, c) in ((0.5, -0.5), (2.0, -0.4), (2.0, 0.8), (0.4, 0.5)):
            ch.Append(nm(a), nm(c))
        ch.SetClosed(True)
        poly.SetPolyShape(pcbnew.SHAPE_POLY_SET(ch))
        p.AddPrimitiveShape(pcbnew.F_Cu, poly)
        seg = pcbnew.PCB_SHAPE(fp, pcbnew.SHAPE_T_SEGMENT)
        seg.SetStart(V2(0, 0))
        seg.SetEnd(V2(nm(-1.5), nm(1.0)))
        seg.SetWidth(nm(0.4))
        p.AddPrimitiveShape(pcbnew.F_Cu, seg)
        pad(fp, "", pcbnew.PAD_SHAPE_CIRCLE, npth, 23, 0, 3.0, 3.0, drill=3.0)
        pad(fp, "1", pcbnew.PAD_SHAPE_RECT, smd, 26, 0, 1.0, 1.0, net=n2)  # duplicate number
        courtyard(b, fp, -2, -3, 28, 3)
        b.Add(fp)
    outline_rect(b, 30, 30, 100, 130)
    pcbnew.SaveBoard(path, b)


def outline_board(path):
    b = new_board(2)
    n1 = add_net(b, "N1")
    fp = footprint(b, "U1", "syn:Two", 60.0, 50.0, 0.0)
    pad(fp, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 0, 1.5, 1.5, net=n1)
    pad(fp, "2", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 4, 0, 1.5, 1.5, net=n1)
    b.Add(fp)
    # a concave, notched outline: an L with a notch and a rounded corner
    pts = [(20, 20), (100, 20), (100, 60), (80, 60), (80, 50), (60, 50)]
    # (the polygon below is closed by an arc and lines)
    b.Add(line(b, 20, 20, 100, 20))
    b.Add(line(b, 100, 20, 100, 60))
    b.Add(line(b, 100, 60, 80, 60))
    b.Add(line(b, 80, 60, 80, 45))      # notch
    b.Add(line(b, 80, 45, 70, 45))
    b.Add(line(b, 70, 45, 70, 60))
    b.Add(line(b, 70, 60, 30, 60))
    b.Add(arc(b, (30, 60), (22.929, 57.071), (20, 50)))
    b.Add(line(b, 20, 50, 20, 20))
    # a slot (inner contour): two lines and two arcs
    b.Add(line(b, 40, 30, 60, 30))
    b.Add(arc(b, (60, 30), (62, 32), (60, 34)))
    b.Add(line(b, 60, 34, 40, 34))
    b.Add(arc(b, (40, 34), (38, 32), (40, 30)))
    pcbnew.SaveBoard(path, b)


def backside_board(path):
    b = new_board(4)
    n1, n2 = add_net(b, "N1"), add_net(b, "N2")
    specs = [("B1", 40.0, 40.0, 0.0, True), ("B2", 40.0, 70.0, 30.0, True),
             ("B3", 40.0, 100.0, 90.0, True), ("B4", 40.0, 130.0, 225.0, True),
             ("F1", 80.0, 70.0, 30.0, False), ("F2", 80.0, 100.0, 225.0, False)]
    for ref, x, y, ang, back in specs:
        fp = footprint(b, ref, "syn:Part", x, y, 0.0, "part")
        pad(fp, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, -2, 0.5, 1.6, 1.0, angle=0.0, net=n1)
        p = pad(fp, "2", pcbnew.PAD_SHAPE_ROUNDRECT, pcbnew.PAD_ATTRIB_SMD, 2, -0.5, 1.6, 1.0, angle=30.0, net=n2)
        p.SetRoundRectRadiusRatio(0.25)
        p = pad(fp, "3", pcbnew.PAD_SHAPE_CHAMFERED_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 2, 1.6, 1.2, net=n1)
        p.SetRoundRectRadiusRatio(0.0)
        p.SetChamferRectRatio(0.25)
        p.SetChamferPositions(CHAMFER_TOP_LEFT)
        p = pad(fp, "4", pcbnew.PAD_SHAPE_TRAPEZOID, pcbnew.PAD_ATTRIB_SMD, 0, -2, 1.6, 1.2, net=n2)
        p.SetDelta(V2(0, nm(0.3)))
        pad(fp, "5", pcbnew.PAD_SHAPE_CIRCLE, pcbnew.PAD_ATTRIB_PTH, 4, 2, 1.4, 1.4, drill=0.7, net=n1)
        s = shape(b, pcbnew.SHAPE_T_SEGMENT, layer=pcbnew.F_SilkS, width=0.12, parent=fp)
        s.SetStart(pt(-3, -3))
        s.SetEnd(pt(3, -3))
        fp.Add(s)
        a = shape(b, pcbnew.SHAPE_T_ARC, layer=pcbnew.F_Fab, width=0.1, parent=fp)
        a.SetArcGeometry(pt(-3, 3), pt(0, 4), pt(3, 3))
        fp.Add(a)
        courtyard(b, fp, -4, -4, 5, 4)
        b.Add(fp)
        if ang != 0.0:
            fp.SetOrientationDegrees(ang)
        if back:
            fp.Flip(fp.GetPosition(), False)
    outline_rect(b, 20, 20, 100, 150)
    pcbnew.SaveBoard(path, b)


def classes_board(path, schema):
    b = new_board(2)
    nets = {n: add_net(b, n) for n in ("A1", "A2", "B", "C", "D", "/sig1", "E")}
    fp = footprint(b, "U1", "syn:Row", 50.0, 50.0, 0.0)
    for i, n in enumerate(nets.values()):
        pad(fp, str(i + 1), pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 3 * i, 0, 1.5, 1.5, net=n)
        pad(fp, str(i + 20), pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 3 * i, 3, 1.5, 1.5, net=n)
    b.Add(fp)
    outline_rect(b, 40, 40, 80, 70)
    pcbnew.SaveBoard(path, b)

    def cls(name, prio, clr, trk, via, drill, full=True):
        c = {"name": name, "priority": prio, "clearance": clr, "track_width": trk,
             "via_diameter": via, "via_drill": drill, "microvia_diameter": 0.3, "microvia_drill": 0.1,
             "diff_pair_gap": 0.25, "diff_pair_width": 0.2, "line_style": 0,
             "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)",
             "tuning_profile": ""}
        if full:
            c["bus_width"] = 12
            c["wire_width"] = 6
        return c

    default = cls("Default", 2147483647, 0.2, 0.25, 0.8, 0.4)
    fast = cls("Fast", 0, 0.15, 0.1, 0.5, 0.25, full=True)
    wide = cls("Wide", 1, 0.3, 0.6, 1.0, 0.5, full=False)     # lacks wire/bus width -> aggregate with Default
    if schema == 5:
        ns = {"classes": [default, fast, wide], "meta": {"version": 5}, "net_colors": None,
              "netclass_assignments": {"B": ["Wide"], "C": ["Fast"]},
              "netclass_patterns": [{"netclass": "Fast", "pattern": "A*"},
                                    {"netclass": "Wide", "pattern": "C"},
                                    {"netclass": "Wide", "pattern": "(D|E)"}]}
    else:
        fast["nets"] = ["A*", "C"]
        wide["nets"] = ["/sig1"]
        for c in (default, fast, wide):
            c.pop("priority")
            c.pop("tuning_profile")
        ns = {"classes": [default, fast, wide], "meta": {"version": 2}, "net_colors": None,
              "netclass_assignments": {"B": "Wide"}}
    pro = {"meta": {"filename": os.path.basename(path)[:-10] + ".kicad_pro", "version": 3 if schema == 5 else 1},
           "net_settings": ns}
    with open(path[:-10] + ".kicad_pro", "w") as f:
        json.dump(pro, f, indent=2)


# ---- K06 ---------------------------------------------------------------------------------------

def lset(*layers):
    s = pcbnew.LSET()
    for l in layers:
        s.AddLayer(l)
    return s


def chain(points):
    c = pcbnew.SHAPE_LINE_CHAIN()
    for x, y in points:
        c.Append(nm(x), nm(y))
    c.SetClosed(True)
    return c


def rect_pts(x, y, w, h):
    return [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]


def make_zone(parent, layers, pts, holes=(), flags=None, net=None, placement=False, name=None):
    """A zone. flags=None: a copper zone; else a rule area with the given
    (tracks, vias, pads, copperpour, footprints) bans."""
    z = pcbnew.ZONE(parent)
    z.SetLayerSet(layers)
    z.Outline().AddOutline(chain(pts))
    for h in holes:
        z.Outline().AddHole(chain(h))
    if flags is None:
        z.SetIsRuleArea(False)
        if net is not None:
            z.SetNet(net)
    else:
        tracks, vias, pads, pour, footprints = flags
        z.SetIsRuleArea(True)
        z.SetDoNotAllowTracks(tracks)
        z.SetDoNotAllowVias(vias)
        z.SetDoNotAllowPads(pads)
        z.SetDoNotAllowZoneFills(pour)
        z.SetDoNotAllowFootprints(footprints)
        if placement:
            z.SetPlacementAreaEnabled(True)
            z.SetPlacementAreaSource("sheet")
    if name:
        z.SetZoneName(name)
    return z


def rule_areas_board(path):
    b = new_board(4)
    n1, n2 = add_net(b, "N1"), add_net(b, "N2")
    fp = footprint(b, "U1", "syn:Two", 100.0, 125.0, 0.0)
    pad(fp, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 0, 1.5, 1.5, net=n1)
    pad(fp, "2", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 4, 0, 1.5, 1.5, net=n2)
    b.Add(fp)
    F, B, I1, I2 = pcbnew.F_Cu, pcbnew.B_Cu, pcbnew.In1_Cu, pcbnew.In2_Cu
    # flags: (tracks, vias, pads, copper pour, footprints)
    areas = [
        ("tracks-vias", lset(F), (1, 1, 0, 0, 0)),
        ("vias", lset(F, B), (0, 1, 0, 0, 0)),
        ("tracks", lset(F), (1, 0, 0, 0, 0)),
        ("footprints", lset(F), (0, 0, 0, 0, 1)),
        ("pads", lset(B), (0, 0, 1, 0, 0)),
        ("pour", lset(F), (0, 0, 0, 1, 0)),
        ("placement", lset(F), (0, 0, 0, 0, 0)),
        ("tracks-footprints", lset(F), (1, 0, 0, 0, 1)),
        ("pads-footprints", lset(B), (0, 0, 1, 0, 1)),
        ("all-copper", pcbnew.LSET.AllCuMask(4), (1, 1, 0, 0, 0)),
        ("f-and-b", lset(F, B), (1, 1, 0, 0, 0)),
        ("hole", lset(F), (1, 1, 0, 0, 0)),
        ("inner", lset(I2, I1), (0, 1, 0, 0, 0)),
    ]
    for i, (name, layers, flags) in enumerate(areas):
        x, y = 20 + 15 * (i % 10), 20 + 20 * (i // 10)
        holes = [[(x + 3, y + 3), (x + 6, y + 3), (x + 6, y + 6), (x + 3, y + 6)]] if name == "hole" else []
        z = make_zone(b, layers, rect_pts(x, y, 10, 10), holes, flags=[bool(f) for f in flags],
                      placement=(name == "placement"), name=name)
        b.Add(z)
    # copper zones: nets, two layers, and netless ones (@:no_net_N)
    b.Add(make_zone(b, lset(F), rect_pts(20, 70, 20, 10), net=n1))
    b.Add(make_zone(b, lset(F, B), rect_pts(50, 70, 20, 10), [[(55, 73), (60, 73), (60, 77), (55, 77)]], net=n2))
    b.Add(make_zone(b, lset(I1), rect_pts(80, 70, 20, 10)))
    b.Add(make_zone(b, lset(I2, I1), rect_pts(110, 70, 20, 10)))
    # footprints that own rule areas, at 0, 30 and 135 degrees on both sides
    specs = [("A0", 30.0, 100.0, 0.0, False, lset(F), (1, 1, 0, 0, 0), False),
             ("A30", 60.0, 100.0, 30.0, False, lset(F, B), (0, 1, 0, 0, 1), True),
             ("A135", 90.0, 100.0, 135.0, False, lset(I1, F), (1, 0, 0, 0, 0), False),
             ("B0", 30.0, 125.0, 0.0, True, lset(F), (1, 1, 0, 0, 0), False),
             ("B30", 60.0, 125.0, 30.0, True, lset(F, B), (0, 1, 0, 0, 1), True),
             ("B135", 90.0, 125.0, 135.0, True, lset(I1, F), (1, 0, 0, 0, 0), False)]
    for ref, x, y, ang, back, layers, flags, hole in specs:
        f = footprint(b, ref, "syn:Area", x, y, 0.0, "area")
        pad(f, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 0, 1.5, 1.5, net=n1)
        holes = [[(x + 4.5, y - 1), (x + 6.5, y - 1), (x + 6.5, y + 1)]] if hole else []
        f.Add(make_zone(f, layers, rect_pts(x + 3, y - 4, 6, 8), holes, flags=[bool(v) for v in flags],
                        name="fp-" + ref))
        b.Add(f)
        if ang != 0.0:
            f.SetOrientationDegrees(ang)
        if back:
            f.Flip(f.GetPosition(), False)
    # a footprint that owns a copper zone: KiCad's export drops it
    f = footprint(b, "C1", "syn:Copper", 150.0, 100.0, 0.0, "copper")
    pad(f, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 0, 1.5, 1.5, net=n2)
    f.Add(make_zone(f, lset(F), rect_pts(147, 96, 6, 8), net=n2, name="fp-copper"))
    b.Add(f)
    outline_rect(b, 10, 10, 200, 150)
    pcbnew.SaveBoard(path, b)


def arcs_board(path):
    import math
    b = new_board(2)
    n1, n2 = add_net(b, "N1"), add_net(b, "N2")
    fp = footprint(b, "U1", "syn:Two", 100.0, 5.0, 0.0)
    pad(fp, "1", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 0, 0, 1.5, 1.5, net=n1)
    pad(fp, "2", pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 4, 0, 1.5, 1.5, net=n2)
    b.Add(fp)

    def seg(x1, y1, x2, y2, net, layer=pcbnew.F_Cu, width=0.25, locked=False):
        t = pcbnew.PCB_TRACK(b)
        t.SetStart(pt(x1, y1))
        t.SetEnd(pt(x2, y2))
        t.SetWidth(nm(width))
        t.SetLayer(layer)
        t.SetNet(net)
        t.SetLocked(locked)
        b.Add(t)

    def arc_track(cx, cy, r, a0, sweep, net, layer=pcbnew.F_Cu, width=0.25):
        p = lambda a: (cx + r * math.cos(math.radians(a)), cy + r * math.sin(math.radians(a)))
        s, m, e = p(a0), p(a0 + sweep / 2), p(a0 + sweep)
        t = pcbnew.PCB_ARC(b)
        t.SetStart(pt(*s))
        t.SetMid(pt(*m))
        t.SetEnd(pt(*e))
        t.SetWidth(nm(width))
        t.SetLayer(layer)
        t.SetNet(net)
        b.Add(t)
        return s, e

    # a segment, an arc and a segment, joined: sweeps of 10, 90 and 270 degrees
    # (positive and negative) at radii from 0.1 mm to 20 mm
    row = 0
    for r in (0.1, 1.0, 5.0, 20.0):
        col = 0
        for sweep in (10.0, 90.0, 270.0, -90.0):
            cx, cy = 30.0 + col * 55.0, 30.0 + row * 55.0
            a0 = 200.0
            s, e = arc_track(cx, cy, r, a0, sweep, n1 if col % 2 == 0 else n2)
            net = n1 if col % 2 == 0 else n2
            seg(s[0] - 2.0, s[1], s[0], s[1], net)
            seg(e[0], e[1], e[0] + 2.0, e[1] + 1.5, net)
            col += 1
        row += 1
    # a locked segment
    seg(5, 5, 20, 5, n2, locked=True)
    outline_rect(b, 0, 0, 240, 240)
    pcbnew.SaveBoard(path, b)


def vias_board(path):
    b = new_board(6)
    n = [add_net(b, "N%d" % i) for i in range(1, 7)]
    fp = footprint(b, "U1", "syn:Row", 20.0, 10.0, 0.0)
    for i in range(6):
        pad(fp, str(i + 1), pcbnew.PAD_SHAPE_RECT, pcbnew.PAD_ATTRIB_SMD, 3 * i, 0, 1.5, 1.5, net=n[i])
    b.Add(fp)
    layers = [pcbnew.F_Cu, pcbnew.In1_Cu, pcbnew.In2_Cu, pcbnew.In3_Cu, pcbnew.In4_Cu, pcbnew.B_Cu]

    def via(x, y, kind, top, bottom, net, size=0.8, drill=0.4, locked=False):
        v = pcbnew.PCB_VIA(b)
        v.SetViaType(kind)
        v.SetPosition(pt(x, y))
        v.SetWidth(nm(size))
        v.SetDrill(nm(drill))
        v.SetLayerPair(layers[top], layers[bottom])
        v.SetNet(net)
        v.SetLocked(locked)
        b.Add(v)

    via(20, 30, pcbnew.VIATYPE_THROUGH, 0, 5, n[0])
    via(30, 30, pcbnew.VIATYPE_BLIND, 0, 2, n[1], 0.7, 0.3)
    via(40, 30, pcbnew.VIATYPE_BURIED, 1, 3, n[2], 0.6, 0.3)
    via(50, 30, pcbnew.VIATYPE_MICROVIA, 0, 1, n[3], 0.3, 0.1)
    via(60, 30, pcbnew.VIATYPE_THROUGH, 0, 5, n[4], 0.8, 0.4, locked=True)   # same padstack as the first, locked
    via(70, 30, pcbnew.VIATYPE_BLIND, 5, 3, n[5], 0.7, 0.3)                  # from the bottom side
    via(80, 30, pcbnew.VIATYPE_BLIND, 0, 2, n[1], 0.7, 0.3)                  # a second via of the same padstack
    outline_rect(b, 5, 5, 100, 50)
    pcbnew.SaveBoard(path, b)


def main(out, only):
    os.makedirs(out, exist_ok=True)
    boards = {
        "pads": lambda p: pads_board(p),
        "outline": lambda p: outline_board(p),
        "backside": lambda p: backside_board(p),
        "classes": lambda p: classes_board(p, 5),
        "classes6": lambda p: classes_board(p, 2),
        "rule-areas": lambda p: rule_areas_board(p),
        "arcs": lambda p: arcs_board(p),
        "vias": lambda p: vias_board(p),
    }
    for name, make in boards.items():
        if only and name not in only:
            continue
        make(os.path.join(out, name + ".kicad_pcb"))
    print("KICAD_VERSION " + pcbnew.GetBuildVersion())


if len(sys.argv) < 2:
    print("usage: synthetic.py OUT_DIR [BOARD ...]", file=sys.stderr)
    sys.exit(2)
main(sys.argv[1], sys.argv[2:])
