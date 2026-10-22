"""K15.42: the small board of `tests/k15_pour_pair_drc.rs`'s island repair test.

Writes `c-cut-island.kicad_pcb` into the current directory. Every coordinate is moteus's
(`moteus_c1.kicad_pcb`, K15.37's `pcb-on` route on main `0564a6d`, session `447b2a9c`):

- GNDPWR's zone of priority 33 on In1.Cu, x 131.2-134.05, y 93.4-99.7 mm, `island_removal_mode`
  1, clearance 0.2 mm, minimum thickness 0.25 mm, with no GNDPWR pad;
- the ends of the `+3V` In1.Cu track that cut it, (133.4799, 88.975) and (133.4799, 103.774)
  mm: here two `+3V` through-hole pads (TP1, TP2; 0.6 mm, drill 0.3 mm);
- a rule area on F.Cu, In2.Cu and B.Cu over the board (tracks and vias not allowed), so the
  route runs on In1.Cu as moteus's did.

The board's edge is x 128-140, y 86-107 mm. KiCad 10.0.7 (`pcb drc --refill-zones`) reports
two `isolated_copper` for the zone on the CLI route of main `0564a6d` (`-mp 3` with both
settings: the straight 0.2 mm track, the default net class, moteus's three on its 0.152 mm one)
and one on the route around the zone that K15.42's island repair makes.

Run with `python3 -I make_boards.py`.
"""
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
\t\t(4 "In1.Cu" signal)
\t\t(6 "In2.Cu" signal)
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


def rect(x0, y0, x1, y1):
    return " ".join(f"(xy {x} {y})" for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])


def pad(ref, x, y):
    return f'''\t(footprint "k1542:TP" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" thru_hole circle (at 0 0) (size 0.6 0.6) (drill 0.3) (layers "*.Cu" "*.Mask")
\t\t\t(net "+3V") (uuid "{u()}"))
\t)
'''


BODY = (
    f'\t(gr_rect (start 128 86) (end 140 107) (stroke (width 0.05) (type default)) (fill no) '
    f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
    + pad("TP1", 133.4799, 88.975)
    + pad("TP2", 133.4799, 103.774)
    + f'''\t(zone (layers "F.Cu" "In2.Cu" "B.Cu") (uuid "{u()}") (name "K") (hatch full 0.508)
\t\t(connect_pads (clearance 0))
\t\t(min_thickness 0.254)
\t\t(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed))
\t\t(placement (enabled no) (sheetname ""))
\t\t(fill (thermal_gap 0.508) (thermal_bridge_width 0.508) (island_removal_mode 0))
\t\t(polygon (pts {rect(128, 86, 140, 107)}))
\t)
\t(zone
\t\t(net "GNDPWR")
\t\t(layer "In1.Cu")
\t\t(uuid "{u()}")
\t\t(name "GNDPWR")
\t\t(priority 33)
\t\t(hatch edge 0.5)
\t\t(connect_pads yes
\t\t\t(clearance 0.2)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 1)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{rect(131.2, 93.4, 134.05, 99.7)}
\t\t\t)
\t\t)
\t)
'''
)
with open("c-cut-island.kicad_pcb", "w") as f:
    f.write(HEAD + BODY + ")\n")
