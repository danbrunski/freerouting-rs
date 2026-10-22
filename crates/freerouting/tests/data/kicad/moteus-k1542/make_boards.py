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

K15.45 adds two boards cut from moteus (K15.37's `pcb-on` route on main `bd44781`, session
`f286d7f6`), V+'s zones on In2.Cu: its zone of priority 0 (clearance 0.5 mm, minimum thickness
0.25 mm, `island_removal_mode` 0) with its outline cut to y 88 mm and x 140 mm (its edges
(131.2, 88)-(131.2, 93.8)-(137, 93.8)-(140, 96.6) verbatim), and its zone of priority 34
(x 131.2-134.1, y 93.5-97.3 mm, clearance 0.2 mm, minimum thickness 0.25 mm, `island_removal_mode` 1) whose top
edge lies 0.3 mm inside the first; a V+ through-hole pad (C1, 0.6 mm, drill 0.3 mm) at C40's
place (135.3, 89.375) mm, in the first zone. KiCad's filler knocks the higher zone's outline
out of the lower one's fill (same net) and its connectivity joins the two fills along that edge:

- `d-zone-join`: nothing else. KiCad 10.0.7 reports no `isolated_copper`;
- `e-zone-cut`: plus the route's `+5V` In2.Cu track through (138.6624, 95.5176), (135.8983,
  95.5176), (131.7201, 91.3394), (131.7201, 90.6106), (132.2356, 90.0951), (132.9644, 90.0951),
  (133.4799, 90.6106) and (137.7939, 90.6107) mm (0.1524 mm), its ends here two `+5V`
  through-hole pads (TP3, TP4). It cuts the lower zone's fill between the higher zone and the pad,
  and KiCad reports the priority-34 zone `isolated_copper` (the route's own, at (134.1, 93.5)
  mm).

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


def pad(ref, x, y, net="+3V"):
    return f'''\t(footprint "k1542:TP" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" thru_hole circle (at 0 0) (size 0.6 0.6) (drill 0.3) (layers "*.Cu" "*.Mask")
\t\t\t(net "{net}") (uuid "{u()}"))
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


# K15.45: V+'s two In2.Cu zones (moteus), joined along the higher one's edge, and cut by +5V.
def v_zone(priority, clearance, removal, pts):
    prio = f"\t\t(priority {priority})\n" if priority else ""
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "V+")
\t\t(layer "In2.Cu")
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
{prio}\t\t(connect_pads yes
\t\t\t(clearance {clearance})
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode {removal})
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''


def join_board(cut):
    uid[0] = 0x100
    body = (
        f'\t(gr_rect (start 128 86) (end 142 100) (stroke (width 0.05) (type default)) (fill no) '
        f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
        + pad("C1", 135.3, 89.375, "V+")
        + v_zone(0, 0.5, 0, [(131.2, 88), (140, 88), (140, 96.6), (137, 93.8), (131.2, 93.8)])
        + v_zone(34, 0.2, 1, [(134.1, 93.5), (131.2, 93.5), (131.2, 97.3), (134.1, 97.3)])
    )
    if cut:
        chain = [(138.6624, 95.5176), (135.8983, 95.5176), (131.7201, 91.3394), (131.7201, 90.6106),
                 (132.2356, 90.0951), (132.9644, 90.0951), (133.4799, 90.6106), (137.7939, 90.6107)]
        body += pad("TP3", *chain[0], "+5V") + pad("TP4", *chain[-1], "+5V")
        for (x0, y0), (x1, y1) in zip(chain, chain[1:]):
            body += (f'''\t(segment (start {x0} {y0}) (end {x1} {y1}) (width 0.1524) (layer "In2.Cu") '''
                     f'''(net "+5V") (uuid "{u()}"))\n''')
    return HEAD + body + ")\n"


for name, cut in (("d-zone-join", False), ("e-zone-cut", True)):
    with open(f"{name}.kicad_pcb", "w") as f:
        f.write(join_board(cut))


# K15.45: +3V's F.Cu zone of priority 41 and GND's of priority 42 (moteus, both outlines
# verbatim), GND's track from C9 pad 2 up through the +3V zone, and /aux2/AUX2P_C's via, all as on
# the pcb-on route (session 673d2823). Right of the track and above the GND zone the +3V zone
# leaves a 0.46 x 0.42 mm strip that KiCad's filler deletes as a blob (0.21 mm deflated, below
# the zone's 0.25 mm minimum thickness), so KiCad reports no island there.
def blob_zone(net, priority, pts):
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "{net}")
\t\t(layer "F.Cu")
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
\t\t(priority {priority})
\t\t(connect_pads yes
\t\t\t(clearance 0.1524)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 1)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''


def blob_board():
    uid[0] = 0x100
    body = (
        f'\t(gr_rect (start 157 104) (end 165 112) (stroke (width 0.05) (type default)) (fill no) '
        f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
        + pad("C9", 160.55, 107.67, "+3V")
        + pad("C9B", 161.51, 107.67, "GND")
        + blob_zone("+3V", 41, [(158.875, 107.975), (159.925, 106.675), (162.2, 106.7),
                                (162.2, 110.55), (158.875, 110.525)])
        + blob_zone("GND", 42, [(161.3, 107.275), (162.7, 107.275), (162.7, 109.5), (161.3, 109.5)])
        + '\t(segment (start 161.51 107.67) (end 161.51 105.2524) (width 0.1524) (layer "F.Cu") '
        + f'(net "GND") (uuid "{u()}"))\n'
        + '\t(via (at 162.2531 106.2578) (size 0.458) (drill 0.254) (layers "F.Cu" "B.Cu") '
        + f'(net "/aux2/AUX2P_C") (uuid "{u()}"))\n'
    )
    return HEAD + body + ")\n"


with open("f-blob.kicad_pcb", "w") as f:
    f.write(blob_board())
