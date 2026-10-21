"""K13.8: the DSN carries each unlocked track arc as the polyline the
`.kicad_pcb` reader lays (`track_arcs`), not as KiCad's one chord."""

import math
import types

import track_arcs
from dsn_fixups import Dsn

# The fixture's arc of net A (`data/arc_pair.kicad_pcb`): radius 1 mm around (100, 100) mm.
A_ARC = ((101000000, 100000000), (100800000, 100600000), (100000000, 101000000))

DSN = """(pcb x
  (structure
    (layer F.Cu (type signal))
    (layer B.Cu (type signal))
    (rule (width 200) (clearance 200))
  )
  (network (net A) (net "/CM5/X"))
  (wiring
    (wire (path F.Cu 200  101000 -97000  101000 -100000)(net A)(type route))
    (wire (path F.Cu 200  101000 -100000  100000 -101000)(net A)(type route))
    (wire (path B.Cu 140  119505 -29387  119288 -29604.7  119070 -29822.5  118905 -29822.5)(net "/CM5/X")(type route))
  )
)
"""

# CM5_MINIMA_3's two B.Cu arcs that KiCad's export merged into one path with a segment.
CM5_ARCS = [
    ((119070000, 29822450), (119223955, 29758680), (119287725, 29604725)),
    ((119287725, 29604725), (119351495, 29450770), (119505450, 29387000)),
]


def arc(order, start, mid, end, layer="F.Cu", net="A", width=200000):
    return track_arcs.Arc(order, layer, net, width, start, mid, end)


def wires(text):
    out = []
    for node in Dsn(text).wiring.children:
        if node.is_list and node.head == "wire":
            path = next(c for c in node.children if c.is_list and c.head == "path")
            out.append([c.text for c in path.children[1:]])
    return out


def um(xy):
    return (xy[0] / 1000.0, -xy[1] / 1000.0)


def test_arc_points_follow_the_arc_within_the_readers_tolerance():
    """`tracks.rs::arc_points`: both ends exact, every point on the arc, no
    chord farther than 1 µm from it and none over 0.1 rad."""
    pts = track_arcs.arc_points(*A_ARC)
    assert pts[0] == A_ARC[0] and pts[-1] == A_ARC[2]
    assert len(pts) == 19  # ceil(pi/2 / (2 acos(1 - 1 µm / 1 mm))) chords
    centre, r = (100000000, 100000000), 1000000
    assert all(abs(math.hypot(x - centre[0], y - centre[1]) - r) <= 1 for x, y in pts)
    for a, b in zip(pts, pts[1:]):
        half = math.hypot(b[0] - a[0], b[1] - a[1]) / 2
        assert r - math.sqrt(r * r - half * half) <= 1000 + 1
        assert 2 * math.asin(half / r) <= 0.1 + 1e-9
    # collinear points give the line; a 1-nm-sagitta arc gives its chord
    assert track_arcs.arc_points((0, 0), (5, 0), (10, 0)) == [(0, 0), (10, 0)]
    assert track_arcs.ki_round(2.5) == 3 and track_arcs.ki_round(-2.5) == -3 and track_arcs.ki_round(0.49999999999999994) == 0


def test_a_two_point_chord_becomes_the_readers_polyline():
    text, lines = track_arcs.apply(DSN, [arc(0, *A_ARC)])
    got = wires(text)[1]
    assert got[:2] == ["F.Cu", "200"]
    points = [(float(x), float(y)) for x, y in zip(got[2::2], got[3::2])]
    assert points == [um(p) for p in track_arcs.arc_points(*A_ARC)]
    assert got[2:4] == ["101000", "-100000"] and got[-2:] == ["100000", "-101000"]  # KiCad's own ends
    assert lines == ["track arcs: 1 of 1 unlocked arc(s) written as polylines, 17 point(s) added (K13.8)"]
    # everything but that path is byte for byte
    first = DSN.index("(path F.Cu 200  101000 -100000")
    assert text[:first] == DSN[:first]
    tail = DSN[DSN.index(")(net A)(type route))\n    (wire (path B.Cu") :]
    assert text.endswith(tail)


