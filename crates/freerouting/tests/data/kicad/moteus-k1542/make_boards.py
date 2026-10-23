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

K15.46 adds `g-open-trade`: GND's F.Cu zone of priority 40, C11 pad 2 and `/Current
Sense/FCUR3`'s route from the zone-join repair's `pcb-on` route (session `dd559bbb`), and a +3V
zone of the test's own that FCUR3's track cuts between two +3V pads (the comment above
`trade_board` names every coordinate).

K15.46's remainder adds `h-pad-shape` and `h-pad-gap`: GND's B.Cu zone of priority 46 (moteus,
outline verbatim), U2 pad 2 (GND; the footprint at (135.2, 106.75) rotated 180, the pad at
(-1.1375, 0), 1.325 x 0.6 mm roundrect) and, standing for a GND item in the zone, the GND via
(138.027, 107.9238) mm (0.458 mm, drill 0.254 mm) of moteus's `pcb-off` route (session
`9f998f1d`). U2 pad 2's centre (136.3375, 106.75) lies 0.0625 mm left of the zone's outline,
its copper 0.6 mm into it (the `pcb-on` routes `f286d7f6` and `daff0cba` have it so). In
`h-pad-gap` the footprint lies 0.7 mm further left, its pad clear of the outline.

K15.46's third part adds `i-via-pocket`: the +3V via (160.9345, 105.914) mm that KiCad reports
`via_dangling` on the pair's `pcb-on` route of main `eb48beb` (session `d124cb86`), with every
item of that route on In2.Cu within 1.6 mm of it verbatim (the GND via (161.5387, 105.2261), the
`/Current Sense/FCUR3` via (160.4718, 105.4663) and its In2.Cu track, the `/aux1/AS5047_CS` via
(160.3447, 106.974), the `/aux2/AUX2P_C` via (162.2637, 106.2148) and six `+5V` In2.Cu
segments; vias 0.458 mm, drill 0.254 mm, tracks 0.1524 mm), the via's three `+3V` F.Cu
segments, and +3V's In2.Cu zone of priority 1 (clearance 0.3 mm, minimum thickness 0.25 mm,
`island_removal_mode` 0, as on moteus) with its outline cut to x 159.3-162.6, y 103.6-107.6 mm.
The zone's 0.3 mm closes the channel between the GND and FCUR3 vias (1.09 mm apart: 0.036 mm
of fill, below the minimum thickness), so KiCad's fill leaves the via's pocket empty and the via
joins copper on F.Cu alone. The test's own items: a +3V through-hole pad (TP7, 0.6 mm, drill
0.3 mm) at (159.8, 104.1) mm in the fill's main part, which joins it to a pad as moteus's pads
do, and one 0.3 mm F.Cu pad per net at y 103.8 mm, x 156-158.5 mm, clear of the zone,
so that each net has a pin. `i-via-pocket.kicad_pro` holds moteus's Default net class (clearance
0.127 mm, track 0.1524 mm, via 0.458 mm, drill 0.254 mm) and its board minimums, so the router's
clearance is moteus's: with it the channel holds 0.38 mm of fill.

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


# K15.46: GND's F.Cu zone of priority 40 and +3V's In2.Cu zone of priority 44 (moteus, both
# outlines verbatim), C11 pad 2 (GND), /Current Sense/FCUR3's route from U3 pad 18 to R15 pad 2
# and two +3V vias' places, all as on the pcb-on route of K15.46's zone-join repair before its
# FCUR3 trial (session dd559bbb, the board the repair left after two kept trials). FCUR3's F.Cu
# track cuts the GND zone's fill in two, the part right of it holding no GND item. On moteus
# FCUR3's removal also closed a +3V connection (+3V 4 -> 3 in the trial's log) through fills this
# board does not carry; here a +3V zone of the test's own (priority 44, F.Cu, its corners taken
# from moteus's coordinates: x 151.5013 of a +3V via, x 153.0281 of a GND track corner, y 109.2
# and 111.375 of the GND zone's corners) stands for them: FCUR3's F.Cu track and via cut it
# between two +3V through-hole pads at moteus's +3V pad centres (152.38, 109.59) and
# (152.5299, 110.6104) mm (0.458 mm, drill 0.254 mm, a via's size).
def smd(ref, x, y, net, layer, w, h, rot=0):
    side = "B" if layer == "B.Cu" else "F"
    return f'''\t(footprint "k1546:{ref}" (layer "{layer}") (uuid "{u()}") (at {x} {y} {rot})
\t\t(property "Reference" "{ref}" (at 0 0 {rot}) (layer "{side}.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 {rot}) (layer "{side}.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0 {rot}) (size {w} {h}) (layers "{layer}" "{side}.Mask")
\t\t\t(net "{net}") (uuid "{u()}"))
\t)
'''


def th(ref, x, y, net, size, drill):
    return f'''\t(footprint "k1546:{ref}" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" thru_hole circle (at 0 0) (size {size} {size}) (drill {drill}) (layers "*.Cu" "*.Mask")
\t\t\t(net "{net}") (uuid "{u()}"))
\t)
'''


def zone(net, layer, priority, clearance, pts):
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "{net}")
\t\t(layer "{layer}")
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
\t\t(priority {priority})
\t\t(connect_pads yes
\t\t\t(clearance {clearance})
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


FCUR3 = "/Current Sense/FCUR3"
FCUR3_TRACKS = {
    "F.Cu": [((153.8773, 109.9167), (153.5988, 110.1952)), ((153.5988, 110.1952), (151.1114, 110.1952)),
             ((153.8773, 109.2551), (153.8773, 109.9167)), ((153.107, 108.4848), (153.8773, 109.2551)),
             ((152.46, 108.4848), (153.107, 108.4848))],
    "B.Cu": [((150.8152, 110.1952), (150.0006, 109.3806)), ((159.7007, 106.8357), (159.7007, 106.8346)),
             ((159.6152, 107.0606), (159.6151, 107.0606)), ((159.7084, 106.8269), (159.7084, 106.8192)),
             ((159.7007, 106.8346), (159.7084, 106.8269)), ((159.1829, 107.4929), (159.6152, 107.0606)),
             ((159.6151, 107.0606), (159.6151, 106.9213)), ((150.0006, 109.8694), (150.0006, 109.3806)),
             ((149.45, 110.42), (150.0006, 109.8694)), ((159.6151, 106.9213), (159.7007, 106.8357)),
             ((151.1114, 110.1952), (150.8152, 110.1952))],
    "In2.Cu": [((159.7084, 106.8192), (158.0428, 108.4848)), ((158.0428, 108.4848), (152.46, 108.4848))],
}
FCUR3_VIAS = [(151.1114, 110.1952), (159.7084, 106.8192), (152.46, 108.4848)]


def trade_board():
    uid[0] = 0x100
    body = (
        f'\t(gr_rect (start 147 102) (end 161 112) (stroke (width 0.05) (type default)) (fill no) '
        f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
        + smd("C11", 153.34, 109.59, "GND", "F.Cu", 0.56, 0.62)
        + smd("U3", 159.1829, 107.4929, FCUR3, "B.Cu", 0.3048, 0.8128, -45)
        + smd("R15", 150.0006, 109.3806, FCUR3, "B.Cu", 0.54, 0.64, -45)
        + th("TP5", 152.38, 109.59, "+3V", 0.458, 0.254)
        + th("TP6", 152.5299, 110.6104, "+3V", 0.458, 0.254)
        + zone("GND", "F.Cu", 40, 0.2, [(154.425, 109.2), (153.125, 109.2), (153.125, 110.1),
                                         (154.425, 111.375)])
        + zone("+3V", "F.Cu", 44, 0.1524, [(151.5013, 109.2), (153.0281, 109.2), (153.0281, 111.375),
                                            (151.5013, 111.375)])
    )
    for layer, tracks in FCUR3_TRACKS.items():
        for (x0, y0), (x1, y1) in tracks:
            body += (f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width 0.1524) (layer "{layer}") '
                     f'(net "{FCUR3}") (uuid "{u()}"))\n')
    for x, y in FCUR3_VIAS:
        body += (f'\t(via (at {x} {y}) (size 0.458) (drill 0.254) (layers "F.Cu" "B.Cu") '
                 f'(net "{FCUR3}") (uuid "{u()}"))\n')
    return HEAD + body + ")\n"


with open("g-open-trade.kicad_pcb", "w") as f:
    f.write(trade_board())


# K15.46's remainder: KiCad joins a pad to a fill its copper meets, also where the pad's centre
# lies outside the zone (the docstring names every coordinate).
def sot_pad2(x, y):
    return f'''\t(footprint "Package_TO_SOT_SMD:SOT-23-5" (layer "B.Cu") (uuid "{u()}") (at {x} {y} 180)
\t\t(property "Reference" "U2" (at 0 0 180) (layer "B.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 180) (layer "B.Fab") (uuid "{u()}"))
\t\t(pad "2" smd roundrect (at -1.1375 0 180) (size 1.325 0.6) (layers "B.Cu" "B.Mask")
\t\t\t(roundrect_rratio 0.25) (net "GND") (uuid "{u()}"))
\t)
'''


def pad_shape_board(x):
    uid[0] = 0x100
    body = (
        f'\t(gr_rect (start 132 104) (end 140 110) (stroke (width 0.05) (type default)) (fill no) '
        f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
        + sot_pad2(x, 106.75)
        + f'''\t(zone
\t\t(net "GND")
\t\t(layer "B.Cu")
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
\t\t(priority 46)
\t\t(connect_pads yes
\t\t\t(clearance 0.1524)
\t\t)
\t\t(min_thickness 0.1)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t(xy 138.25 106.4) (xy 136.4 106.4) (xy 136.4 107.2) (xy 137.2 107.2) (xy 137.2 108) (xy 138.25 108)
\t\t\t)
\t\t)
\t)
'''
        + f'\t(via (at 138.027 107.9238) (size 0.458) (drill 0.254) (layers "F.Cu" "B.Cu") (net "GND") '
        f'(uuid "{u()}"))\n'
    )
    return HEAD + body + ")\n"


with open("h-pad-shape.kicad_pcb", "w") as f:
    f.write(pad_shape_board(135.2))
with open("h-pad-gap.kicad_pcb", "w") as f:
    f.write(pad_shape_board(134.5))


# K15.46's third part: the +3V via KiCad reports `via_dangling` on moteus's pair route
# `d124cb86` (the docstring names every coordinate).
POCKET_VIAS = [("GND", 161.5387, 105.2261), ("+3V", 160.9345, 105.914),
               ("/Current Sense/FCUR3", 160.4718, 105.4663), ("/aux1/AS5047_CS", 160.3447, 106.974),
               ("/aux2/AUX2P_C", 162.2637, 106.2148)]
POCKET_TRACKS = [
    ("+5V", "In2.Cu", (162.0043, 105.5053), (161.0624, 106.4472)),
    ("+5V", "In2.Cu", (162.0043, 105.0144), (162.0043, 105.5053)),
    ("+5V", "In2.Cu", (160.1809, 106.4472), (159.8351, 106.793)),
    ("+5V", "In2.Cu", (161.0624, 106.4472), (160.1809, 106.4472)),
    ("+5V", "In2.Cu", (159.8351, 106.793), (159.8351, 107.2156)),
    ("+5V", "In2.Cu", (161.1797, 104.1898), (162.0043, 105.0144)),
    ("/Current Sense/FCUR3", "In2.Cu", (160.4718, 105.4663), (156.0585, 109.8796)),
    ("+3V", "F.Cu", (160.745, 106.1007), (160.9317, 105.914)),
    ("+3V", "F.Cu", (160.745, 106.7142), (160.745, 106.1007)),
    ("+3V", "F.Cu", (160.9317, 105.914), (160.9317, 105.0037)),
    ("+3V", "F.Cu", (160.9317, 105.914), (160.9345, 105.914)),
]


def pocket_board():
    uid[0] = 0x100
    body = (
        f'\t(gr_rect (start 155.5 103) (end 163.5 110.5) (stroke (width 0.05) (type default)) '
        f'(fill no) (layer "Edge.Cuts") (uuid "{u()}"))\n'
        + th("TP7", 159.8, 104.1, "+3V", 0.6, 0.3)
        + "".join(smd(f"N{k}", 156 + 0.5 * k, 103.8, net, "F.Cu", 0.3, 0.3)
                  for k, net in enumerate(["GND", "+5V", "/Current Sense/FCUR3", "/aux1/AS5047_CS",
                                           "/aux2/AUX2P_C", "+3V"]))
        + f'''\t(zone
\t\t(net "+3V")
\t\t(layer "In2.Cu")
\t\t(uuid "{u()}")
\t\t(hatch edge 0.5)
\t\t(priority 1)
\t\t(connect_pads yes
\t\t\t(clearance 0.3)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{rect(159.3, 103.6, 162.6, 107.6)}
\t\t\t)
\t\t)
\t)
'''
    )
    for net, layer, (x0, y0), (x1, y1) in POCKET_TRACKS:
        body += (f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width 0.1524) (layer "{layer}") '
                 f'(net "{net}") (uuid "{u()}"))\n')
    for net, x, y in POCKET_VIAS:
        body += (f'\t(via (at {x} {y}) (size 0.458) (drill 0.254) (layers "F.Cu" "B.Cu") '
                 f'(net "{net}") (uuid "{u()}"))\n')
    return HEAD + body + ")\n"


with open("i-via-pocket.kicad_pcb", "w") as f:
    f.write(pocket_board())


# moteus's Default net class and board minimums (`moteus_c1.kicad_pro`).
POCKET_PROJECT = {
    "board": {"design_settings": {"rules": {
        "min_clearance": 0.127, "min_copper_edge_clearance": 0.254, "min_hole_clearance": 0.254,
        "min_through_hole_diameter": 0.254, "min_track_width": 0.127, "min_via_annular_width": 0.1016,
        "min_via_diameter": 0.381}}},
    "meta": {"filename": "i-via-pocket.kicad_pro", "version": 3},
    "net_settings": {"classes": [{"name": "Default", "clearance": 0.127, "track_width": 0.1524,
                                  "via_diameter": 0.458, "via_drill": 0.254,
                                  "microvia_diameter": 0.381, "microvia_drill": 0.254,
                                  "diff_pair_width": 0.1524, "diff_pair_gap": 0.254,
                                  "priority": 2147483647}],
                     "meta": {"version": 4}},
}
with open("i-via-pocket.kicad_pro", "w") as f:
    json.dump(POCKET_PROJECT, f, indent=2)
    f.write("\n")
