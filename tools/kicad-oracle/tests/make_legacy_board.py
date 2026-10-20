"""Writes a tiny KiCad 4 (`(version 4)`) board whose net class says 0.15 mm
clearance, and two sessions for it, for the oracle's legacy-rules test (Q19).
Plain python3, no pcbnew: the board is written as text.

    python3 make_legacy_board.py OUT_DIR

OUT_DIR/legacy/legacy.kicad_pcb: a 20 x 20 mm outline, nets A and B on two
0.4 x 0.4 mm SMD pads each (A at (5, 10) and (15, 10), B at (5, 12) and
(15, 12)), net class Default with clearance 0.15 mm and no `.kicad_pro`: a
KiCad 4 board keeps its rules in the board file, and KiCad 6+ moves them to
the project when it loads it.

OUT_DIR/gap-170.ses and OUT_DIR/gap-120.ses: A as a straight 0.2 mm track
along y = 10, B as a 0.2 mm track that runs parallel to it from x = 6 to 14
with a copper gap of 0.17 mm (inside the board's 0.15 mm rule, outside
KiCad's default 0.2 mm) or 0.12 mm (a breach of the board's rule).
"""

import os
import sys

BOARD = """(kicad_pcb (version 4) (host pcbnew 4.0.7)
  (general (links 2) (no_connects 0) (area 0 0 20 20) (thickness 1.6) (drawings 4)
    (tracks 0) (zones 0) (modules 4) (nets 3))
  (page A4)
  (layers
    (0 F.Cu signal)
    (31 B.Cu signal)
    (34 B.Paste user)
    (35 F.Paste user)
    (36 B.SilkS user)
    (37 F.SilkS user)
    (38 B.Mask user)
    (39 F.Mask user)
    (44 Edge.Cuts user)
    (49 F.Fab user)
  )
  (setup
    (last_trace_width 0.2)
    (trace_clearance 0.15)
    (zone_clearance 0.15)
    (zone_45_only no)
    (trace_min 0.15)
    (segment_width 0.2)
    (edge_width 0.1)
    (via_size 0.6)
    (via_drill 0.4)
    (via_min_size 0.6)
    (via_min_drill 0.4)
    (uvia_size 0.3)
    (uvia_drill 0.1)
    (uvias_allowed no)
    (uvia_min_size 0.2)
    (uvia_min_drill 0.1)
    (pcb_text_width 0.3)
    (pcb_text_size 1.5 1.5)
    (mod_edge_width 0.15)
    (mod_text_size 1 1)
    (mod_text_width 0.15)
    (pad_size 0.4 0.4)
    (pad_drill 0)
    (pad_to_mask_clearance 0)
    (aux_axis_origin 0 0)
    (visible_elements FFFFFF7F)
  )
  (net 0 "")
  (net 1 A)
  (net 2 B)
  (net_class Default "This is the default net class."
    (clearance 0.15)
    (trace_width 0.2)
    (via_dia 0.6)
    (via_drill 0.4)
    (uvia_dia 0.3)
    (uvia_drill 0.1)
    (add_net A)
    (add_net B)
  )
{modules}{edges})
"""

MODULE = """  (module T:P1 (layer F.Cu) (tedit 0) (tstamp {stamp})
    (at {x} {y})
    (fp_text reference {ref} (at 0 1) (layer F.SilkS) hide
      (effects (font (size 1 1) (thickness 0.15))))
    (fp_text value P1 (at 0 -1) (layer F.Fab) hide
      (effects (font (size 1 1) (thickness 0.15))))
    (pad 1 smd rect (at 0 0) (size 0.4 0.4) (layers F.Cu F.Paste F.Mask)
      (net {net} {name}))
  )
"""

EDGE = "  (gr_line (start {x0} {y0}) (end {x1} {y1}) (layer Edge.Cuts) (width 0.1))\n"

PADS = (("A1", 5, 10, 1, "A"), ("A2", 15, 10, 1, "A"), ("B1", 5, 12, 2, "B"), ("B2", 15, 12, 2, "B"))

SESSION = """(session "legacy"
  (base_design "legacy")
  (was_is
  )
  (routes
    (resolution um 10)
    (parser
      (host_cad "KiCad's Pcbnew")
      (host_version "10.0.7")
    )
    (library_out
    )
    (network_out
      (net A
        (wire
          (path F.Cu 2000 {a})
        )
      )
      (net B
        (wire
          (path F.Cu 2000 {b})
        )
      )
    )
  )
)
"""

TRACK_WIDTH_MM = 0.2


def ses_point(x_mm, y_mm):
    """A KiCad point in mm as DSN/SES session units (0.1 um, y up)."""
    return f"{round(x_mm * 10000)} {round(-y_mm * 10000)}"


def session(gap_mm):
    y_parallel = 10 + TRACK_WIDTH_MM + gap_mm
    a = [(5, 10), (15, 10)]
    b = [(5, 12), (6, y_parallel), (14, y_parallel), (15, 12)]
    return SESSION.format(a="  ".join(ses_point(*p) for p in a), b="  ".join(ses_point(*p) for p in b))


def main(out_dir):
    board_dir = os.path.join(out_dir, "legacy")
    os.makedirs(board_dir, exist_ok=True)
    modules = "".join(
        MODULE.format(stamp=f"{i + 1:08X}", ref=ref, x=x, y=y, net=net, name=name)
        for i, (ref, x, y, net, name) in enumerate(PADS)
    )
    corners = [(0, 0), (20, 0), (20, 20), (0, 20)]
    edges = "".join(
        EDGE.format(x0=x0, y0=y0, x1=corners[(i + 1) % 4][0], y1=corners[(i + 1) % 4][1])
        for i, (x0, y0) in enumerate(corners)
    )
    with open(os.path.join(board_dir, "legacy.kicad_pcb"), "w", encoding="utf-8") as f:
        f.write(BOARD.replace("{modules}", modules).replace("{edges}", edges))
    for name, gap in (("gap-170", 0.17), ("gap-120", 0.12)):
        with open(os.path.join(out_dir, name + ".ses"), "w", encoding="utf-8") as f:
            f.write(session(gap))
    print(board_dir)


if __name__ == "__main__":
    main(sys.argv[1])
