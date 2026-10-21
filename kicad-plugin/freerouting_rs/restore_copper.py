"""The board's own copper comes back at its own coordinates after the session
import (Q22.3, K13.2).

KiCad's `ImportSpecctraSES` deletes every unlocked track and via and adds the
session's, so copper that was on the board before routing comes back at the
coordinates the round trip gave it:
- on the DSN path, KiCad's `ExportSpecctraDSN` prints coordinates with `%.6g`
  (micrometres, so 1 µm steps from 100 mm on), and the session adds its 0.1 µm
  resolution. Input copper moved by up to 0.6 µm: cm4-edge-ai's diff pairs came
  back 0.7 µm short of their 0.15 mm gap (Q22.1, cause v-a);
- on the `.kicad_pcb` path the reader rounds to 0.1 µm, and the session again
  (One-Air-Max's `/Solar_module/SYS`-`VBAT` near-touch counted against one more
  segment pair, K10.5).

`restore` undoes that. An imported track or via that matches an original gets
the original's geometry back. A match has:
- the same kind (track or via), net and layer (a via: its layer pair);
- for a track, the same width within the session's 0.1 µm (the original's width
  is restored with the coordinates); for a via, the same diameter and drill;
- every coordinate within `bound_nm` of the original's, a track in either
  direction.

Then each end of an imported track or via that was not restored and lies within
the bound of a restored end of its net moves onto that end, so routed copper
that met the input copper still meets it, unless the track would end up with
length 0 (cm4-edge-ai: a 0.5 µm router stub from a pad centre to the rounded end
of an input track). Anything the router moved further stays as imported.

Track arcs (K13.8). The session has no arcs: KiCad's export writes an arc as
its chord, and the plugin's `track_arcs` (DSN path) and the `.kicad_pcb` reader
(snapshot path) as a polyline of points on the arc, no chord farther than 1 µm
from it (`ARC_SAGITTA_NM`). An unlocked original arc is put back where the
imported tracks that have not matched a track lay it unmoved: a chain of
tracks of its net and layer, width within 0.1 µm, from within the bound of its
start to within the bound of its end, every point and every track's midpoint
within the bound plus 1 µm of the arc and each point further along it (in
either direction; a track the router split counts). The chain's tracks are
removed and the arc is added with the original's start, mid, end, width,
layer and uuid, on the chain's net. A bare chord (its midpoint is the arc's
sagitta off) and a polyline the router moved stay as imported. Arcs are
matched after tracks and vias, in board order, each imported track used once;
at a branch the nearest continuation is taken, then the shortest, then board
order. Adding and removing need the board: `restore` takes an `editor`
(`PcbnewEditor(board, pcbnew)`, or a test's stand-in); without one no arc is
put back.

Matching is deterministic: every candidate pair is sorted by its squared
distance, then the original's board order, then the imported item's board
order, and taken greedily, each item once. The originals are plain tuples taken
before the import (`originals`), because the import deletes the board's own
objects.

A restored item also gets the original's uuid back (`SetUuid`). KiCad saves
tracks sorted by net, layer, type and uuid, and which of two segments that meet
at a junction its DRC names in a clearance item follows that order: with the
geometry restored but new uuids, One-Air-Max's `/Solar_module/SYS`-`VBAT`
near-touch named `SYS` 2.8057 mm where the board names `SYS` 8.0070 mm (K13.2).

Callers: `export_flow.after_import` (the plugin's Route and Route in GUI, and
`headless_run.py import`) and the oracle's `kicad_steps.py import`. Java's
plugin imports the session as it is (docs/DIVERGENCES.md, kicad-plugin).

Pure Python: no pcbnew import, no wx. Items are read through the pcbnew methods
`GetClass`, `IsLocked`, `GetNetname`, `GetLayer`, `GetWidth`, `GetStart`/`GetEnd`
(tracks), `GetMid` (arcs), `GetPosition`, `TopLayer`/`BottomLayer`, `GetDrillValue`
(vias), `m_Uuid`, and moved with `SetStart`/`SetEnd`/`SetWidth`/`SetPosition`/`SetUuid`; a
new point is made with the type of the item's own point (`VECTOR2I(x, y)`).
`PcbnewEditor` gets the pcbnew module from its caller and uses `PCB_ARC`, `VECTOR2I`,
`KIID`, the arc's setters (`SetMid`, `SetLayer`, `SetNet`) and the board's `Add`/`Remove`.
"""

