"""K15.24: the DSN export's planes cut where a higher-priority zone of
another net fills.

KiCad's DSN export writes each copper zone, on each of its copper layers, as
one `(plane NET (polygon LAYER 0 ...) (window ...))` of the zone's whole
outline (`specctra_export.cpp`, `zone->IterateWithHoles()`), and no zone
priority. Its zone filler does not fill that outline: it knocks out every
zone of another net that is higher in `ZONE::HigherPriority`, grown by the
clearance between the two (`ZONE_FILLER::buildDifferentNetZoneClearances`).
The router counts a pad or via centred in a plane as on it, so a via of the
lower zone's net where the higher zone fills counted as connected, and KiCad
found it dangling (the power-control unit's In2.Cu: V_BUS_12V, priority 1,
under V_BUS_24V, 41; K15.13).

`kicad_board.plane_cuts` computes, with pcbnew's own polygon arithmetic, each
exported plane as the zone's outline minus the outline of every higher zone
of another net on that layer, grown by the larger of the two zones' own
clearances (K15.15's rule, the `.kicad_pcb` reader's
`freerouting-kicad/src/zone_priority.rs`), and `apply` writes the planes
that changed in place of KiCad's: one `(plane ...)` per piece left, its holes
as `(window ...)`; a plane with nothing left is dropped. Every other byte of
the export stays as KiCad wrote it, so a board without such an overlap
exports byte-identically.

The outline minus the higher zones, not the zone's fill: a fill has a
thermal-relief or clearance hole round every pad, so a pad's centre (the
router's contact test) is not in it, and the router would count every pad of
the net as off its plane.

Pure Python: no pcbnew, no wx. The plane facts come from
`kicad_board.plane_cuts` as plain lists and dicts.
"""

try:  # inside the plugin package
    from .dsn_fixups import Dsn
except ImportError:  # tests and headless scripts put this folder on sys.path
    from dsn_fixups import Dsn

POINTS_PER_LINE = 4  # coordinate pairs per line of a written polygon, as KiCad wraps them


def _um(nm):
    """A coordinate in nm as DSN micrometres: no trailing zeros, no -0."""
    text = f"{nm / 1000:.3f}".rstrip("0").rstrip(".")
    return "0" if text == "-0" else text


def _polygon(layer, ring, indent):
    """`(polygon LAYER 0 x y ...)` of a ring in nm (KiCad's frame, Y down),
    closed with its first point as KiCad's export closes it."""
    pts = [f"{_um(x)} {_um(-y)}" for x, y in list(ring) + list(ring[:1])]
    lines = ["  ".join(pts[k : k + POINTS_PER_LINE]) for k in range(0, len(pts), POINTS_PER_LINE)]
    return f"(polygon {layer} 0  " + f"\n{indent}        ".join(lines) + ")"


def plane_text(net, layer, poly, indent, nl):
    """One `(plane ...)`: the outline of `poly` (`[outline, hole, ...]`, nm),
    its holes as windows."""
    out = f"(plane {net} {_polygon(layer, poly[0], indent)}"
    for hole in poly[1:]:
        out += f"{nl}{indent}  (window {_polygon(layer, hole, indent + '  ')})"
    return out + ")"


def _line_span(text, start, end):
    """The span of a node with its line, when nothing else is on that line."""
    left = text.rfind("\n", 0, start) + 1
    right = text.find("\n", end)
    if text[left:start].strip() or right < 0 or text[end:right].strip():
        return start, end
    return left, right + 1


def _mismatch(exported, planes):
    """The (layer, net) keys whose plane counts differ, as text, or None."""
    count = {}
    for _node, net, layer in exported:
        count.setdefault((layer, net), [0, 0])[0] += 1
    for p in planes:
        count.setdefault((p["layer"], p["net"]), [0, 0])[1] += 1
    bad = [f"{layer} {net}: {a} in the DSN, {b} on the board" for (layer, net), (a, b) in sorted(count.items()) if a != b]
    return "; ".join(bad) or None


def _log_line(p):
    cut = p["cut"]
    by = f"{len(cut['by'])} higher-priority zone(s) of another net fill ({', '.join(cut['by'])})"
    if not cut["polys"]:
        return f"plane {p['net']} on {p['layer']}: left out, every part of it lies where {by}"
    before, after = cut["area_mm2"]
    windows = sum(len(poly) - 1 for poly in cut["polys"])
    return (
        f"plane {p['net']} on {p['layer']}: cut where {by}, {before:.2f} -> {after:.2f} mm2, "
        f"{len(cut['polys'])} plane(s), {windows} window(s)"
    )


def apply(dsn_text, planes):
    """Writes the cut planes of `planes` (`kicad_board.plane_cuts`: one dict
    per exported plane, `net` and `layer` as the DSN names them, `cut` None or
    `{"by": [net], "polys": [[outline, hole, ...] in nm], "area_mm2": (before,
    after)}`) in place of KiCad's. Planes are matched by layer and net, in
    order. Returns `(text, log_lines)`: one line per cut plane; with nothing
    cut, the text unchanged and no line; a WARNING and the text unchanged when
    the DSN's planes are not the board's."""
    if not any(p["cut"] is not None for p in planes):
        return dsn_text, []
    dsn = Dsn(dsn_text)
    exported = dsn.planes()
    bad = _mismatch(exported, planes)
    if bad:
        return dsn_text, [
            f"WARNING: planes not cut by zone priority: the DSN's planes do not match the board's zones ({bad})"
        ]
    pending = {}
    for p in planes:
        pending.setdefault((p["layer"], p["net"]), []).append(p)
    edits, log = [], []
    for node, net, layer in exported:
        p = pending[(layer, net)].pop(0)
        if p["cut"] is None:
            continue
        net_token = node.children[1].text
        layer_token = next(c for c in node.children[2:] if c.is_list).children[1].text
        if not p["cut"]["polys"]:
            start, end = _line_span(dsn_text, node.start, node.end)
            edits.append((start, end, ""))
        else:
            line_start = dsn_text.rfind("\n", 0, node.start) + 1
            indent = dsn_text[line_start : node.start]
            indent = indent if not indent.strip() else "    "
            texts = [plane_text(net_token, layer_token, poly, indent, dsn.nl) for poly in p["cut"]["polys"]]
            edits.append((node.start, node.end, f"{dsn.nl}{indent}".join(texts)))
        log.append(_log_line(p))
    text = dsn_text
    for start, end, repl in sorted(edits, reverse=True):
        text = text[:start] + repl + text[end:]
    return text, log
