"""K15.38: the small board of `tests/k15_pour_keepout.rs`'s rule-area pour flag test.

Writes `c-pour-allowed-unfilled.kicad_pcb` into the current directory; `c-pour-allowed.kicad_pcb`
is that file after `kicad-cli pcb drc --refill-zones --save-board` (KiCad 10.0.7). A 30 x 20 mm
board with one F.Cu zone (`POUR`, x 2-28, y 2-18 mm, islands kept) and three rule areas on F.Cu:

- `R1`, the board's, x 5-11, y 5-15: tracks and vias not allowed, copper pour allowed (the
  reader writes a `keepout`);
- `R2`, footprint U1's (at (20, 10), 90 degrees), x 17-23, y 5-15: tracks not allowed, vias
  and copper pour allowed (a `wire_keepout`, as canary-pwr's two rule areas of this kind);
- `R3`, the board's, x 14-16, y 4-16: tracks, vias and copper pour not allowed.

KiCad fills `POUR` through R1 and R2 and cuts it at R3's outline.

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
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def rect(x0, y0, x1, y1):
    return " ".join(f"(xy {x} {y})" for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
def rule_area(indent, name, tracks, vias, pour, box):
    t = "\t" * indent
    return f'''{t}(zone (layer "F.Cu") (uuid "{u()}") (name "{name}") (hatch full 0.508)
{t}\t(connect_pads (clearance 0))
{t}\t(min_thickness 0.254)
{t}\t(keepout (tracks {tracks}) (vias {vias}) (pads allowed) (copperpour {pour}) (footprints allowed))
{t}\t(placement (enabled no) (sheetname ""))
{t}\t(fill (thermal_gap 0.508) (thermal_bridge_width 0.508) (island_removal_mode 0))
{t}\t(polygon (pts {rect(*box)}))
{t})
'''
BODY = (
    f'\t(gr_rect (start 0 0) (end 30 20) (stroke (width 0.05) (type default)) (fill no) '
    f'(layer "Edge.Cuts") (uuid "{u()}"))\n'
    + rule_area(1, "R1", "not_allowed", "not_allowed", "allowed", (5, 5, 11, 15))
    + f'''\t(footprint "k1538:U1" (layer "F.Cu") (uuid "{u()}") (at 20 10 90)
\t\t(property "Reference" "U1" (at 0 0 90) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 90) (layer "F.Fab") (uuid "{u()}"))
'''
    + rule_area(2, "R2", "not_allowed", "allowed", "allowed", (17, 5, 23, 15))
    + "\t)\n"
    + rule_area(1, "R3", "not_allowed", "not_allowed", "not_allowed", (14, 4, 16, 16))
    + f'''\t(zone
\t\t(net "POUR")
\t\t(layer "F.Cu")
\t\t(uuid "{u()}")
\t\t(name "POUR")
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
\t\t\t\t{rect(2, 2, 28, 18)}
\t\t\t)
\t\t)
\t)
'''
)
with open("c-pour-allowed-unfilled.kicad_pcb", "w") as f:
    f.write(HEAD + BODY + ")\n")