import math
from collections import namedtuple

DSN = "dsn"
KICAD_PCB = "kicad_pcb"

# Per coordinate, in nanometres: `%.6g` of micrometres under 1 m (0.5 µm) plus
# the session's 0.1 µm; on the `.kicad_pcb` path the reader's 0.1 µm plus the
# session's.
BOUND_NM = {DSN: 600, KICAD_PCB: 200}
SESSION_NM = 100  # a track width's round trip through the session
# K13.8: the largest distance between a polyline chord and its arc, as the
# reader and `track_arcs` lay them (`tracks.rs`, `MAX_SAGITTA_NM`).
ARC_SAGITTA_NM = 1000

TRACK, VIA, ARC = "track", "via", "arc"

Original = namedtuple("Original", "order kind net layer width drill points uuid")


def bound_nm(board_input):
    """The per-coordinate bound of a board input (`options.board_input`)."""
    return BOUND_NM.get(board_input, BOUND_NM[DSN])


def _kind(item):
    get_class = getattr(item, "GetClass", None)
    name = get_class() if get_class is not None else None
    if name == "PCB_TRACK":
        return TRACK
    if name == "PCB_VIA":
        return VIA
    if name == "PCB_ARC":
        return ARC
    return None


def _xy(point):
    return (int(point.x), int(point.y))


def _point(like, xy):
    return type(like)(xy[0], xy[1])


def _via_layers(via):
    try:
        return (via.TopLayer(), via.BottomLayer())
    except AttributeError:
        return (via.GetLayer(),)


def _via_width(via):
    try:
        return via.GetWidth(via.TopLayer())
    except (TypeError, AttributeError):  # one width for the via (KiCad 8)
        return via.GetWidth()


def _via_drill(via):
    getter = getattr(via, "GetDrillValue", None) or via.GetDrill
    return getter()


def _uuid(item):
    kiid = getattr(item, "m_Uuid", None)
    return kiid.AsString() if kiid is not None else None


def _record(item, order):
    kind = _kind(item)
    if kind == TRACK:
        points = (_xy(item.GetStart()), _xy(item.GetEnd()))
        return Original(order, kind, item.GetNetname(), (item.GetLayer(),), item.GetWidth(), 0, points, _uuid(item))
    if kind == ARC:
        points = (_xy(item.GetStart()), _xy(item.GetMid()), _xy(item.GetEnd()))
        return Original(order, kind, item.GetNetname(), (item.GetLayer(),), item.GetWidth(), 0, points, _uuid(item))
    if kind == VIA:
        points = (_xy(item.GetPosition()),)
        width, drill = _via_width(item), _via_drill(item)
        return Original(order, kind, item.GetNetname(), _via_layers(item), width, drill, points, _uuid(item))
    return None


def originals(tracks):
    """The geometry of the unlocked tracks, arcs and vias of `tracks` (the board's
    before the import, or the ones `plugin.detach_unlocked_tracks` took off),
    in that order."""
    out = []
    for item in tracks:
        if item.IsLocked():
            continue
        record = _record(item, len(out))
        if record is not None:
            out.append(record)
    return out


