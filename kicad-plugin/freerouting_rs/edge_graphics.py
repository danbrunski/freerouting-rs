"""Q18.5: the Edge.Cuts and Margin shapes KiCad's DSN export leaves out, as
copper keepouts (decided 2026-10-20: routing never breaks the board's
manufacturing rules, graded under the board's own rules).

KiCad's DRC (`copper_edge_clearance`) keeps copper `m_CopperEdgeClearance`
from every shape on Edge.Cuts (from its centreline: the DRC counts no stroke
there) and on Margin (from the edge of its stroke). KiCad's DSN export carries
only the board outline: its outer contours as the boundary, which the router
keeps `router.copper_to_edge_clearance_um` from (the plugin passes
`m_CopperEdgeClearance`, Q18.3), and its holes as keepouts, which the router
keeps only its default clearance from. Everything else is invisible to the
router: Margin shapes, Edge.Cuts lines that close no contour (all of Edge.Cuts
when the outline does not close and KiCad exports the bounding box instead),
and the edge clearance of the holes.

`apply` writes each such shape into the DSN as keepouts on every layer of the
DSN's structure. Each straight piece (arcs and circles are cut into chords at
most `SAGITTA_MM` inside the curve) becomes the rectangle within `w` of it,
reaching `w` past both ends, where `w` = the edge clearance + half the stroke
(Margin only) - the DSN's default clearance (what the router keeps from a
keepout), at least `MIN_HALF_MM`, plus the chord error and 1 um of rounding
for curves. An
Edge.Cuts piece on an outer contour of the exported outline (both ends and the
middle within `ON_OUTLINE_MM`) is skipped: the router's own edge clearance
covers it. The keepouts are named `KEEPOUT_NAME` and go before the
structure's `(via ...)`, like `dsn_fixups`' edge strips.

Q18.7: KiCad's DRC (`solder_mask_bridge`) also flags copper of two nets in one
mask aperture. A board-level F.Mask or B.Mask shape (an exposed-copper area no
pad owns; slice-dcmt-bread-slice has two) becomes a keepout named
`MASK_KEEPOUT_NAME` on the copper layer of its side: a filled shape is its own
polygon (a circle `SAGITTA_MM` and 1 um wider), its stroke and an unfilled
shape the rectangles within half the stroke of each piece. The keepout holds
every net out, the net of the copper under the aperture too.

The `.kicad_pcb` reader (`crates/freerouting-kicad`, `edge_keepouts.rs`) does
the same for a `.kicad_pcb` input. Pure Python: the board facts come from
`kicad_board.edge_graphics`, in KiCad's frame (mm, Y down).
"""

import math

try:  # inside the plugin package
    from .dsn_fixups import Dsn, _children, um
except ImportError:  # tests and headless scripts put this folder on sys.path
    from dsn_fixups import Dsn, _children, um

KEEPOUT_NAME = "edge_clearance"
MASK_KEEPOUT_NAME = "solder_mask"
MASK_LAYERS = ("F.Mask", "B.Mask")
SAGITTA_MM = 0.005  # chords of arcs and circles stay this close to the curve
MIN_HALF_MM = 0.001  # a keepout is at least the line itself
ON_OUTLINE_MM = 0.02  # an Edge.Cuts piece this close to an outer contour is the outline


def _arc_points(s, m, e):
    """start, mid, end -> points along the arc (start and end exact)."""
    ax, ay = s
    bx, by = m
    cx, cy = e
    d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    if abs(d) < 1e-12:  # collinear: a straight line
        return [s, e]
    ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d
    uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d
    r = math.hypot(ax - ux, ay - uy)
    a0 = math.atan2(ay - uy, ax - ux)
    full = 2.0 * math.pi
    d1 = (math.atan2(cy - uy, cx - ux) - a0) % full
    dm = (math.atan2(by - uy, bx - ux) - a0) % full
    sweep = d1 if dm <= d1 else d1 - full
    n = _chords(r, abs(sweep))
    pts = [(ux + r * math.cos(a0 + sweep * i / n), uy + r * math.sin(a0 + sweep * i / n)) for i in range(n + 1)]
    pts[0], pts[-1] = s, e
    return pts


def _chords(r, sweep):
    """Chords for `sweep` radians of radius `r` within SAGITTA_MM of the curve."""
    if r <= SAGITTA_MM:
        return max(1, math.ceil(sweep / (math.pi / 2)))
    step = 2.0 * math.acos(1.0 - SAGITTA_MM / r)
    return max(1, math.ceil(sweep / step))


def _pieces(shape):
    """(straight pieces [(p, q)], curved?) of a shape."""
    kind, pts = shape["kind"], [tuple(p) for p in shape["points"]]
    if kind == "polyline":
        return list(zip(pts, pts[1:])), False
    if kind == "ring":
        return list(zip(pts, pts[1:] + pts[:1])), False
    if kind == "arc":
        arc = _arc_points(*pts)
        return list(zip(arc, arc[1:])), len(arc) > 2
    if kind == "circle":
        (x, y), (px, py) = pts
        r = math.hypot(px - x, py - y)
        n = max(4, _chords(r, 2.0 * math.pi))
        ring = [(x + r * math.cos(2 * math.pi * i / n), y + r * math.sin(2 * math.pi * i / n)) for i in range(n)]
        return list(zip(ring, ring[1:] + ring[:1])), True
    raise ValueError(f"unknown edge shape kind {kind!r}")


def _distance(p, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    length2 = dx * dx + dy * dy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2))
    return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)


