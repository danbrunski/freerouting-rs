"""K15.33: the small boards of `tests/k15_pour_blob.rs`, cut from the pcu board.

Writes `<name>-unfilled.kicad_pcb` into the current directory; `<name>.kicad_pcb` is each one
after `kicad-cli pcb drc --refill-zones --save-board` (KiCad 10.0.7), with the pcu board's
`.kicad_dru` beside it (as `<name>.kicad_dru` and `<name>-unfilled.kicad_dru`; its unconditional
0.127 mm clearance rule is the clearance KiCad's filler applies, K15.32). Every coordinate of
the zone and the copper is the pcu board's after K15.32's DSN route
(`target/orchestrator/k1532/runs/pcu-dsn-off-dflt`, session `aac8151f...`), where KiCad reports
`via_dangling` for `/CH5_NO`'s via at (112.9114, 92.1902):

- zone EFUSE_NO_5 (`/CH5_NO`, F.Cu, priority 43) verbatim: clearance 0.5, min_thickness 0.5,
  pads connected solid, smoothing fillet (radius 0), island_removal_mode 0;
- R11's pad 1 (`/CH5_NO`), its bounding box as the router reads it (3.35 x 1.225 mm at
  (111.30275, 90.312502));
- the via, and its B.Cu track to (114.2991, 93.5779), which ends here in this script's B.Cu pad
  (on the pcu board the track goes on to a via and U7's pads);
- `SCL`'s routed F.Cu track from (113.8454, 90.2934) to (110.8041, 93.3347) and `VCC_3V3`'s from
  (111.1355, 94.7741) to (114.6028, 91.3068), each with this script's 0.4 mm pad at both ends.

`SCL`'s track crosses the zone's bottom right corner and cuts off a triangle of 0.65 mm2 that
holds the via. Opened by the zone's min_thickness (eroded by 0.249 mm) the triangle leaves a
blob 0.29 mm across, and KiCad's filler deletes every deflated polygon whose bounding box is
smaller than min_thickness on both axes (`ZONE_FILLER::fillCopperZone`, "a blob min-thickness by
min-thickness is not useful"):

- `a-corner-cut-off`: as on the pcu board. KiCad fills no copper at the via (`via_dangling`
  "Via [/CH5_NO] on F.Cu - B.Cu");
- `b-thin-zone`: the same with the zone's min_thickness 0.4 mm. The blob (0.47 mm) stays, the
  re-inflated triangle holds the via, and KiCad reports no `via_dangling`.

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
\t(gr_rect (start 106 84) (end 120 98) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-4000-8000-0000000000e0"))
'''
uid = [0x100]
def u():
    uid[0] += 1
    return "00000000-0000-4000-8000-%012x" % uid[0]
def smd(ref, x, y, net, w, h, side="F"):
    return f'''\t(footprint "k1533:smd" (layer "{side}.Cu") (uuid "{u()}") (at {x} {y})
\t\t(property "Reference" "{ref}" (at 0 0 0) (layer "{side}.SilkS") (uuid "{u()}"))
\t\t(property "Value" "x" (at 0 0 0) (layer "{side}.Fab") (uuid "{u()}"))
\t\t(pad "1" smd rect (at 0 0) (size {w} {h}) (layers "{side}.Cu" "{side}.Mask") (net "{net}") (uuid "{u()}"))
\t)
'''
def seg(x0, y0, x1, y1, net, layer="F.Cu", w=0.2):
    return f'\t(segment (start {x0} {y0}) (end {x1} {y1}) (width {w}) (layer "{layer}") (net "{net}") (uuid "{u()}"))\n'
def via(x, y, net):
    return f'\t(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "{net}") (uuid "{u()}"))\n'
def zone(min_thickness):
    xy = " ".join(f"(xy {x} {y})" for x, y in EFUSE_NO_5)
    return f'''\t(zone
\t\t(net "{NO}")
\t\t(layer "F.Cu")
\t\t(uuid "{u()}")
\t\t(name "EFUSE_NO_5")
\t\t(hatch edge 0.5)
\t\t(priority 43)
\t\t(connect_pads yes
\t\t\t(clearance 0.5)
\t\t)
\t\t(min_thickness {min_thickness})
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
NO = "/CH5_NO"
SCL = "SCL"
VCC = "VCC_3V3"
EFUSE_NO_5 = [(116.808911, 86.7), (114.758911, 86.7), (114.758911, 86.35), (109.55, 86.35), (109.55, 87.879714),
              (109.529992, 87.879887), (109.569852, 92.501826), (113.087132, 92.495592), (113.212128, 88.527913),
              (116.808911, 88.527913)]
def parts(min_thickness):
    out = [smd("R11", 111.30275, 90.312502, NO, 3.35, 1.225), smd("J1", 114.2991, 93.5779, NO, 0.4, 0.4, "B"),
           smd("J2", 113.8454, 90.2934, SCL, 0.4, 0.4), smd("J3", 110.8041, 93.3347, SCL, 0.4, 0.4),
           smd("J4", 111.1355, 94.7741, VCC, 0.4, 0.4), smd("J5", 114.6028, 91.3068, VCC, 0.4, 0.4)]
    out += [via(112.9114, 92.1902, NO), seg(112.9114, 92.1902, 114.2991, 93.5779, NO, "B.Cu"),
            seg(113.8454, 90.2934, 110.8041, 93.3347, SCL), seg(111.1355, 94.7741, 114.6028, 91.3068, VCC)]
    return out + [zone(min_thickness)]
for name, min_thickness in [("a-corner-cut-off", 0.5), ("b-thin-zone", 0.4)]:
    uid[0] = 0x100
    with open(f"{name}-unfilled.kicad_pcb", "w") as f:
        f.write(HEAD + "".join(parts(min_thickness)) + ")\n")