def _cell(xy, size):
    return (xy[0] // size, xy[1] // size)


def _near(xy, size):
    cx, cy = _cell(xy, size)
    return [(cx + dx, cy + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1)]


def _within(a, b, bound):
    return abs(a[0] - b[0]) <= bound and abs(a[1] - b[1]) <= bound


def _dist2(a, b):
    return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2


class _Index:
    """The originals by (kind, net, layer) and grid cell of each of their points."""

    def __init__(self, records, bound):
        self.size = 2 * bound + 1
        self.cells = {}
        for rec in records:  # board order, so every list is in that order
            for xy in sorted(set(rec.points)):
                key = (rec.kind, rec.net, rec.layer, _cell(xy, self.size))
                self.cells.setdefault(key, []).append(rec)

    def near(self, kind, net, layer, xy):
        seen = {}
        for cell in _near(xy, self.size):
            for rec in self.cells.get((kind, net, layer, cell), ()):
                seen[rec.order] = rec
        return [seen[order] for order in sorted(seen)]


def _match_distance(rec, imported, bound):
    """The squared distance of an imported record to an original, or None when
    it is not a match (`imported` is a record of the same kind, net and layer)."""
    if rec.kind == TRACK:
        if abs(rec.width - imported.width) > SESSION_NM:
            return None
        best = None
        for a, b in ((0, 1), (1, 0)):  # either direction
            if _within(rec.points[0], imported.points[a], bound) and _within(rec.points[1], imported.points[b], bound):
                d = _dist2(rec.points[0], imported.points[a]) + _dist2(rec.points[1], imported.points[b])
                best = d if best is None else min(best, d)
        return best
    if rec.width != imported.width or rec.drill != imported.drill:
        return None
    return _dist2(rec.points[0], imported.points[0]) if _within(rec.points[0], imported.points[0], bound) else None


def _pairs(records, imported, bound):
    """Every (distance, original order, imported order) that matches."""
    index = _Index(records, bound)
    pairs = []
    for i, rec in enumerate(imported):
        if rec is None:
            continue
        for orig in index.near(rec.kind, rec.net, rec.layer, rec.points[0]):
            d = _match_distance(orig, rec, bound)
            if d is not None:
                pairs.append((d, orig.order, i))
    return sorted(pairs)


def _restore_item(item, orig):
    if orig.kind == TRACK:
        item.SetStart(_point(item.GetStart(), orig.points[0]))
        item.SetEnd(_point(item.GetEnd(), orig.points[1]))
        if item.GetWidth() != orig.width:
            item.SetWidth(orig.width)
    else:
        item.SetPosition(_point(item.GetPosition(), orig.points[0]))
    kiid = getattr(item, "m_Uuid", None)
    if orig.uuid and kiid is not None and hasattr(item, "SetUuid") and kiid.AsString() != orig.uuid:
        item.SetUuid(type(kiid)(orig.uuid))


def _ends(rec):
    """The points of an original that other copper meets: an arc's ends, not its mid."""
    return (rec.points[0], rec.points[2]) if rec.kind == ARC else rec.points


def _restored_ends(records, chosen, bound):
    """{(net, grid cell): [(xy, original order, point index)]} of the restored
    originals, in their order."""
    size = 2 * bound + 1
    ends = {}
    for order in sorted(chosen):
        rec = records[order]
        for k, xy in enumerate(_ends(rec)):
            ends.setdefault((rec.net, _cell(xy, size)), []).append((xy, order, k))
    return ends


# --- K13.8: track arcs ----------------------------------------------------------


class _ArcShape:
    """An original arc as a circle through its three points (a line when they
    are collinear), with the distance of a point from it and the point's
    position along it, both in nanometres."""

    def __init__(self, rec):
        (sx, sy), (mx, my), (ex, ey) = rec.points
        bx, by, cx, cy = mx - sx, my - sy, ex - sx, ey - sy
        cross = float(bx * cy - by * cx)
        self.start = (float(sx), float(sy))
        self.circle = cross != 0.0
        if not self.circle:
            self.dx, self.dy = float(cx), float(cy)
            self.length = math.hypot(cx, cy)
            return
        b2, c2 = float(bx * bx + by * by), float(cx * cx + cy * cy)
        ux, uy = (cy * b2 - by * c2) / (2.0 * cross), (bx * c2 - cx * b2) / (2.0 * cross)
        self.centre = (sx + ux, sy + uy)
        self.r = math.hypot(ux, uy)
        self.sign = 1.0 if cross > 0 else -1.0
        self.ts = math.atan2(-uy, -ux)
        sweep = (self.sign * (math.atan2(cy - uy, cx - ux) - self.ts)) % math.tau
        self.length = (sweep if sweep > 0 else math.tau) * self.r

    def off(self, p):
        """The distance of `p` from the circle (the line)."""
        if not self.circle:
            if self.length == 0:
                return math.hypot(p[0] - self.start[0], p[1] - self.start[1])
            return abs((p[0] - self.start[0]) * self.dy - (p[1] - self.start[1]) * self.dx) / self.length
        return abs(math.hypot(p[0] - self.centre[0], p[1] - self.centre[1]) - self.r)

    def along(self, p):
        """How far along the arc, from its start, `p` lies; a point just before
        the start is negative."""
        if not self.circle:
            if self.length == 0:
                return 0.0
            return ((p[0] - self.start[0]) * self.dx + (p[1] - self.start[1]) * self.dy) / self.length
        u = (self.sign * (math.atan2(p[1] - self.centre[1], p[0] - self.centre[0]) - self.ts)) % math.tau
        if u * self.r > self.length + (math.tau * self.r - self.length) / 2:
            u -= math.tau
        return u * self.r

    def on(self, p, tol):
        return self.off(p) <= tol and -tol <= self.along(p) <= self.length + tol


def _arc_step(shape, imported, free, at, tol, bound):
    """The free imported track that continues the chain at `at`: one end
    within the bound of `at`, the other end and its midpoint on the arc and
    further along it. Ties: nearest, then shortest step, then board order.
    Returns (index, far end) or None."""
    best = None
    here = shape.along(at)
    for i in free:
        p = imported[i].points
        for a, b in ((p[0], p[1]), (p[1], p[0])):
            if not _within(a, at, bound) or not shape.on(b, tol):
                continue
            ahead = shape.along(b)
            mid = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)
            if ahead <= here or shape.off(mid) > tol:
                continue
            key = (_dist2(a, at), ahead, i)
            if best is None or key < best[0]:
                best = (key, i, b)
    return None if best is None else (best[1], best[2])


