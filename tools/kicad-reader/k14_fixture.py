"""K14: generate the board-snapshot fix-up fixture with pcbnew.

    python3 -I tools/kicad-reader/k14_fixture.py [OUT_DIR]

Run under a KiCad interpreter (WSL: the system python3 of KiCad 10.0.7) from
the repository root. OUT_DIR defaults to `crates/freerouting-kicad/tests/data`.
Writes, there:

- `k14-fixups.kicad_pcb` and `k14-fixups.kicad_pro`: a four-layer board built
  with pcbnew, saved, loaded again (so the project's net classes apply),
  zones filled with `ZONE_FILLER` and saved again;
- `k14-fixups.kicad10.dsn`: KiCad's own export of it (`ExportSpecctraDSN`);
- `k14-fixups.board.json`: the plugin's board facts, `kicad_board.collect`
  and `export_flow.board_classes`, as the plugin reads them from pcbnew;
- `k14-fixups.fixed.dsn`: the export after `dsn_fixups.apply` with
  `class_widths`, `drop_pour_nets` and `route_on_plane_layers` on (the
  values `SETTINGS` names, which are also what `auto` resolves to here).

Never imports the freerouting_rs plugin package (its `register()` raises a
wxWidgets dialog outside the editor): the plugin's folder goes on `sys.path`
and its pure modules load as top-level modules, as the plugin tests do.
The board holds random UUIDs, so a rerun changes the committed files; the
tests compare the reader against the recorded files and need no pcbnew.

The board: nets GND, VBUS and V3V3 (class Power, 1.0 mm), VSW (class Switch,
1.5 mm), SIG1 and SIG2 (Default, 0.25 mm), on SMD pads; In1.Cu and In2.Cu
are power layers. Zones: GND on F.Cu (priority 0) and B.Cu, a VBUS pour over
it on F.Cu (priority 1, the outer pour of a class K14.4 lists), a GND plane
on In1.Cu (the only net there, so `auto` drops GND's outer pours), and VBUS
and V3V3 halves on In2.Cu (the plane layer opened to routing).
"""

import json
import os
import subprocess
import sys

import pcbnew

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "kicad-plugin", "freerouting_rs"))

import dsn_fixups  # noqa: E402  (the plugin's folder, not the package)
import export_flow  # noqa: E402
import kicad_board  # noqa: E402

STEM = "k14-fixups"
SETTINGS = {
    "class_widths": {"Power": 0.6, "Switch": 0.8},
    "drop_pour_nets": ["GND"],
    "route_on_plane_layers": ["In2.Cu"],
}
V2 = pcbnew.VECTOR2I


def nm(mm):
    return int(round(mm * 1e6))


def pt(x, y):
    return V2(nm(x), nm(y))


def chain(points):
    c = pcbnew.SHAPE_LINE_CHAIN()
    for x, y in points:
        c.Append(nm(x), nm(y))
    c.SetClosed(True)
    return c


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def outline(b, x0, y0, x1, y1):
    pts = rect(x0, y0, x1, y1)
    for (xa, ya), (xb, yb) in zip(pts, pts[1:] + pts[:1]):
        s = pcbnew.PCB_SHAPE(b)
        s.SetShape(pcbnew.SHAPE_T_SEGMENT)
        s.SetLayer(pcbnew.Edge_Cuts)
        s.SetWidth(nm(0.1))
        s.SetStart(pt(xa, ya))
        s.SetEnd(pt(xb, yb))
        b.Add(s)


def part(b, ref, x, y, pads):
    """A footprint at (x, y) with SMD rect pads `(number, dx, w, h, net)` on F.Cu."""
    fp = pcbnew.FOOTPRINT(b)
    fp.SetFPIDAsString("k14:" + ref)
    fp.SetReference(ref)
    fp.SetValue("V")
    fp.SetPosition(pt(x, y))
    for number, dx, w, h, net in pads:
        p = pcbnew.PAD(fp)
        p.SetNumber(number)
        p.SetAttribute(pcbnew.PAD_ATTRIB_SMD)
        p.SetShape(pcbnew.PAD_SHAPE_RECT)
        p.SetSize(V2(nm(w), nm(h)))
        p.SetFPRelativePosition(pt(dx, 0))
        layers = pcbnew.LSET()
        for layer in (pcbnew.F_Cu, pcbnew.F_Paste, pcbnew.F_Mask):
            layers.AddLayer(layer)
        p.SetLayerSet(layers)
        p.SetNet(net)
        fp.Add(p)
    b.Add(fp)


def zone(b, layer, net, pts, priority):
    z = pcbnew.ZONE(b)
    z.SetLayer(layer)
    z.Outline().AddOutline(chain(pts))
    z.SetIsRuleArea(False)
    z.SetNet(net)
    z.SetAssignedPriority(priority)
    b.Add(z)