def test_a_chord_in_a_merged_path_run_end_to_start_gets_its_points_in_path_order():
    """KiCad merges an arc into the path of the tracks it joins; this path runs
    from the second arc's end to the first arc's start."""
    arcs = [arc(i, *a, layer="B.Cu", net="/CM5/X", width=140000) for i, a in enumerate(CM5_ARCS)]
    text, lines = track_arcs.apply(DSN, arcs)
    got = wires(text)[2]
    points = [(float(x), float(y)) for x, y in zip(got[2::2], got[3::2])]
    second = [um(p) for p in reversed(track_arcs.arc_points(*CM5_ARCS[1]))]
    first = [um(p) for p in reversed(track_arcs.arc_points(*CM5_ARCS[0]))]
    want = [(119505, -29387)] + second[1:-1] + [(119288, -29604.7)] + first[1:-1] + [(119070, -29822.5), (118905, -29822.5)]
    assert points == want
    assert lines[0].startswith("track arcs: 2 of 2 unlocked arc(s) written as polylines")


def test_an_arc_with_no_chord_in_the_wiring_is_counted_and_nothing_changes():
    others = [
        arc(0, *A_ARC, layer="B.Cu"),
        arc(1, *A_ARC, net="B"),
        arc(2, *A_ARC, width=250000),
        arc(3, (101000000, 100000000), (100800000, 100600000), (100000000, 101002000)),  # 2 µm off at the end
    ]
    text, lines = track_arcs.apply(DSN, others)
    assert text == DSN
    assert lines == ["track arcs: 0 of 4 unlocked arc(s) written as polylines, 0 point(s) added (K13.8); 4 not found in the DSN's wiring, left as chords"]
    assert track_arcs.apply(DSN, []) == (DSN, [])


def test_each_arc_replaces_one_chord():
    doubled = DSN.replace(
        "    (wire (path B.Cu",
        "    (wire (path F.Cu 200  100000 -101000  101000 -100000)(net A)(type route))\n    (wire (path B.Cu",
    )
    text, _ = track_arcs.apply(doubled, [arc(0, *A_ARC)])
    lengths = [len(w) for w in wires(text)]
    assert lengths[1] == 2 + 2 * 19 and lengths[2] == 6  # the second copy stays a chord


class _Pt:
    def __init__(self, xy):
        self.x, self.y = xy


def _item(cls, points, locked=False, net="A", layer=0, width=200000):
    pts = [_Pt(p) for p in points]
    return types.SimpleNamespace(
        GetClass=lambda: cls,
        IsLocked=lambda: locked,
        GetNetname=lambda: net,
        GetLayer=lambda: layer,
        GetWidth=lambda: width,
        GetStart=lambda: pts[0],
        GetMid=lambda: pts[1],
        GetEnd=lambda: pts[-1],
    )


def test_collect_reads_the_unlocked_arcs_in_board_order():
    board = types.SimpleNamespace(
        GetTracks=lambda: [
            _item("PCB_TRACK", [(0, 0), (1, 1)]),
            _item("PCB_ARC", A_ARC, locked=True),
            _item("PCB_ARC", A_ARC, net="B", layer=2),
            _item("PCB_VIA", [(0, 0)]),
        ],
        GetLayerName=lambda layer: {0: "F.Cu", 2: "B.Cu"}[layer],
    )
    assert track_arcs.collect(board) == [track_arcs.Arc(0, "B.Cu", "B", 200000, *A_ARC)]


# The reader's own points for two arcs (`freerouting_kicad::tracks::arc_points`,
# printed by a probe binary on 2026-10-21; the same probe found the port
# identical on CM5_MINIMA_3's 200 arcs and 2000 random arcs, 289027 points).
READER_POINTS = {
    A_ARC: "101000000 100000000 100996195 100087156 100984808 100173648 100965926 100258819 100939693 100342020 "
    "100906308 100422618 100866025 100500000 100819152 100573576 100766044 100642788 100707107 100707107 "
    "100642788 100766044 100573576 100819152 100500000 100866025 100422618 100906308 100342020 100939693 "
    "100258819 100965926 100173648 100984808 100087156 100996195 100000000 101000000",
    CM5_ARCS[0]: "119070000 29822450 119091341 29821402 119112476 29818267 119133202 29813075 119153320 29805877 "
    "119172635 29796741 119190962 29785757 119208123 29773029 119223955 29758680 119238304 29742848 "
    "119251032 29725687 119262016 29707360 119271152 29688045 119278350 29667927 119283542 29647201 "
    "119286677 29626066 119287725 29604725",
}


def test_arc_points_are_the_readers_points():
    for arc_xy, text in READER_POINTS.items():
        v = [int(t) for t in text.split()]
        assert track_arcs.arc_points(*arc_xy) == list(zip(v[::2], v[1::2]))