def _arc_chain(rec, imported, free, bound):
    """The imported tracks that lay the original arc `rec` from its start to
    its end: within `bound` at the ends and `bound` plus the polyline's
    sagitta everywhere else. Returns their indices in chain order, or None."""
    shape = _ArcShape(rec)
    tol = bound + ARC_SAGITTA_NM
    candidates = [
        i
        for i in free
        if abs(imported[i].width - rec.width) <= SESSION_NM and all(shape.on(p, tol) for p in imported[i].points)
    ]
    chain, at, left = [], rec.points[0], set(candidates)
    while left:
        step = _arc_step(shape, imported, sorted(left), at, tol, bound)
        if step is None:
            return None
        i, at = step
        chain.append(i)
        left.discard(i)
        if _within(at, rec.points[2], bound):
            return chain
    return None


def _arc_chains(records, imported, used_imp, bound):
    """{original order: [imported index]} for the original arcs whose
    polyline came back unmoved, each imported track used once, the arcs in
    board order."""
    by_key = {}
    for i, rec in enumerate(imported):
        if rec is not None and rec.kind == TRACK and i not in used_imp:
            by_key.setdefault((rec.net, rec.layer), []).append(i)
    taken, chains = set(), {}
    for rec in records:
        if rec.kind != ARC:
            continue
        free = [i for i in by_key.get((rec.net, rec.layer), ()) if i not in taken]
        chain = _arc_chain(rec, imported, free, bound)
        if chain:
            chains[rec.order] = chain
            taken.update(chain)
    return chains


class PcbnewEditor:
    """Puts an arc back on a pcbnew board in place of the imported tracks
    that lay it (`restore`'s `editor`). `pcbnew` is the module, passed in."""

    def __init__(self, board, pcbnew):
        self.board, self.pcbnew = board, pcbnew

    def add_arc(self, rec, like):
        """A new arc with the original's geometry, width, layer and uuid, on the
        net of `like` (one of the imported tracks it replaces)."""
        pcbnew = self.pcbnew
        arc = pcbnew.PCB_ARC(self.board)
        arc.SetStart(pcbnew.VECTOR2I(*rec.points[0]))
        arc.SetMid(pcbnew.VECTOR2I(*rec.points[1]))
        arc.SetEnd(pcbnew.VECTOR2I(*rec.points[2]))
        arc.SetWidth(rec.width)
        arc.SetLayer(rec.layer[0])
        arc.SetNet(like.GetNet())
        if rec.uuid and hasattr(arc, "SetUuid"):
            arc.SetUuid(pcbnew.KIID(rec.uuid))
        self.board.Add(arc)
        return arc

    def remove(self, item):
        """Takes an imported track off the board. C++ keeps it (`thisown = 0`,
        as `plugin.detach_unlocked_tracks` does): inside an action plugin
        nothing may free a board item behind KiCad's back."""
        self.board.Remove(item)
        try:
            item.thisown = 0
        except AttributeError:  # not a SWIG object
            pass


