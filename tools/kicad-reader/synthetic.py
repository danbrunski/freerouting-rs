"""K05.8: generate the synthetic test boards of fixtures/kicad/synthetic/ with pcbnew.

    python3 -I tools/kicad-reader/synthetic.py OUT_DIR

Run under a KiCad interpreter (WSL: the system python3, KiCad 10.0.7). Writes
pads.kicad_pcb, outline.kicad_pcb, backside.kicad_pcb, classes.kicad_pcb and
classes6.kicad_pcb with their .kicad_pro files. The exports next to them
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


def main(out):
    os.makedirs(out, exist_ok=True)
    pads_board(os.path.join(out, "pads.kicad_pcb"))
    outline_board(os.path.join(out, "outline.kicad_pcb"))
    backside_board(os.path.join(out, "backside.kicad_pcb"))
    classes_board(os.path.join(out, "classes.kicad_pcb"), 5)
    classes_board(os.path.join(out, "classes6.kicad_pcb"), 2)
    print("KICAD_VERSION " + pcbnew.GetBuildVersion())


if len(sys.argv) != 2:
    print("usage: synthetic.py OUT_DIR", file=sys.stderr)
    sys.exit(2)
main(sys.argv[1])
