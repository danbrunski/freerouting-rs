"""The board's unlocked track arcs go to the router as polylines (K13.8).

KiCad's `ExportSpecctraDSN` writes each track arc as one chord, start to end
(KiCad 10.0.7: CM5_MINIMA_3's 200 arcs, 192 as a two-point wire and 8 as one
leg of a merged path). The router then keeps routed copper clear of the chord,
not of the arc, and the chord itself comes back through the session: the
chord of one track of a differential pair cuts into its partner's clearance
(CM5_MINIMA_3: 172 clearance items in K13.6, every one beside an arc).

`apply` replaces the chord of each unlocked arc in the DSN's `(wiring ...)` by
the polyline the `.kicad_pcb` reader lays for the same arc
(`crates/freerouting-kicad/src/tracks.rs`, `arc_points`): both ends as KiCad
wrote them, interior points on the arc, no chord farther than 1 µm from the
arc and none spanning more than 0.1 rad. `arc_points` repeats the reader's
arithmetic in the same operation order; `math.hypot` may differ from Rust's
`f64::hypot` in the last bit, which can move an interior point by 1 nm. After
the import, `restore_copper` puts the original arc back where the session
returns its polyline unmoved.

A chord is found by its layer, net, width (within 1 µm) and both ends (within
1 µm: the export prints micrometres with `%.6g`), in either direction, in a
two-point wire or as one leg of a longer path; each arc is used once, in board
order, the DSN's wires in file order. Locked arcs are left as chords: the
import keeps locked copper as it is. An arc whose chord is not found stays as
the export wrote it and is counted in the log line.

Pure Python: no pcbnew import, no wx. `collect` reads the board through the
pcbnew methods `GetTracks`, `GetLayerName`, and per item `GetClass`,
`IsLocked`, `GetLayer`, `GetNetname`, `GetWidth`, `GetStart`, `GetMid`,
`GetEnd`.
"""

import math
from collections import namedtuple

try:  # inside the plugin package
    from .dsn_fixups import Dsn, _unquote
except ImportError:  # tests and headless scripts put this folder on sys.path
    from dsn_fixups import Dsn, _unquote

# The reader's tolerance (`tracks.rs`): the longest arc one chord may span,
# radians, and the largest distance between a chord and its arc, nanometres.
MAX_CHORD_ANGLE = 0.1
MAX_SAGITTA_NM = 1000.0
MATCH_UM = 1.0  # `%.6g` of micrometres is within 0.5 µm under 1 m

Arc = namedtuple("Arc", "order layer net width start mid end")


def ki_round(v):
    """Rust's `f64::round` (half away from zero), as `units::ki_round`."""
    a = abs(v)
    r = math.floor(a)
    if a - r >= 0.5:
        r += 1
    return int(r) if v >= 0 else -int(r)


def arc_points(start, mid, end):
    """The reader's points of an arc track from `start` through `mid` to `end`
    (nanometre tuples): both ends exact. Collinear points give the line."""
    bx, by = float(mid[0]) - float(start[0]), float(mid[1]) - float(start[1])
    cx, cy = float(end[0]) - float(start[0]), float(end[1]) - float(start[1])
    cross = bx * cy - by * cx
    if cross == 0.0 or not math.isfinite(cross):
        return [tuple(start), tuple(end)]
    d = 2.0 * cross
    b2 = bx * bx + by * by
    c2 = cx * cx + cy * cy
    ux = (cy * b2 - by * c2) / d
    uy = (bx * c2 - cx * b2) / d
    r = math.hypot(ux, uy)
    if not (math.isfinite(r) and r > 0.0):
        return [tuple(start), tuple(end)]
    ts = math.atan2(-uy, -ux)
    te = math.atan2(cy - uy, cx - ux)
    sweep = te - ts
    if cross > 0.0:
        while sweep <= 0.0:
            sweep += math.tau
    else:
        while sweep >= 0.0:
            sweep -= math.tau
    ratio = min(MAX_SAGITTA_NM / r, 1.0)
    step = min(2.0 * math.acos(1.0 - ratio), MAX_CHORD_ANGLE)
    n = min(max(math.ceil(abs(sweep) / step), 1), 100_000)
    ox, oy = float(start[0]) + ux, float(start[1]) + uy
    out = [tuple(start)]
    for i in range(1, n):
        t = ts + sweep * float(i) / float(n)
        out.append((ki_round(ox + r * math.cos(t)), ki_round(oy + r * math.sin(t))))
    out.append(tuple(end))
    return out


def _xy(point):
    return (int(point.x), int(point.y))


