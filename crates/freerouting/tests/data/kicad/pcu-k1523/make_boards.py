"""K15.23: the small boards of `tests/k15_pour_fill_stub.rs`, cut from the pcu board.

Writes `<name>-unfilled.kicad_pcb` into the current directory; `<name>.kicad_pcb` is each one
after `kicad-cli pcb drc --severity-all --format json --refill-zones --save-board` (KiCad
10.0.7), with the `.kicad_dru` beside it (the pcu board's, as K15.4's copy
`target/orchestrator/k154/stripped/` has it, copied as `<name>.kicad_dru` and
`<name>-unfilled.kicad_dru`). Every coordinate of the zone and the copper is the
pcu board's after K15.18's `.kicad_pcb` route (`target/orchestrator/k1518/runs/`, session
`a2d99014...`): zone EFUSE_NO_6 (`/CH6_NO`, F.Cu, priority 43, clearance 0.5, min_thickness 0.5)
verbatim, `/CH6_NO`'s routed stub (F.Cu from (124.59, 108.0953) to a via at (125.0442,
107.7107), B.Cu to (126.9387, 105.8162)) and `Net-(U15-PGTH)`'s routed F.Cu track at x
124.0539 through the zone. The pads are this script's: J1 (`/CH6_NO`) in the zone, J2
(`/CH6_NO`, through-hole) at the B.Cu end, J3 and J4 (`Net-(U15-PGTH)`) at the track's ends.

- `a-stub-beside-a-crossing`: all of it. KiCad's refill leaves 0.347 mm of fill between the
  PGTH track's clearance and the zone's edge at x 124.6279, under the zone's 0.5 mm minimum
  thickness, so the stub's end lies in the outline but in no fill;
- `b-no-crossing`: the same without the PGTH track (J3 and J4 stay): the fill reaches the end.

Run with `python3 -I make_boards.py`.
"""
HEAD = '''(kicad_pcb
\t(version 20260206)
\t(generator "pcbnew")
\t(generator_version "10.0")
\t(general
\t\t(thickness 1.67)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal)
\t\t(4 "In1.Cu" power "GND")
\t\t(6 "In2.Cu" power "V_BUS")
\t\t(2 "B.Cu" signal)
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
\t(gr_rect (start 115 100) (end 135 118) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e0"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def smd(ref, x, y, net, size):
    return f'''\t(footprint "k1523:smd" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size {size} {size}) (layers "F.Cu" "F.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def tht(ref, x, y, net):
    return f'''\t(footprint "k1523:tht" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" thru_hole circle (at 0 0) (size 1 1) (drill 0.5) (layers "*.Cu" "*.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def seg(x0, y0, x1, y1, net, layer="F.Cu", w=0.2):
    return f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width {w}) (layer "{layer}") (net "{net}") (uuid "{u()}"))\n'
def via(x, y, net):
    return f'\t(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "{net}") (uuid "{u()}"))\n'
OUTLINE = [(121.827913, 106.791089), (121.827913, 108.841089), (121.75, 108.841089), (121.75, 114.1),
           (123.980145, 114.1), (123.98118, 114.219998), (128.597296, 114.180188),
           (128.569831, 110.634741), (124.627913, 110.665277), (124.627913, 106.791089)]
def zone():
    xy = " ".join(f"(xy {x} {y})" for x, y in OUTLINE)
    return f'''\t(zone
\t\t(net "/CH6_NO")
\t\t(layer "F.Cu")
\t\t(uuid "{u()}")
\t\t(name "EFUSE_NO_6")
\t\t(hatch edge 0.5)
\t\t(priority 43)
\t\t(connect_pads yes
\t\t\t(clearance 0.5)
\t\t)
\t\t(min_thickness 0.5)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(smoothing fillet)
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''
CH6 = "/CH6_NO"
PGTH = "Net-(U15-PGTH)"
def parts(crossing):
    out = [smd("J1", 127.5, 113.0, CH6, 1), tht("J2", 126.9387, 105.8162, CH6),
           tht("J3", 121.0447, 106.6112, PGTH), smd("J4", 122.7801, 111.3433, PGTH, 0.6)]
    out += [seg(124.59, 108.0953, 124.9746, 107.7107, CH6), seg(124.9746, 107.7107, 125.0442, 107.7107, CH6),
            via(125.0442, 107.7107, CH6), seg(126.9387, 105.8162, 125.0442, 107.7107, CH6, "B.Cu")]
    if crossing:
        out += [seg(121.0447, 106.6112, 123.6889, 106.6112, PGTH), seg(123.6889, 106.6112, 124.0539, 106.9762, PGTH),
                seg(124.0539, 106.9762, 124.0539, 110.0695, PGTH), seg(124.0539, 110.0695, 122.7801, 111.3433, PGTH)]
    out.append(zone())
    return out
for name, crossing in [("a-stub-beside-a-crossing", True), ("b-no-crossing", False)]:
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        f.write(HEAD + "".join(parts(crossing)) + ")\n")
