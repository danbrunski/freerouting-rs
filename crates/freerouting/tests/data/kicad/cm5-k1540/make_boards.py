"""K15.40: the small boards of `tests/k15_pad_clearance_fill.rs`, cut from CM5_MINIMA_3.

Writes `<name>-unfilled.kicad_pcb` and `<name>-unfilled.kicad_pro` into the current directory;
`<name>.kicad_pcb` is each one after `kicad-cli pcb drc --refill-zones --save-board` (KiCad
10.0.7), and `<name>.dsn` the KiCad plugin's export of it (`kicad-plugin/headless_run.py export`
with `{"schema": 5, "profile": "enhanced"}`: KiCad's Specctra export plus the plugin's fix-ups,
among them `edge_and_pad_keepouts`' pad-clearance circles and edge strips), its first line's path
cut to the file name. Every coordinate is CM5_MINIMA_3's (K13.6's board,
`target/orchestrator/k135/boards/cm5_minima`), where K15.40 measured its route on main `7bbbc09`
(`target/orchestrator/k1540/`):

- the board outline: CM5's Edge.Cuts near its mounting hole at (127.25, 77.6) mm, verbatim (the
  bottom edge y 81.07 from x 120, two arcs, the 45-degree line, the corner arc, the right edge x
  132.9096), closed by two cuts at x 120 and y 72; Board Setup's copper-to-edge clearance 0.372
  mm, hole clearance 0.2 mm, Default class clearance 0.13 mm (CM5's `.kicad_pro`);
- the GND zone: CM5's (F.Cu and B.Cu of its five layers; clearance 0.15 mm, min thickness 0.15
  mm, thermal gap 0.13 mm, spoke 0.157 mm, island removal 2 with 5 mm2), its outline cut to the
  board's box;
- `a-netless-npth`: the two NPTHs Module301 and Module302 stack at (127.25, 77.6), verbatim
  (2.7 mm with `(clearance 1.5)`, 3.0 mm with `(clearance 1.7)`, no net). KiCad keeps the fill
  hole radius + 1.7 mm (+ 0.5 um) from the centre; the plugin's circles are 5.44 and 6.14 mm
  (the clearance less the 0.13 mm default);
- `c-edge-rule`: `a-netless-npth` with a `.kicad_dru` (`c-edge-rule.kicad_dru`, and
  `-unfilled`) holding the pcu board's outer-layer rule, `edge_clearance` 0.254 mm, below Board
  Setup's 0.372 mm. KiCad keeps the fill 0.2545 mm from the straight edges: the rule replaces
  Board Setup's value;
- `d-inner-layers` (K15.45): `a-netless-npth` on CM5's six copper layers (F.Cu, In1.Cu-In4.Cu,
  B.Cu) with the GND zone on CM5's five GND layers (F.Cu, In1.Cu, In2.Cu, In4.Cu, B.Cu). The plugin
  and the reader write the pad-clearance circles on F.Cu and B.Cu only; KiCad keeps the fill hole
  radius + 1.7 mm (+ 0.5 um) from the centre on the inner layers too;
- `e-pad-override` (K15.45): pads 102, 104, 106 and 108 of Module302 (CM5's module footprint,
  `(clearance 0.127)` at the footprint, on B.Cu at (127.25, 77.6) rotated 180 degrees), verbatim,
  their nets (`/CM5/PCIE_PI.nCLKREQ`, `nWAKE`, `PWR_EN`, class 90ohm with 0.2 mm by CM5's
  `/*PCIE*` pattern; GND), CM5's GND zone settings on B.Cu over them (the zone and the board
  outline are cut to x 89.5-94.5, y 44.5-49 mm). The footprint's clearance overrides the net
  classes' 0.2 mm and the zone's 0.15 mm: KiCad keeps the fill 0.1275 mm from the signal pads
  (K15.40's model kept 0.2016 mm on CM5_MINIMA_3's route, 0.12 mm2 at (91.70, 46.70));
- `b-gnd-pad`: CM5 has no mounting-hole pad of a pour's net, so Module302's hole is made a GND
  plated pad (3.6 mm, drill 3.0 mm) with its `(clearance 1.7)`, at the same place. KiCad joins it
  to the GND zone by thermal relief (gap 0.13 mm).

Run with `python3 -I make_boards.py`.
"""
import json

