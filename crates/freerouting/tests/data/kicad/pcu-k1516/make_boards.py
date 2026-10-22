"""K15.16: the small boards of `tests/drc_kicad_fills.rs`.

Writes `<name>-unfilled.kicad_pcb` (zones without fills) into the current
directory; `<name>.kicad_pcb` is each one after
`kicad-cli pcb drc --severity-all --format json --refill-zones --save-board`
(KiCad 10.0.7). The layer stack is the pcu board's (`../power-control-unit`
at `76163c7`). Run with `python3 -I make_boards.py`.
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
\t(gr_rect (start 0 0) (end 50 40) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e0"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def pad(ref, x, y, net):
    return f'''\t(footprint "k1516:smd" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def seg(x0, y0, x1, y1, net, layer="F.Cu", w=0.5):
    return f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width {w}) (layer "{layer}") (net "{net}") (uuid "{u()}"))\n'
def zone(net, layer, pts, mode=0, prio=0):
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "{net}")
\t\t(layer "{layer}")
\t\t(uuid "{u()}")
\t\t(name "{net}")
\t\t(hatch edge 0.5)
\t\t(priority {prio})
\t\t(connect_pads yes
\t\t\t(clearance 0.3)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode {mode})
\t\t\t(island_area_min 10)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''
rect = lambda x0, y0, x1, y1: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
def board(parts):
    return HEAD + "".join(parts) + ")\n"
boards = {}
# a: two pads of N1 on F.Cu joined only by N1's F.Cu pour; N1 also has a plane on In2.Cu,
# so the DSN reader drops the F.Cu pour (K15.11's dropped pours)
boards["a-pour-joins-two-pads"] = board([
    pad("J1", 15, 20, "N1"), pad("J2", 35, 20, "N1"),
    zone("N1", "F.Cu", rect(10, 10, 40, 30)),
    zone("N1", "In2.Cu", rect(1, 1, 49, 39)),
])
# b: N2's trace cuts N1's pour in two, one N1 pad on each side
boards["b-pour-cut-in-two"] = board([
    pad("J1", 15, 20, "N1"), pad("J2", 35, 20, "N1"),
    pad("J3", 25, 5, "N2"), pad("J4", 25, 35, "N2"),
    seg(25, 5, 25, 35, "N2"),
    zone("N1", "F.Cu", rect(10, 10, 40, 30)),
])
# c: N2's trace cuts off a part of N1's pour with no N1 item: an isolated island
for mode in (0, 1):
    uid[0] = 0x100
    boards[f"c-island-mode-{mode}"] = board([
        pad("J1", 15, 20, "N1"), pad("J2", 20, 20, "N1"),
        seg(15, 20, 20, 20, "N1"),
        pad("J3", 35, 5, "N2"), pad("J4", 35, 35, "N2"),
        seg(35, 5, 35, 35, "N2"),
        zone("N1", "F.Cu", rect(10, 10, 40, 30), mode=mode),
    ])

# c2: as c, island_removal_mode 2, the part under or over island_area_min
for label, amin in (("keep", 10), ("remove", 200)):
    uid[0] = 0x100
    boards[f"c-island-mode-2-{label}"] = board([
        pad("J1", 15, 20, "N1"), pad("J2", 20, 20, "N1"),
        seg(15, 20, 20, 20, "N1"),
        pad("J3", 35, 5, "N2"), pad("J4", 35, 35, "N2"),
        seg(35, 5, 35, 35, "N2"),
        zone("N1", "F.Cu", rect(10, 10, 40, 30), mode=2),
    ]).replace("(island_area_min 10)", f"(island_area_min {amin})")
# d2: N1's pour cut in two by N2's trace, neither part touching an N1 item; N1's pads elsewhere
uid[0] = 0x400
boards["d2-two-isolated-parts"] = board([
    pad("J1", 5, 5, "N1"), pad("J2", 5, 35, "N1"), seg(5, 5, 5, 35, "N1"),
    pad("J3", 25, 5, "N2"), pad("J4", 25, 35, "N2"), seg(25, 5, 25, 35, "N2"),
    zone("N1", "F.Cu", rect(10, 10, 40, 30)),
])
# d3: three parts: one with N1's pads, two isolated
uid[0] = 0x500
boards["d3-one-connected-two-isolated"] = board([
    pad("J1", 12, 15, "N1"), pad("J2", 12, 25, "N1"), seg(12, 15, 12, 25, "N1"),
    pad("J3", 20, 5, "N2"), pad("J4", 20, 35, "N2"), seg(20, 5, 20, 35, "N2"),
    pad("J5", 30, 5, "N4"), pad("J6", 30, 35, "N4"), seg(30, 5, 30, 35, "N4"),
    zone("N1", "F.Cu", rect(10, 10, 40, 30)),
])
for name, text in boards.items():
    open(f"{name}-unfilled.kicad_pcb", "w").write(text)
