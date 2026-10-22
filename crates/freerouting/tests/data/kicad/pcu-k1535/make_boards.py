"""K15.35: the small boards of `tests/k15_pour_keepout.rs`, cut from the pcu board.

Writes `<name>-unfilled.kicad_pcb` into the current directory; `<name>.kicad_pcb` is each one
after `kicad-cli pcb drc --refill-zones --save-board` (KiCad 10.0.7), with the pcu board's
`.kicad_dru` beside it (as `<name>.kicad_dru` and `<name>-unfilled.kicad_dru`). Every
coordinate is the pcu board's after K15.34's DSN route (main `af0c0de`, session `6a90062c...`,
`target/orchestrator/k1535/runs/pcu-dsn-off-base`), where the cleanup's fill model lay 0.127 mm
short of KiCad's refilled fill along a footprint's rule area and round an NPTH:

- `a-l1-keepout`: zone BUCK_SW (`Net-(U1-SW)`, F.Cu, priority 22) verbatim, and L1 (at
  (204, 120.25)) with its two pads and its own rule area (tracks, vias and copper pour not
  allowed) x 200.6472-207.3528, y 113.9-126.6, verbatim. KiCad fills BUCK_SW up to the rule
  area's outline (x 200.6472) with no clearance;
- `b-f1-npth`: zone V_PD_IN (`/V_PD_IN`, F.Cu, priority 66) verbatim, and F1 (at (97.8, 120.1),
  -90 degrees) with its two pads and its NPTH (2.794 mm at (97.0, 124.1)) verbatim. KiCad's
  export writes the NPTH as a keepout circle grown by the board's hole clearance (3.294 mm),
  and KiCad fills V_PD_IN up to the hole clearance (0.254 mm, the `.kicad_dru`'s rule).

`<name>.dsn` is KiCad 10.0.7's Specctra export of `<name>.kicad_pcb`
(`tools/kicad-reader/export_dsn.py`), its first line's path cut to the file name.

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
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def edge(x0, y0, x1, y1):
    return (f'\t(gr_rect (start {x0} {y0}) (end {x1} {y1}) (stroke (width 0.05) (type default)) (fill no) '
            f'(layer "Edge.Cuts") (uuid "{u()}"))\n')
def footprint(ref, at, body):
    return f'''\t(footprint "k1535:{ref}" (layer "F.Cu") (uuid "{u()}") (at {at})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
{body}\t)
'''
def zone(net, name, priority, island_mode, pts):
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "{net}")
\t\t(layer "F.Cu")
\t\t(uuid "{u()}")
\t\t(name "{name}")
\t\t(hatch edge 0.5)
\t\t(priority {priority})
\t\t(connect_pads yes
\t\t\t(clearance 0.5)
\t\t)
\t\t(min_thickness 0.5)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(smoothing fillet)
\t\t\t(island_removal_mode {island_mode})
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''
def smd(number, x, y, net):
    return f'''\t\t(pad "{number}" smd rect (at {x} {y}) (size 2.8956 5.4102) (layers "F.Cu" "F.Mask" "F.Paste")
\t\t\t(net "{net}") (uuid "{u()}"))
'''
def l1_rule_area():
    xy = " ".join(f"(xy {x} {y})" for x, y in L1_RULE_AREA)
    return f'''\t\t(zone (layer "F.Cu") (uuid "{u()}") (hatch full 0.508)
\t\t\t(connect_pads (clearance 0))
\t\t\t(min_thickness 0.254)
\t\t\t(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
\t\t\t(placement (enabled no) (sheetname ""))
\t\t\t(fill (thermal_gap 0.508) (thermal_bridge_width 0.508) (island_removal_mode 0))
\t\t\t(polygon (pts {xy}))
\t\t)
'''
def tht(number, x, y, net):
    return f'''\t\t(pad "{number}" thru_hole circle (at {x} {y} 270) (size 2.1082 2.1082) (drill 1.6002)
\t\t\t(layers "*.Cu" "*.Mask") (remove_unused_layers no) (net "{net}") (uuid "{u()}"))
'''
def npth():
    return f'''\t\t(pad "" np_thru_hole circle (at 4 0.800001 270) (size 2.794 2.794) (drill 2.794)
\t\t\t(layers "*.Cu" "*.Mask") (uuid "{u()}"))
'''
SW = "Net-(U1-SW)"
C7 = "Net-(C7-Pad1)"
BUCK_SW = [(195.25, 113.600464), (195.25, 132.25), (200.748315, 132.25), (200.745165, 113.599536)]
L1_RULE_AREA = [(200.6472, 113.9), (207.3528, 113.9), (207.3528, 126.6), (200.6472, 126.6)]
SENSE = "/CH1_SENSE"
PD = "/V_PD_IN"
V_PD_IN = [(99.5, 126.5), (99.45003, 126.500002), (99.449897, 122.25), (77.5, 122.25), (77.5, 129.65),
           (99.5, 129.65)]
BOARDS = {
    "a-l1-keepout": edge(193, 111, 210, 134)
    + footprint("L1", "204 120.25", smd("1", -4.8514, 0, SW) + smd("2", 4.8514, 0, C7) + l1_rule_area())
    + zone(SW, "BUCK_SW", 22, 1, BUCK_SW),
    "b-f1-npth": edge(76, 118, 101, 131)
    + footprint("F1", "97.8 120.1 -90", npth() + tht("1", 0, 0, SENSE) + tht("2", 7.999999, 0, PD))
    + zone(PD, "V_PD_IN", 66, 0, V_PD_IN),
}
for name, body in BOARDS.items():
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        f.write(HEAD + body + ")\n")