def _snap(xy, net, ends, bound):
    """The nearest restored end of `net` within the bound of `xy` (ties: the
    original's order, then its start before its end), or None."""
    best = None
    for cell in _near(xy, 2 * bound + 1):
        for end, order, k in ends.get((net, cell), ()):
            if _within(end, xy, bound):
                key = (_dist2(end, xy), order, k)
                if best is None or key < best[0]:
                    best = (key, end)
    return None if best is None else best[1]


def _follow(item, rec, ends, bound):
    """Moves the ends of an unrestored imported item onto restored ends within
    the bound, unless that would leave a track of length 0 (a router stub of
    under 1 µm that joined a rounded end to the true one stays as imported:
    KiCad reports a zero-length track as `tracks_crossing`). Returns the number
    of ends moved."""
    moved = 0
    getters = (("GetStart", "SetStart"), ("GetEnd", "SetEnd")) if rec.kind == TRACK else (("GetPosition", "SetPosition"),)
    points = list(rec.points)
    for k, ((get, put), xy) in enumerate(zip(getters, rec.points)):
        target = _snap(xy, rec.net, ends, bound)
        if target is None or target == xy or (len(points) == 2 and points[1 - k] == target):
            continue
        getattr(item, put)(_point(getattr(item, get)(), target))
        points[k] = target
        moved += 1
    return moved


def restore(tracks, records, bound, editor=None):
    """Restores the imported (unlocked) tracks and vias of `tracks` that match
    `records` (`originals`, taken before the import), puts back the original
    arcs whose polyline came back unmoved (with `editor`, a `PcbnewEditor` or
    an object with its `add_arc` and `remove`; without one no arc is put
    back), then moves the routed ends that met them. Returns `{"tracks",
    "vias", "arcs", "ends", "unmatched"}`: restored tracks, vias and arcs,
    routed ends moved, originals with no match."""
    items = [item for item in tracks if not item.IsLocked() and _kind(item) in (TRACK, VIA)]
    imported = [_record(item, i) for i, item in enumerate(items)]
    used_orig, used_imp, chosen = set(), set(), {}
    for _d, order, i in _pairs(records, imported, bound):
        if order in used_orig or i in used_imp:
            continue
        used_orig.add(order)
        used_imp.add(i)
        chosen[order] = i
    arcs = _arc_chains(records, imported, used_imp, bound) if editor is not None else {}
    counts = {"tracks": 0, "vias": 0, "arcs": len(arcs), "ends": 0, "unmatched": len(records) - len(chosen) - len(arcs)}
    for order in sorted(chosen):
        orig = records[order]
        _restore_item(items[chosen[order]], orig)
        counts["tracks" if orig.kind == TRACK else "vias"] += 1
    replaced = set()
    for order in sorted(arcs):
        chain = arcs[order]
        editor.add_arc(records[order], items[chain[0]])
        for i in chain:
            editor.remove(items[i])
        replaced.update(chain)
    ends = _restored_ends(records, set(chosen) | set(arcs), bound)
    for i, item in enumerate(items):
        if i not in used_imp and i not in replaced:
            counts["ends"] += _follow(item, imported[i], ends, bound)
    return counts


def log_line(counts):
    """The run log's line for `restore`'s counts."""
    return (
        f"input copper: {counts['tracks']} track(s) and {counts['vias']} via(s) back at their own coordinates, "
        f"{counts['ends']} routed end(s) moved onto them, {counts['unmatched']} input item(s) left as routed (Q22.3), "
        f"{counts.get('arcs', 0)} track arc(s) back as arcs (K13.8)"
    )