HEAD = '''(kicad_pcb
\t(version 20260206)
\t(generator "pcbnew")
\t(generator_version "10.0")
\t(general
\t\t(thickness 1.6)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal)
INNER\t\t(2 "B.Cu" signal)
\t\t(13 "F.Paste" user)
\t\t(15 "B.Paste" user)
\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(7 "B.SilkS" user "B.Silkscreen")
\t\t(1 "F.Mask" user)
\t\t(3 "B.Mask" user)
\t\t(25 "Edge.Cuts" user)
\t\t(31 "F.CrtYd" user "F.Courtyard")
\t\t(29 "B.CrtYd" user "B.Courtyard")
\t\t(35 "F.Fab" user)
\t\t(33 "B.Fab" user)
\t)
\t(setup (pad_to_mask_clearance 0))
\t(net 0 "")
\t(net 1 "GND")
'''
uid = [0x100]


def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]


def line(a, b):
    return (f'\t(gr_line (start {a[0]} {a[1]}) (end {b[0]} {b[1]}) (stroke (width 0.05) (type default)) '
            f'(layer "Edge.Cuts") (uuid "{u()}"))\n')


def arc(a, m, b):
    return (f'\t(gr_arc (start {a[0]} {a[1]}) (mid {m[0]} {m[1]}) (end {b[0]} {b[1]}) (stroke (width 0.05) '
            f'(type default)) (layer "Edge.Cuts") (uuid "{u()}"))\n')


# CM5_MINIMA_3's Edge.Cuts round the hole, verbatim, closed by two cuts (x 120, y 72).
RIGHT_TOP = (132.909189, 72)  # on CM5's line (132.901934, 30.038716)-(132.909646, 74.640883)
OUTLINE = (
    line((120, 81.07), (126.424817, 81.07))
    + arc((126.424817, 81.07), (128.349345, 81.455227), (129.977331, 82.551528))
    + line((129.977331, 82.551528), (130.978053, 83.561932))
    + arc((135.373718, 79.19012), (135.336085, 83.54796), (130.978053, 83.561932))
    + line((134.34828, 78.149531), (135.373718, 79.19012))
    + arc((134.34828, 78.149531), (133.283427, 76.536885), (132.909646, 74.640883))
    + line((132.909646, 74.640883), RIGHT_TOP)
    + line(RIGHT_TOP, (120, 72))
    + line((120, 72), (120, 81.07))
)


# K15.45: CM5_MINIMA_3's inner copper layers, verbatim.
INNER = """\t\t(4 "In1.Cu" signal)
\t\t(6 "In2.Cu" signal)
\t\t(8 "In3.Cu" signal)
\t\t(10 "In4.Cu" signal)
"""


