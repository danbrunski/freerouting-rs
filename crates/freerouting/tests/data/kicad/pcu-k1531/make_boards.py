"""K15.31: the small boards of `tests/k15_pour_fill_contacts.rs`, cut from the pcu board.

Writes `<name>-unfilled.kicad_pcb` into the current directory; `<name>.kicad_pcb` is each one
after `kicad-cli pcb drc --severity-all --format json --refill-zones --save-board` (KiCad
10.0.7), with the pcu board's `.kicad_dru` beside it (K15.23's copy, as `<name>.kicad_dru` and
`<name>-unfilled.kicad_dru`). Every coordinate of the zone and the copper is the pcu board's
after K15.25's `.kicad_pcb` route with `router.pour_fill_target=true`
(`target/orchestrator/k1525/runs/b/final-true`, session `a1118e6c...`), where KiCad reports
`/CH2_NO`'s U4 pad 10 and pad 8 unconnected, and zone EFUSE_NO_2 unconnected to itself: zone
EFUSE_NO_2 (`/CH2_NO`, F.Cu, priority 48, clearance 0.5, min_thickness 0.5, pads connected solid,
island_removal_mode 0) verbatim, the routed F.Cu tracks of `/CH2_NO` (U4 pad 8 to (134.3849,
96.6742), U4 pad 10 to (138.049, 102.1244)), of `VCC_3V3` (C11 pad 2 down to (134.7427,
100.8994), on along x 134.7427 and y 104.0325, and along y 101.4152 from x 129.4945), of
`BUS_GND` and of `/CH2_SENSE` near the zone, and the pads within 1 mm of it, each the pcu pad's
bounding box as the router reads it (U4 pads 6-10, C11, U17 pads 17-20 and its exposed pad, C54
pad 1, R85 pad 2, R8 pad 1).

- `a-pads-in-three-islands`: all of it. `VCC_3V3`'s tracks cut EFUSE_NO_2's fill in three: the
  top left part with U17's pads 18-20, R85 pad 2 and the end of U4 pad 8's track; the bottom left
  part with C54 pad 1; the right part with R8 pad 1 and the end of U4 pad 10's track;
- `b-no-vcc-cut`: the same without `VCC_3V3`'s tracks inside the zone (the diagonal from C11 pad 2
  and every track after it): EFUSE_NO_2's fill is one piece.

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
\t(gr_rect (start 129 93) (end 144 106) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e1"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def smd(ref, x, y, net, w, h):
    return f'''\t(footprint "k1531:smd" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "F.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "F.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size {w} {h}) (layers "F.Cu" "F.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def seg(x0, y0, x1, y1, net, layer="F.Cu", w=0.2):
    return f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width {w}) (layer "{layer}") (net "{net}") (uuid "{u()}"))\n'
def via(x, y, net):
    return f'\t(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "{net}") (uuid "{u()}"))\n'
def zone(name, layer, priority, outline):
    xy = " ".join(f"(xy {x} {y})" for x, y in outline)
    return f'''\t(zone
\t\t(net "{NO}")
\t\t(layer "{layer}")
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
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{xy}
\t\t\t)
\t\t)
\t)
'''
NO = "/CH2_NO"
VCC = "VCC_3V3"
GND = "BUS_GND"
SENSE = "/CH2_SENSE"
EFUSE_NO_2 = [(132.877913, 96.291089), (132.877913, 98.341089), (132.6, 98.341089), (132.6, 103.341089),
              (135.027913, 103.341089), (135.038103, 104.522695), (139.684852, 104.482865),
              (139.647704, 100.148953), (135.677913, 100.151031), (135.677913, 96.291089)]
PADS = [("U4", 136.3428, 96.3, VCC, 0.3, 1.45), ("U4", 136.8428, 96.3, GND, 0.3, 1.45),
        ("U4", 137.3428, 96.3, NO, 0.3, 1.45), ("U4", 137.8428, 96.3, SENSE, 0.3, 1.45),
        ("U4", 138.3428, 96.3, NO, 0.3, 1.45), ("C11", 138.8928, 98.6493, GND, 1.0, 1.45),
        ("C11", 136.9928, 98.6493, VCC, 1.0, 1.45), ("U17", 132.575, 97.1028, GND, 0.3556, 1.4097),
        ("U17", 133.225, 97.1028, NO, 0.3556, 1.4097), ("U17", 133.875, 97.1028, NO, 0.3556, 1.4097),
        ("U17", 134.525, 97.1028, NO, 0.3556, 1.4097), ("U17", 131.6, 94.15, GND, 2.9718, 2.9718),
        ("C54", 133.6678, 102.5993, NO, 1.15, 1.8), ("R85", 133.1553, 99.8993, NO, 1.025, 1.4),
        ("R8", 136.3303, 102.0493, NO, 1.225, 3.35)]
TRACKS = [(136.8428, 97.0219, 136.8428, 96.3, GND), (132.575, 97.1027, 132.575, 98.5604, GND),
          (138.8928, 98.6493, 138.0145, 97.771, GND), (137.3781, 97.5572, 136.8428, 97.0219, GND),
          (138.0145, 97.771, 138.0145, 97.5572, GND), (138.0145, 97.5572, 137.3781, 97.5572, GND),
          (132.575, 98.5604, 132.6377, 98.6231, GND),
          (136.3428, 97.9993, 136.3428, 96.3, VCC), (136.9928, 98.6493, 136.3428, 97.9993, VCC),
          (137.3428, 95.5735, 137.3428, 96.3, NO), (139.7159, 100.4575, 139.7159, 97.6731, NO),
          (139.7159, 97.6731, 138.3428, 96.3, NO), (134.3849, 96.6742, 135.825, 95.2341, NO),
          (135.825, 95.2341, 137.0034, 95.2341, NO), (138.049, 102.1244, 139.7159, 100.4575, NO),
          (137.0034, 95.2341, 137.3428, 95.5735, NO),
          (137.8428, 95.5809, 137.8428, 96.3, SENSE), (143.5799, 100.2997, 138.5808, 95.3006, SENSE),
          (138.1231, 95.3006, 137.8428, 95.5809, SENSE), (138.5808, 95.3006, 138.1231, 95.3006, SENSE)]
# VCC_3V3's tracks inside EFUSE_NO_2, from C11 pad 2 on.
CUT = [(134.7427, 100.8994, 136.9928, 98.6493, VCC), (134.7427, 101.4152, 134.7427, 100.8994, VCC),
       (134.7427, 101.4152, 134.7427, 102.9618, VCC), (129.4945, 101.4152, 134.7427, 101.4152, VCC),
       (134.7427, 102.9618, 135.8134, 104.0325, VCC), (135.8134, 104.0325, 142.7738, 104.0325, VCC)]
GND_VIA = (138.0145, 97.5572)
def parts(cut):
    out = [smd(ref, x, y, net, w, h) for ref, x, y, net, w, h in PADS]
    out += [seg(*t) for t in TRACKS + (CUT if cut else [])]
    out += [via(*GND_VIA, GND), zone("EFUSE_NO_2", "F.Cu", 48, EFUSE_NO_2)]
    return out
for name, cut in [("a-pads-in-three-islands", True), ("b-no-vcc-cut", False)]:
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        f.write(HEAD + "".join(parts(cut)) + ")\n")