def build(path):
    b = pcbnew.BOARD()
    b.SetCopperLayerCount(4)
    b.SetLayerType(pcbnew.In1_Cu, pcbnew.LT_POWER)
    b.SetLayerType(pcbnew.In2_Cu, pcbnew.LT_POWER)
    nets = {}
    for name in ("GND", "VBUS", "V3V3", "VSW", "SIG1", "SIG2"):
        nets[name] = pcbnew.NETINFO_ITEM(b, name)
        b.Add(nets[name])
    n = nets
    outline(b, 0, 0, 50, 40)
    part(b, "U1", 20, 20, [(str(i + 1), 1.27 * i, 0.6, 1.2, n[k]) for i, k in enumerate(("VBUS", "V3V3", "SIG1", "SIG2", "GND"))])
    part(b, "U2", 32, 20, [("1", 0, 0.8, 1.6, n["VSW"]), ("2", 1.5, 0.8, 1.6, n["GND"]), ("3", 3.0, 0.8, 1.6, n["SIG1"])])
    part(b, "C1", 8, 8, [("1", 0, 1.0, 1.2, n["VBUS"]), ("2", 2.0, 1.0, 1.2, n["GND"])])
    part(b, "C2", 40, 32, [("1", 0, 1.0, 1.2, n["V3V3"]), ("2", 2.0, 1.0, 1.2, n["GND"])])
    part(b, "L1", 32, 10, [("1", 0, 1.5, 2.0, n["VSW"]), ("2", 4.0, 1.5, 2.0, n["VBUS"])])
    part(b, "J1", 44, 8, [("1", 0, 1.0, 1.0, n["SIG2"]), ("2", 2.0, 1.0, 1.0, n["V3V3"])])
    zone(b, pcbnew.F_Cu, n["GND"], rect(1, 1, 49, 39), 0)
    zone(b, pcbnew.F_Cu, n["VBUS"], rect(3, 3, 14, 14), 1)
    zone(b, pcbnew.B_Cu, n["GND"], rect(1, 1, 49, 39), 0)
    zone(b, pcbnew.In1_Cu, n["GND"], rect(1, 1, 49, 39), 0)
    zone(b, pcbnew.In2_Cu, n["VBUS"], rect(1, 1, 24.5, 39), 0)
    zone(b, pcbnew.In2_Cu, n["V3V3"], rect(25.5, 1, 49, 39), 0)
    pcbnew.SaveBoard(path, b)


def project(path):
    def cls(name, clearance, track, via, drill):
        return {
            "name": name, "priority": 2147483647 if name == "Default" else 0, "clearance": clearance,
            "track_width": track, "via_diameter": via, "via_drill": drill,
            "microvia_diameter": 0.3, "microvia_drill": 0.1, "diff_pair_gap": 0.25,
            "diff_pair_width": 0.2, "diff_pair_via_gap": 0.25, "line_style": 0, "bus_width": 12,
            "wire_width": 6, "pcb_color": "rgba(0, 0, 0, 0.000)",
            "schematic_color": "rgba(0, 0, 0, 0.000)", "tuning_profile": "",
        }

    classes = [cls("Default", 0.2, 0.25, 0.8, 0.4), cls("Power", 0.2, 1.0, 1.0, 0.5), cls("Switch", 0.25, 1.5, 1.0, 0.5)]
    patterns = [{"netclass": "Power", "pattern": "VBUS"}, {"netclass": "Power", "pattern": "V3V3"},
                {"netclass": "Switch", "pattern": "VSW"}]
    pro = {
        "meta": {"filename": STEM + ".kicad_pro", "version": 3},
        "net_settings": {"classes": classes, "meta": {"version": 5}, "net_colors": None,
                         "netclass_assignments": None, "netclass_patterns": patterns},
    }
    with open(path, "w") as f:
        json.dump(pro, f, indent=2)


def main(out):
    """Builds the board and its project, then does the rest in a second
    interpreter: pcbnew keeps the project the first `SaveBoard` opened in
    memory, so a `LoadBoard` in the same process would not read the net
    classes written after it."""
    os.makedirs(out, exist_ok=True)
    base = os.path.join(out, STEM)
    build(base + ".kicad_pcb")
    project(base + ".kicad_pro")
    return subprocess.run([sys.executable, "-I", os.path.abspath(__file__), "--finish", out]).returncode


def finish(out):
    base = os.path.join(out, STEM)
    board = pcbnew.LoadBoard(base + ".kicad_pcb")
    if not pcbnew.ZONE_FILLER(board).Fill(board.Zones()):
        raise SystemExit("zone fill failed")
    pcbnew.SaveBoard(base + ".kicad_pcb", board)
    if not pcbnew.ExportSpecctraDSN(board, base + ".kicad10.dsn"):
        raise SystemExit("ExportSpecctraDSN failed")
    prl = base + ".kicad_prl"
    if os.path.exists(prl):
        os.remove(prl)
    facts = kicad_board.collect(board, pcbnew)
    facts["classes"] = export_flow.board_classes(board)
    with open(base + ".board.json", "w") as f:
        json.dump(facts, f, indent=1, sort_keys=True)
        f.write("\n")
    with open(base + ".kicad10.dsn", encoding="utf-8", newline="") as f:
        export = f.read()
    dsn = dsn_fixups.Dsn(export)
    auto = dsn_fixups.resolve({"class_widths": "auto", "drop_pour_nets": "auto"}, facts, dsn)
    if auto["class_widths"] != SETTINGS["class_widths"] or auto["drop_pour_nets"] != SETTINGS["drop_pour_nets"]:
        raise SystemExit(f"auto gives {auto}, not the SETTINGS")
    fixed, log = dsn_fixups.apply(export, facts, SETTINGS)
    with open(base + ".fixed.dsn", "w", encoding="utf-8", newline="") as f:
        f.write(fixed)
    print("KICAD_VERSION " + pcbnew.GetBuildVersion())
    for line in log:
        print(line)
    return 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--finish"]:
        sys.exit(finish(sys.argv[2]))
    default = os.path.join(REPO, "crates", "freerouting-kicad", "tests", "data")
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else default))