def zone(layers='"F.Cu" "B.Cu"'):
    pts = [(120, 72), (136.31, 72), (136.31, 84.45), (120, 84.45)]
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net 1)
\t\t(net_name "GND")
\t\t(layers {layers})
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
\t\t(connect_pads
\t\t\t(clearance 0.15)
\t\t)
\t\t(min_thickness 0.15)
\t\t(fill yes
\t\t\t(thermal_gap 0.13)
\t\t\t(thermal_bridge_width 0.157)
\t\t\t(island_removal_mode 2)
\t\t\t(island_area_min 5)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''


def footprint(ref, pad):
    return f'''\t(footprint "k1540:{ref}" (layer "F.Cu") (uuid "{u()}") (at 127.25 77.6 180)
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
{pad}\t)
'''


def npth(size, layers, clearance):
    return (f'\t\t(pad "" np_thru_hole circle (at 0 0 180) (size {size} {size}) (drill {size}) '
            f'(layers {layers} "*.Mask") (clearance {clearance}) (uuid "{u()}"))\n')


def gnd_pad():
    return (f'\t\t(pad "1" thru_hole circle (at 0 0 180) (size 3.6 3.6) (drill 3) (layers "*.Cu" "*.Mask") '
            f'(net 1 "GND") (clearance 1.7) (uuid "{u()}"))\n')


BOARDS = {
    "a-netless-npth": footprint("Module301", npth(2.7, '"F&B.Cu"', 1.5))
    + footprint("Module302", npth(3, '"F&B.Cu"', 1.7)),
    "b-gnd-pad": footprint("Module302", gnd_pad()),
}
BOARDS["c-edge-rule"] = BOARDS["a-netless-npth"]
BOARDS["d-inner-layers"] = BOARDS["a-netless-npth"]
# K15.45: the six-layer board's copper layers and GND zone layers (CM5_MINIMA_3's).
SIX = {"d-inner-layers": '"F.Cu" "B.Cu" "In1.Cu" "In2.Cu" "In4.Cu"'}
DRU = {"c-edge-rule": '''(version 1)

(rule "Edge clearance, outer layers (10 mil)"
    (layer outer)
    (constraint edge_clearance (min 0.254mm))
    (severity error))
'''}
PRO = {
    "board": {"design_settings": {"rules": {"min_clearance": 0.0, "min_copper_edge_clearance": 0.372,
                                            "min_hole_clearance": 0.2}}},
    "net_settings": {"classes": [{"name": "Default", "clearance": 0.13, "track_width": 0.15,
                                  "via_diameter": 0.45, "via_drill": 0.3}]},
    "meta": {"filename": "x.kicad_pro", "version": 3},
}
for name, body in BOARDS.items():
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        head = HEAD.replace("INNER", INNER if name in SIX else "")
        f.write(head + OUTLINE + body + (zone(SIX[name]) if name in SIX else zone()) + ")\n")
    with open(f"{name}-unfilled.kicad_pro", "w") as f:
        json.dump(dict(PRO, meta={"filename": f"{name}-unfilled.kicad_pro", "version": 3}), f, indent=2)
    if name in DRU:
        with open(f"{name}-unfilled.kicad_dru", "w") as f:
            f.write(DRU[name])

# K15.45: `e-pad-override`, CM5's Module302 pads 102-108 (verbatim) under its GND zone on B.Cu.
E_NETS = ["GND", "/CM5/PCIE_PI.nCLKREQ", "/CM5/PCIE_PI.nWAKE", "/CM5/PCIE_PI.PWR_EN"]
E_PADS = [("102", 31.3, 2), ("104", 30.9, 3), ("106", 30.5, 4), ("108", 30.1, 1)]


def e_board():
    uid[0] = 0x100
    head = HEAD.replace("INNER", "").replace('\t(net 1 "GND")\n', "".join(
        f'\t(net {i + 1} "{n}")\n' for i, n in enumerate(E_NETS)))
    box = [(89.5, 44.5), (94.5, 44.5), (94.5, 49), (89.5, 49)]
    outline = "".join(line(box[i], box[(i + 1) % 4]) for i in range(4))
    pads = "".join(
        f'''\t\t(pad "{num}" smd rect (at 35.04 {y} 270) (size 0.2 0.7) (layers "B.Cu" "B.Mask" "B.Paste") '''
        f'''(net {net} "{E_NETS[net - 1]}") (uuid "{u()}"))\n'''
        for num, y, net in E_PADS)
    fp = f'''\t(footprint "CM5IO:Raspberry-Pi-5-Compute-Module_HSS" (layer "B.Cu") (uuid "{u()}") (at 127.25 77.6 180)
\t\t(property "Reference" "Module302" (at 0 0 0) (layer "B.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "B.Fab") (uuid "{u()}"))
\t\t(clearance 0.127)
{pads}\t)
'''
    z = zone('"B.Cu"').replace("(xy 120 72) (xy 136.31 72) (xy 136.31 84.45) (xy 120 84.45)",
                              "(xy 89.5 44.5) (xy 94.5 44.5) (xy 94.5 49) (xy 89.5 49)")
    return head + outline + fp + z + ")\n"


E_PRO = dict(PRO, meta={"filename": "e-pad-override-unfilled.kicad_pro", "version": 3})
E_PRO["net_settings"] = {
    "classes": PRO["net_settings"]["classes"] + [{"name": "90ohm", "clearance": 0.2, "track_width": 0.147,
                                                  "via_diameter": 0.45, "via_drill": 0.3, "priority": 2}],
    "netclass_patterns": [{"netclass": "90ohm", "pattern": "/*PCIE*"}],
}
with open("e-pad-override-unfilled.kicad_pcb", "w") as f:
    f.write(e_board())
with open("e-pad-override-unfilled.kicad_pro", "w") as f:
    json.dump(E_PRO, f, indent=2)
