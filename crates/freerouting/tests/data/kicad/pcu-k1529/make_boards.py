"""K15.29: the small boards of `tests/k15_pour_island_via.rs`, cut from the pcu board.

Writes `<name>-unfilled.kicad_pcb` into the current directory; `<name>.kicad_pcb` is each one
after `kicad-cli pcb drc --severity-all --format json --refill-zones --save-board` (KiCad
10.0.7), with the pcu board's `.kicad_dru` beside it (K15.23's copy, as `<name>.kicad_dru` and
`<name>-unfilled.kicad_dru`). Every coordinate of the zones and the copper is the pcu board's
after K15.27's DSN route (`target/orchestrator/k1527/runs/pcu-dsn-off-dflt`, session
`55618a37...`), where KiCad reports `via_dangling` for `Net-(U24-OUT)`'s via at (61.2182,
118.9218): zones USB_OUT (`Net-(U24-OUT)`, F.Cu, priority 37) and USB_OUT_POUR (B.Cu, priority
68) verbatim (clearance 0.5, min_thickness 0.5, pads connected solid, island_removal_mode 0),
that via, `BUS_GND`'s routed F.Cu track along y 120.4697 with its via at (59.2922, 120.4697), and
`Net-(J13-CC2)`'s F.Cu diagonal from (58.175, 119.375) to (60.6, 116.95). The pads are this
script's, each the pcu pad's bounding box as the router reads it: U24's pad 3 (`Net-(U24-OUT)`,
2.1 x 0.4 mm at (57.6, 120.45)) and pad 4 (`Net-(J13-CC2)`), C72's pad 2 (`Net-(J13-CC2)`) and
J1 (`BUS_GND`, at the track's far end).

- `a-via-island-cut-off`: all of it. The `BUS_GND` track and via cut USB_OUT's fill in two: the
  part with U24's pad 3 (x 56.4 to 58.9) and the part with the via (x 59 to 61.6), which no pad
  reaches, so KiCad's filler removes it. USB_OUT_POUR's B.Cu fill has no pad at all, so KiCad
  keeps it; the via is connected on B.Cu only;
- `b-no-bus-gnd`: the same without `BUS_GND`'s track and via (J1 stays): USB_OUT's fill is one
  piece with the pad and the via, and the via is connected on both layers.

`<name>.dsn` is KiCad 10.0.7's Specctra export of `<name>.kicad_pcb`
(`tools/kicad-reader/export_dsn.py`), its first line's path cut to the file name. KiCad's
filler keeps the `.kicad_dru`'s unconditional 0.127 mm clearance rule, not the zones' 0.5 mm:
refilled without the `.kicad_dru`, `b-no-bus-gnd`'s USB_OUT fill is 0.893 mm2, with it 7.701 mm2.

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
\t(gr_rect (start 53 114) (end 82 142) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e0"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def smd(ref, x, y, net, w, h):
    return f'''\t(footprint "k1529:smd" (layer "F.Cu") (uuid "{u()}") (at {x} {y})
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
\t\t(net "{OUT}")
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
OUT = "Net-(U24-OUT)"
GND = "BUS_GND"
CC2 = "Net-(J13-CC2)"
USB_OUT = [(56.4, 120.800001), (56.4, 120.000001), (59, 120.000001), (59, 118.600001), (61.6, 118.600001),
           (61.6, 120.800001), (58.65, 120.800001)]
USB_OUT_POUR = [(59, 117.575), (58.927722, 122.589654), (75.24232, 139.381424), (79.542907, 136.179575),
                (61.622919, 117.620922)]
def parts(bus_gnd):
    out = [smd("U24", 57.6, 120.45, OUT, 2.1, 0.4), smd("U25", 58.2, 119.375, CC2, 0.9, 0.95),
           smd("C72", 60.6, 116.95, CC2, 1.0, 1.0), smd("J1", 63.45, 119.275, GND, 0.6, 0.6)]
    out += [seg(57.9, 119.375, 58.175, 119.375, CC2), seg(58.175, 119.375, 60.6, 116.95, CC2),
            via(61.2182, 118.9218, OUT)]
    if bus_gnd:
        out += [seg(59.2922, 120.4697, 62.2553, 120.4697, GND), seg(62.2553, 120.4697, 63.45, 119.275, GND),
                via(59.2922, 120.4697, GND)]
    out += [zone("USB_OUT", "F.Cu", 37, USB_OUT), zone("USB_OUT_POUR", "B.Cu", 68, USB_OUT_POUR)]
    return out
for name, bus_gnd in [("a-via-island-cut-off", True), ("b-no-bus-gnd", False)]:
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        f.write(HEAD + "".join(parts(bus_gnd)) + ")\n")
