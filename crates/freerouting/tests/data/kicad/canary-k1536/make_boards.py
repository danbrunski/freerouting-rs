"""K15.36: the small boards of `tests/k15_junction_stub.rs`, cut from canary-pwr's route.

Every coordinate is canary-pwr's (`95882f00`) after K15.31's `.kicad_pcb` route with
`router.pour_fill_contacts=true` and `router.pour_fill_target=true`
(`target/orchestrator/k1531/runs/rebased-canary/contacts-target`, session `cce88912...`), where
KiCad 10.0.7 reports `track_dangling` on a 0.0725 mm B.Cu stub of `/Supervisor/CAN_EN`: the
B.Cu copper of the net from U7 pad 6 (SMD, 0.6 x 1.95 mm at (143.54, 82.125)) to the via at
(151.7716, 51.5288), in the route's wires:

- the wire from U7 pad 6 to (144.9009, 79.175), width 0.2;
- the neckdown from there to the junction (144.777, 79.0511), width 0.15;
- the wire from the via down x 151.7716 and diagonally to the junction, width 0.2;
- the stub from the junction to (144.7257, 79.1024), width 0.2, on the line of the diagonal.

The segments are written so that the KiCad reader (`freerouting-kicad/src/tracks.rs`, KiCad's
export merge) makes each wire one trace, as the route had them: the via's wire, the neckdown,
the stub (its width differs from the neckdown's, so a new wire starts), U7's wire.

- `a-stub-at-a-junction`: that copper. KiCad 10.0.7 (`kicad-cli pcb drc --severity-all`)
  reports the stub `track_dangling` (and the via `via_dangling`: the board has none of its other
  layers' copper);
- `b-stub-end-on-a-track`: the same and a B.Cu track of the net from the stub's end
  (144.7257, 79.1024) to a test pad at (144.0257, 79.1024) (not in the route). KiCad reports no
  `track_dangling`.

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
\t(gr_rect (start 140 50) (end 155 84) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e1"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def smd(ref, x, y, net, w, h):
    return f'''\t(footprint "k1536:smd" (layer "B.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "B.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "B.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size {w} {h}) (layers "B.Cu" "B.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def wire(points, w):
    return "".join(
        f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width {w}) (layer "B.Cu") (net "{NET}") (uuid "{u()}"))\n'
        for (x0, y0), (x1, y1) in zip(points, points[1:]))
def via(x, y):
    return f'\t(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "{NET}") (uuid "{u()}"))\n'
NET = "/Supervisor/CAN_EN"
JUNCTION = (144.777, 79.0511)
STUB_END = (144.7257, 79.1024)
VIA_WIRE = [(151.7716, 51.5288), (151.7716, 72.0565), JUNCTION]
NECKDOWN = [(144.9009, 79.175), JUNCTION]
STUB = [JUNCTION, STUB_END]
U7_WIRE = [(143.54, 82.125), (143.54, 81.7967), (144.54, 80.7967), (144.6351, 80.7967),
           (145.098, 80.3338), (145.098, 79.3721), (144.9009, 79.175)]
TEST_PAD = (144.0257, 79.1024)
def parts(extra_track):
    out = [smd("U7", 143.54, 82.125, NET, 0.6, 1.95), via(*VIA_WIRE[0])]
    out += [wire(VIA_WIRE, 0.2), wire(NECKDOWN, 0.15), wire(STUB, 0.2), wire(U7_WIRE, 0.2)]
    if extra_track:
        out += [smd("TP1", *TEST_PAD, NET, 0.6, 0.6), wire([STUB_END, TEST_PAD], 0.2)]
    return out
for name, extra in [("a-stub-at-a-junction", False), ("b-stub-end-on-a-track", True)]:
    uid[0] = 0x100
    with open(f"{name}.kicad_pcb", "w") as f:
        f.write(HEAD + "".join(parts(extra)) + ")\n")
