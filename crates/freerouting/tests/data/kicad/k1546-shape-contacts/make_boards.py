"""K15.46: which fill a pad, a via and a track join (`router.pour_fill_contacts`).

Writes `meets.kicad_pcb` and `gaps.kicad_pcb` into the current directory. Each board has one
case per net Nk: a zone of Nk on F.Cu (priority 0, clearance 0.2 mm, minimum thickness 0.25 mm,
`island_removal_mode` 1) over x x0..x0+5, y 10..14 mm with x0 = 10 + 8k; a witness SMD pad W<k>
of Nk (1 x 1 mm) at the zone's centre; and the case's item of Nk (with an SMD pad PN<k> at a
track's far end). KiCad 10.0.7 (`pcb drc --refill-zones`, and pcbnew's connectivity after a
refill) joins every item of `meets` to its zone and no item of `gaps`:

`meets`: N0 a 1 x 1 mm pad whose centre lies 0.3 mm left of the outline (0.2 mm of it inside);
N1 the same 0.45 mm left (0.05 mm inside); N2 a via (0.6 mm, drill 0.3 mm) 0.2 mm left; N3 a
0.25 mm track ending 0.1 mm left of the outline; N4 a 0.25 mm track across the zone, both ends
outside; N5 a 0.5 mm pad, N6 a via and N7 a track end at (x0 + 0.02, 10.02), inside the outline
but outside its fill's rounded corner; N8 a track ending 0.5 mm inside (Java's contact as well).

`gaps`: N0 the pad 0.05 mm clear of the outline; N1 the via and N2 the track end 0.05 mm clear;
N3 the pad of N0 in `meets` with a 0.25 mm track of net X at x0 + 0.5 that takes the fill next to
it; N4 a via 0.05 mm inside the outline, cut from the zone's fill (an island of its own) by a
0.25 mm track of net X at x0 + 0.85.

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
\t\t(2 "B.Cu" signal)
\t\t(1 "F.Mask" user)
\t\t(3 "B.Mask" user)
\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(25 "Edge.Cuts" user)
\t\t(35 "F.Fab" user)
\t)
\t(setup (pad_to_mask_clearance 0))
'''
uid = [0x100]


def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]


def smd(ref, x, y, net, w=1.0, h=1.0):
    return f'''\t(footprint "k1546:{ref}" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size {w} {h}) (layers "F.Cu" "F.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''


def via(x, y, net):
    return (f'\t(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "{net}") '
            f'(uuid "{u()}"))\n')


def seg(a, b, net, w=0.25):
    return (f'\t(segment (start {a[0]} {a[1]}) (end {b[0]} {b[1]}) (width {w}) (layer "F.Cu") '
            f'(net "{net}") (uuid "{u()}"))\n')


def zone(net, x0, y0, x1, y1):
    xy = " ".join(f"(xy {x} {y})" for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
    return f'''\t(zone (net "{net}") (layer "F.Cu") (uuid "{u()}") (hatch edge 0.5) (priority 0)
\t\t(connect_pads yes (clearance 0.2)) (min_thickness 0.25)
\t\t(fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
\t\t(polygon (pts {xy}))
\t)
'''


def r(v):
    return round(v, 4)


MEETS = [
    lambda n, x0: smd(f"P{n}", r(x0 - 0.3), 12, n),
    lambda n, x0: smd(f"P{n}", r(x0 - 0.45), 12, n),
    lambda n, x0: via(r(x0 - 0.2), 12, n),
    lambda n, x0: smd(f"P{n}", x0 - 2, 12, n, 0.5, 0.5) + seg((x0 - 2, 12), (r(x0 - 0.1), 12), n),
    lambda n, x0: smd(f"P{n}", x0 + 1, 8.5, n, 0.5, 0.5) + seg((x0 + 1, 8.5), (x0 + 1, 15.5), n),
    lambda n, x0: smd(f"P{n}", r(x0 + 0.02), 10.02, n, 0.5, 0.5),
    lambda n, x0: via(r(x0 + 0.02), 10.02, n),
    lambda n, x0: smd(f"P{n}", x0 - 1, 9, n, 0.5, 0.5) + seg((x0 - 1, 9), (r(x0 + 0.02), 10.02), n),
    lambda n, x0: smd(f"P{n}", x0 - 2, 12, n, 0.5, 0.5) + seg((x0 - 2, 12), (x0 + 0.5, 12), n),
]
GAPS = [
    lambda n, x0: smd(f"P{n}", r(x0 - 0.55), 12, n),
    lambda n, x0: via(r(x0 - 0.35), 12, n),
    lambda n, x0: smd(f"P{n}", x0 - 2, 12, n, 0.5, 0.5) + seg((x0 - 2, 12), (r(x0 - 0.175), 12), n),
    lambda n, x0: smd(f"P{n}", r(x0 - 0.3), 12, n) + seg((x0 + 0.5, 9), (x0 + 0.5, 15), "X"),
    lambda n, x0: via(r(x0 + 0.05), 12, n) + seg((x0 + 0.85, 9), (x0 + 0.85, 15), "X"),
]


def board(cases):
    uid[0] = 0x100
    body = (f'\t(gr_rect (start 5 5) (end {10 + 8 * len(cases)} 19) (stroke (width 0.05) '
            f'(type default)) (fill no) (layer "Edge.Cuts") (uuid "{u()}"))\n')
    for k, case in enumerate(cases):
        x0 = 10 + 8 * k
        body += zone(f"N{k}", x0, 10, x0 + 5, 14) + smd(f"W{k}", x0 + 2.5, 12, f"N{k}") + case(f"N{k}", x0)
    return HEAD + body + ")\n"


for name, cases in [("meets", MEETS), ("gaps", GAPS)]:
    with open(f"{name}.kicad_pcb", "w") as f:
        f.write(board(cases))