def _on_outline(p, q, rings):
    mid = ((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
    for ring in rings:
        sides = list(zip(ring, ring[1:] + ring[:1]))
        if all(any(_distance(x, a, b) <= ON_OUTLINE_MM for a, b in sides) for x in (p, q, mid)):
            return True
    return False


def _quad(p, q, w):
    """The rectangle within `w` of the segment p-q, reaching `w` past both ends."""
    dx, dy = q[0] - p[0], q[1] - p[1]
    length = math.hypot(dx, dy)
    if length == 0:
        dx, dy, length = 1.0, 0.0, 1.0
    ux, uy = dx / length * w, dy / length * w
    nx, ny = -uy, ux
    a = (p[0] - ux, p[1] - uy)
    b = (q[0] + ux, q[1] + uy)
    return [(a[0] + nx, a[1] + ny), (b[0] + nx, b[1] + ny), (b[0] - nx, b[1] - ny), (a[0] - nx, a[1] - ny)]


def quads(facts, default_clearance_mm):
    """The keepout rectangles of `facts` (mm, KiCad's frame) and the number of
    shapes they come from."""
    edge = max(facts.get("edge_clearance_mm") or 0.0, 0.0)
    rings = [[tuple(p) for p in r] for r in facts.get("outline_mm") or []]
    out, shapes = [], 0
    for shape in facts.get("shapes") or []:
        if shape["layer"] in MASK_LAYERS:
            continue
        pieces, curved = _pieces(shape)
        margin = shape["layer"] != "Edge.Cuts"
        if not margin:
            pieces = [(p, q) for p, q in pieces if not _on_outline(p, q, rings)]
        if not pieces:
            continue
        half_stroke = shape.get("width_mm", 0.0) / 2 if margin else 0.0
        w = max(edge + half_stroke - default_clearance_mm, MIN_HALF_MM)
        w += SAGITTA_MM + MIN_HALF_MM if curved else 0.0  # the chord error and the DSN's 1 um rounding
        out += [_quad(p, q, w) for p, q in pieces]
        shapes += 1
    return out, shapes


def mask_polygons(facts):
    """Q18.7: `(side, polygon)` of every board-level mask shape of `facts`
    (`side` 0 for F.Mask, 1 for B.Mask; mm, KiCad's frame), and the number of
    shapes. A filled shape is its own polygon (a circle `SAGITTA_MM` and 1 um
    wider), its stroke and an unfilled shape the rectangles within half the
    stroke of each piece."""
    out, shapes = [], 0
    for shape in facts.get("shapes") or []:
        if shape["layer"] not in MASK_LAYERS:
            continue
        side = MASK_LAYERS.index(shape["layer"])
        kind, pts = shape["kind"], [tuple(p) for p in shape["points"]]
        width = shape.get("width_mm", 0.0)
        shapes += 1
        if shape.get("filled") and kind in ("ring", "circle"):
            if kind == "ring" and len(pts) >= 3:
                out.append((side, pts))
            elif kind == "circle":
                (x, y), (px, py) = pts
                r = math.hypot(px - x, py - y) + SAGITTA_MM + MIN_HALF_MM
                n = max(8, _chords(r, 2.0 * math.pi))
                step = 2 * math.pi / n
                out.append((side, [(x + r * math.cos(step * i), y + r * math.sin(step * i)) for i in range(n)]))
            if width <= 0:
                continue
        pieces, curved = _pieces(shape)
        w = max(width / 2, MIN_HALF_MM) + (SAGITTA_MM + MIN_HALF_MM if curved else 0.0)
        out += [(side, _quad(p, q, w)) for p, q in pieces]
    return out, shapes


def apply(dsn_text, facts):
    """The DSN with the keepouts of `facts` (`kicad_board.edge_graphics`), and
    the log lines; unchanged with nothing to keep out."""
    dsn = Dsn(dsn_text)
    rects, shapes = quads(facts, dsn.default_clearance_mm())
    masks, mask_shapes = mask_polygons(facts)
    if not rects and not masks:
        return dsn_text, []
    if dsn.structure is None:
        raise ValueError("no (structure ...) in the DSN")
    layers = [name for name, _kind, _atom in dsn.layers]

    def keepout(name, layer, poly):
        coords = " ".join(f"{um(x)} {-um(y)}" for x, y in poly + poly[:1])
        return f'    (keepout "{name}" (polygon {layer} 0 {coords})){dsn.nl}'

    lines = [keepout(KEEPOUT_NAME, layer, quad) for layer in layers for quad in rects]
    mask_lines = [keepout(MASK_KEEPOUT_NAME, layers[-1] if side else layers[0], poly) for side, poly in masks]
    via = next(iter(_children(dsn.structure, "via")), None)
    if via is not None:
        at = dsn.text.rfind("\n", 0, via.start)
        at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
    else:  # before the structure's closing parenthesis, on its own line
        at = dsn.text.rfind("\n", 0, dsn.structure.end - 1)
        at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
    text = dsn_text[:at] + dsn.nl + "".join(lines + mask_lines).rstrip("\r\n") + dsn_text[at:]
    edge = facts.get("edge_clearance_mm") or 0.0
    log = []
    if lines:
        log.append(
            f"edge graphics: {shapes} Edge.Cuts/Margin shape(s) off the board outline kept {edge:g} mm clear "
            f"({len(lines)} keepouts)"
        )
    if mask_lines:
        log.append(
            f"solder-mask graphics: {mask_shapes} board F.Mask/B.Mask shape(s) kept clear on their copper side "
            f"({len(mask_lines)} keepouts)"
        )
    return text, log