def collect(board):
    """The unlocked track arcs of a pcbnew board, in board order."""
    out = []
    for item in board.GetTracks():
        get_class = getattr(item, "GetClass", None)
        if get_class is None or get_class() != "PCB_ARC" or item.IsLocked():
            continue
        out.append(
            Arc(
                len(out),
                board.GetLayerName(item.GetLayer()),
                item.GetNetname(),
                item.GetWidth(),
                _xy(item.GetStart()),
                _xy(item.GetMid()),
                _xy(item.GetEnd()),
            )
        )
    return out


def _dsn_um(xy):
    """A board point in the DSN's micrometres, Y negated."""
    return (xy[0] / 1000.0, -xy[1] / 1000.0)


def _fmt(v):
    text = f"{v:.3f}".rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


def _close(a, b):
    return abs(a[0] - b[0]) <= MATCH_UM and abs(a[1] - b[1]) <= MATCH_UM


def _num(text):
    try:
        return float(text)
    except ValueError:
        return None


class _Wire:
    """A `(wire (path LAYER WIDTH x y ...) (net N) ...)` of the wiring."""

    def __init__(self, node):
        self.path = next((c for c in node.children if c.is_list and c.head == "path"), None)
        net = next((c for c in node.children if c.is_list and c.head == "net"), None)
        self.net = _unquote(net.children[1].text) if net is not None and len(net.children) > 1 else None
        self.points = None
        if self.path is None or len(self.path.children) < 7 or any(c.is_list for c in self.path.children):
            return
        tokens = [c.text for c in self.path.children[1:]]
        values = [_num(t) for t in tokens[2:]]
        if len(values) % 2 or any(v is None for v in values) or _num(tokens[1]) is None:
            return
        self.layer_token, self.width_token = tokens[0], tokens[1]
        self.layer, self.width = _unquote(tokens[0]), _num(tokens[1])
        self.tokens = [(tokens[2 + 2 * i], tokens[3 + 2 * i]) for i in range(len(values) // 2)]
        self.points = [(values[2 * i], values[2 * i + 1]) for i in range(len(values) // 2)]


def _find(arcs, used, wire, a, b):
    """The first unused arc whose chord is the leg a -> b of `wire`, and
    whether it runs end to start."""
    for arc in arcs.get((wire.layer, wire.net), ()):
        if arc.order in used or abs(arc.width / 1000.0 - wire.width) > MATCH_UM:
            continue
        s, e = _dsn_um(arc.start), _dsn_um(arc.end)
        if _close(a, s) and _close(b, e):
            return arc, False
        if _close(a, e) and _close(b, s):
            return arc, True
    return None, False


def _rewrite(wire, arcs, used):
    """The new `(path ...)` text of `wire` and the number of points added, or
    (None, 0) when no leg is an arc's chord."""
    out = [wire.tokens[0]]
    added = 0
    for k in range(1, len(wire.points)):
        arc, reverse = _find(arcs, used, wire, wire.points[k - 1], wire.points[k])
        if arc is not None:
            used.add(arc.order)
            inner = arc_points(arc.start, arc.mid, arc.end)[1:-1]
            if reverse:
                inner.reverse()
            for xy in inner:
                x, y = _dsn_um(xy)
                out.append((_fmt(x), _fmt(y)))
            added += len(inner)
        out.append(wire.tokens[k])
    if not added:  # no arc, or arcs whose polyline is their chord
        return None, 0
    points = "  ".join(f"{x} {y}" for x, y in out)
    return f"(path {wire.layer_token} {wire.width_token}  {points})", added


def apply(dsn_text, arcs):
    """Rewrites the chords of `arcs` (`collect`) as polylines. Returns
    `(new_text, log_lines)`; no line when there is no unlocked arc."""
    if not arcs:
        return dsn_text, []
    dsn = Dsn(dsn_text)
    index = {}
    for arc in arcs:
        index.setdefault((arc.layer, arc.net), []).append(arc)
    used, edits, points = set(), [], 0
    wiring = dsn.wiring.children if dsn.wiring is not None else []
    for node in wiring:
        if not node.is_list or node.head != "wire":
            continue
        wire = _Wire(node)
        if wire.points is None:
            continue
        text, added = _rewrite(wire, index, used)
        if text is not None:
            edits.append((wire.path.start, wire.path.end, text))
            points += added
    for start, end, text in sorted(edits, reverse=True):
        dsn_text = dsn_text[:start] + text + dsn_text[end:]
    line = f"track arcs: {len(used)} of {len(arcs)} unlocked arc(s) written as polylines, {points} point(s) added (K13.8)"
    missing = len(arcs) - len(used)
    if missing:
        line += f"; {missing} not found in the DSN's wiring, left as chords"
    return dsn_text, [line]
