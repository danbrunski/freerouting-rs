#!/usr/bin/env python3
"""Writes fill_wires.json: what the plugin's `dsn_fixups.fill_rects` and
`dsn_fixups._wire` give on recorded fills, the expected values of
`crates/freerouting-kicad/tests/fill_wires.rs` (K14.4).

Usage, from the repository root:

    python3 -I crates/freerouting-kicad/tests/data/fill_wires_gen.py \\
        > crates/freerouting-kicad/tests/data/fill_wires.json

It loads `kicad-plugin/freerouting_rs/dsn_fixups.py` by path (with its own
folder on `sys.path` for `rule_areas`), never the `freerouting_rs` package:
the package `__init__` registers the plugin, which needs pcbnew and raises a
wx dialog. Pure Python, no pcbnew.

Inputs: the synthetic chains below and the recorded outer-layer zone fills
of `fill_wires_zones.json` (its `source` names each board). Outputs, per
case: the rectangles of `fill_rects`, in order; for each rectangle, and for
the direct `_wire` rectangles, the mm geometry `_wire` computes (`pts` and
`width`, read from its frame as it returns) and the integers of the path it
writes (`um(width)`, then `um(x) -um(y)` per point). Floats are written
with `repr`, which Python round-trips exactly. `um_cases` records
`round(v * 1000)` (half to even) on ties and near-ties.
"""

import importlib.util
import json
import re
import sys
from pathlib import Path

DATA = Path(__file__).resolve().parent
REPO = DATA.parents[3]
PLUGIN = REPO / "kicad-plugin" / "freerouting_rs"
sys.path.insert(0, str(PLUGIN))  # dsn_fixups imports rule_areas as a top-level module
spec = importlib.util.spec_from_file_location("dsn_fixups", PLUGIN / "dsn_fixups.py")
dsn_fixups = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dsn_fixups)

# name -> chains (mm, y down, as pcbnew gives a zone's fill)
SYNTHETIC = {
    "rectangle": [[(100.0, 50.0), (110.0, 50.0), (110.0, 52.0), (100.0, 52.0)]],
    "l_shape": [[(0.0, 0.0), (4.0, 0.0), (4.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)]],
    "ring_with_hole": [
        [(10.0, 10.0), (16.0, 10.0), (16.0, 16.0), (10.0, 16.0)],
        [(12.0, 12.0), (14.0, 12.0), (14.0, 14.0), (12.0, 14.0)],
    ],
    "triangle": [[(20.0, 20.0), (25.0, 20.0), (22.3, 23.7)]],
    # vertices on strip boundaries, from a left edge where x + 0.25 is exact ...
    "strip_boundaries_exact": [
        [(30.0, 0.0), (31.0, 0.0), (31.0, 0.5), (30.75, 0.5), (30.75, 1.25), (30.25, 1.25), (30.25, 0.5), (30.0, 0.5)]
    ],
    # ... and from one where it is not (137.1 + 0.25 need not equal 137.35)
    "strip_boundaries_inexact": [
        [(137.1, 60.1), (138.1, 60.1), (138.1, 60.6), (137.85, 60.6), (137.85, 61.35), (137.35, 61.35), (137.35, 60.6), (137.1, 60.6)]
    ],
    # 0.27 wide: the last 0.02 mm strip is below MIN_MM and dropped
    "thin_last_strip": [[(5.0, 5.0), (5.27, 5.0), (5.27, 6.0), (5.0, 6.0)]],
    # 0.31 wide: the last strip is 0.06 mm, kept
    "short_last_strip": [[(5.0, 7.0), (5.31, 7.0), (5.31, 8.0), (5.0, 8.0)]],
    # an interval 0.04 mm tall: below MIN_MM, no rectangle
    "thin_interval": [[(40.0, 0.0), (42.0, 0.0), (42.0, 0.04), (40.0, 0.04)]],
    # a 0.3 x 0.3 square: _wire's max(...) takes the + 0.001 side
    "square": [[(50.0, 50.0), (50.3, 50.0), (50.3, 50.3), (50.0, 50.3)]],
    # coordinates on half micrometres
    "half_um": [[(0.0005, 0.0015), (1.0025, 0.0015), (1.0025, 0.2525), (0.0005, 0.2525)]],
    # slanted sides in two directions, a vertical edge at a probe and an empty chain
    "slanted": [[(60.0, 0.0), (61.7, 0.3), (62.2, 1.9), (61.0, 2.6), (60.4, 1.1)], []],
    "empty": [],
    # a top edge rising 0.0005 mm per strip: each strip is within 1e-3 of the
    # one before it, the merge compares with the first strip of the run
    "drifting_edge": [[(70.0, 0.0), (72.0, 0.0), (72.0, 1.004), (70.0, 1.0)]],
    # a vertex exactly on the second strip's left probe (0.25 + 1e-4, as
    # fill_rects computes it): only one of its two edges crosses there; a
    # second chain above it, so a crossing counted twice would shift the pairing
    "vertex_on_probe": [
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.25 + 1e-4, 1.5), (0.0, 1.0)],
        [(0.0, 2.0), (1.0, 2.0), (1.0, 3.0), (0.0, 3.0)],
    ],
    # the same, its edge into the vertex running left to right
    "vertex_on_probe_reversed": [
        [(0.0, 1.0), (0.25 + 1e-4, 1.5), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)],
        [(0.0, 2.0), (1.0, 2.0), (1.0, 3.0), (0.0, 3.0)],
    ],
    # an interval exactly MIN_MM tall is kept (hi - lo >= MIN_MM)
    "interval_of_min": [[(80.0, 0.0), (81.0, 0.0), (81.0, 0.05), (80.0, 0.05)]],
    # 0.3 wide: the loop's x < x_end - MIN_MM at its boundary
    "width_slab_plus_min": [[(0.0, 4.0), (0.3, 4.0), (0.3, 5.0), (0.0, 5.0)]],
}

