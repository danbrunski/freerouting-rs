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
(tracks), `GetPosition`, `TopLayer`/`BottomLayer`, `GetDrillValue` (vias), `m_Uuid`, and
moved with `SetStart`/`SetEnd`/`SetWidth`/`SetPosition`/`SetUuid`; a new point is made with
the type of the item's own point (`VECTOR2I(x, y)`).
"""

from collections import namedtuple

DSN = "dsn"
KICAD_PCB = "kicad_pcb"

# Per coordinate, in nanometres: `%.6g` of micrometres under 1 m (0.5 µm) plus
# the session's 0.1 µm; on the `.kicad_pcb` path the reader's 0.1 µm plus the
# session's.
BOUND_NM = {DSN: 600, KICAD_PCB: 200}
SESSION_NM = 100  # a track width's round trip through the session

TRACK, VIA = "track", "via"

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
    return None  # arcs: the session has none, KiCad's import writes chords


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
    if kind == VIA:
        points = (_xy(item.GetPosition()),)
        width, drill = _via_width(item), _via_drill(item)
        return Original(order, kind, item.GetNetname(), _via_layers(item), width, drill, points, _uuid(item))
    return None


def originals(tracks):
    """The geometry of the unlocked tracks and vias of `tracks` (the board's
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


def _restored_ends(records, chosen, bound):
    """{(net, grid cell): [(xy, original order, point index)]} of the restored
    originals, in their order."""
    size = 2 * bound + 1
    ends = {}
    for order in sorted(chosen):
        rec = records[order]
        for k, xy in enumerate(rec.points):
            ends.setdefault((rec.net, _cell(xy, size)), []).append((xy, order, k))
    return ends


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


def restore(tracks, records, bound):
    """Restores the imported (unlocked) tracks and vias of `tracks` that match
    `records` (`originals`, taken before the import), then moves the routed ends
    that met them. Returns `{"tracks", "vias", "ends", "unmatched"}`: restored
    tracks and vias, routed ends moved, originals with no match."""
    items = [item for item in tracks if not item.IsLocked() and _kind(item) is not None]
    imported = [_record(item, i) for i, item in enumerate(items)]
    used_orig, used_imp, chosen = set(), set(), {}
    for _d, order, i in _pairs(records, imported, bound):
        if order in used_orig or i in used_imp:
            continue
        used_orig.add(order)
        used_imp.add(i)
        chosen[order] = i
    counts = {"tracks": 0, "vias": 0, "ends": 0, "unmatched": len(records) - len(chosen)}
    for order in sorted(chosen):
        orig = records[order]
        _restore_item(items[chosen[order]], orig)
        counts["tracks" if orig.kind == TRACK else "vias"] += 1
    ends = _restored_ends(records, chosen, bound)
    for i, item in enumerate(items):
        if i not in used_imp:
            counts["ends"] += _follow(item, imported[i], ends, bound)
    return counts


def log_line(counts):
    """The run log's line for `restore`'s counts."""
    return (
        f"input copper: {counts['tracks']} track(s) and {counts['vias']} via(s) back at their own coordinates, "
        f"{counts['ends']} routed end(s) moved onto them, {counts['unmatched']} input item(s) left as routed (Q22.3)"
    )