# name -> (x0, y0, x1, y1) given to _wire directly
WIRE_RECTS = {
    "square_1mm": (0.0, 0.0, 1.0, 1.0),
    "wide_near_square": (1.0, 2.0, 1.0004, 2.0003),
    "tall_near_square": (0.0, 0.0, 1.0, 1.0005),
    "tall_exact_plus": (3.0, 3.0, 3.5, 3.5009),
    "long_horizontal": (10.0, 20.0, 15.0, 20.25),
    "long_vertical": (10.0, 20.0, 10.25, 25.0),
    "half_um_points": (0.0005, 0.0015, 2.0025, 0.0035),
    "half_um_negative": (-1.0025, -0.0035, 1.0015, -0.0015),
    # squares where x0 + h / 2 + 0.001 differs from x0 + (h / 2 + 0.001)
    "square_order_a": (0.4, 0.0, 0.75, 0.35),
    "square_order_b": (2.0, 0.2, 2.7, 0.9),
}

UM_CASES = [
    0.0005, 0.0015, 0.0025, 0.0035, -0.0005, -0.0025, 1.0005, 1.0025, 2.0025, 0.0045,
    100.0005, 137.1005, 0.5, 0.0, -0.0, 0.0004999999, 0.00050001, 60.8475, 138.2325,
    1e-7, 123.4565, -123.4565, 1.0035, 2.0045, 5.0055, 0.0075,
]

PATH = re.compile(r"\(path (\S+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+)\)")


def wire_geometry(rect):
    """Runs dsn_fixups._wire on `rect`; its `pts` and `width` (mm) from its
    frame on return, and the integers of the path text it writes."""
    seen = {}

    def profile(frame, event, _arg):
        if event == "return" and frame.f_code is dsn_fixups._wire.__code__:
            seen["pts"] = frame.f_locals["pts"]
            seen["width"] = frame.f_locals["width"]

    old = sys.getprofile()
    sys.setprofile(profile)
    try:
        text = dsn_fixups._wire("L", "N", rect, "\n")
    finally:
        sys.setprofile(old)
    m = PATH.search(text)
    return {
        "rect": list(rect),
        "pts_mm": list(seen["pts"]),
        "width_mm": seen["width"],
        "path_um": [int(m.group(i)) for i in range(2, 7)],
    }


def case(name, chains, source=None):
    rects = list(dsn_fixups.fill_rects(chains))
    out = {"name": name}
    if source is not None:
        out["source"] = source
    out["chains"] = [[list(p) for p in c] for c in chains]
    out["rects"] = [list(r) for r in rects]
    out["wires"] = [wire_geometry(r) for r in rects]
    return out


def main():
    zones = json.loads((DATA / "fill_wires_zones.json").read_text(encoding="utf-8"))
    cases = [case(n, c) for n, c in SYNTHETIC.items()]
    for z in zones["zones"]:
        cases.append(case(z["name"], [[tuple(p) for p in c] for c in z["chains"]], z["source"]))
    doc = {
        "comment": "Written by fill_wires_gen.py from kicad-plugin/freerouting_rs/dsn_fixups.py; do not edit.",
        "slab_mm": dsn_fixups.SLAB_MM,
        "min_mm": dsn_fixups.MIN_MM,
        "um_cases": [{"mm": v, "times_1000": v * 1000, "um": dsn_fixups.um(v)} for v in UM_CASES],
        "fill_cases": cases,
        "wire_cases": [dict(wire_geometry(r), name=n) for n, r in WIRE_RECTS.items()],
    }
    json.dump(doc, sys.stdout, indent=None, separators=(",", ":"))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
