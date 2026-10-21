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
shape the rectangles within half the stroke of each piece. Q22.18: the router
fits it to the pins under it (a pad of one net under it is reached, pads of
several nets keep every net out), and it carries `(clearance_class null)`
(`MASK_CLEARANCE_CLASS`): copper keeps the solder-mask-to-copper clearance
below from the aperture, no more.

Q22.12: KiCad's DRC also keeps copper of another net
`m_SolderMaskToCopperClearance` from an aperture, so `mask_polygons` grows each
shape by it (`kicad_board.edge_graphics`' `mask_to_copper_mm`): the shape's
stroke is taken twice the clearance wider, so a filled body gets a rectangle
within the clearance of each side and a stroke widens by it on each side.
Q22.18: every shape grows by `MASK_MARGIN_MM` (1 um) more, so with no clearance
a body gets 1 um strips too: the keepouts keep no clearance of their own and
must hold KiCad's exact grown aperture after the chords and the 1 um rounding
(342 nm inside it at worst before, on canary-pwr; 0 after). The pads' apertures grow by it in
`kicad_board.mask_expansions`.

Q22.16: a footprint's own mask shapes (`kicad_board.edge_graphics` gives them a
`footprint` key) become keepouts the same way, named `solder_mask_pads` by
`footprint_masks`, which the router fits to the pads under them; none for a
footprint that allows bridges. `pad_trim.trim` then renames one over pads of
several nets `solder_mask_shared` and cuts those pads back to their part
outside it.

The `.kicad_pcb` reader (`crates/freerouting-kicad`, `edge_keepouts.rs`) does
the same for a `.kicad_pcb` input. Pure Python: the board facts come from
`kicad_board.edge_graphics`, in KiCad's frame (mm, Y down).
"""

import math

try:  # inside the plugin package
    from . import footprint_masks, pad_trim
    from .dsn_fixups import Dsn, _children, um
except ImportError:  # tests and headless scripts put this folder on sys.path
    import footprint_masks
    import pad_trim
    from dsn_fixups import Dsn, _children, um

KEEPOUT_NAME = "edge_clearance"
MASK_KEEPOUT_NAME = "solder_mask"
MASK_LAYERS = ("F.Mask", "B.Mask")
MASK_CLEARANCE_CLASS = "null"  # Q22.18: the router's class 0, no clearance of its own
# Q22.18: every mask aperture grows by this on top of the clearance, so the keepout holds KiCad's
# exact grown aperture after the 1 um rounding (at most 0.71 um inward) and the router's 0.1 um
MASK_MARGIN_MM = 0.001
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
    """Q18.7: `(side, polygon, name, footprint?)` of every mask shape of
    `facts` (`side` 0 for F.Mask, 1 for B.Mask; mm, KiCad's frame), and the
    number of board shapes and of footprint shapes. A filled shape is its own polygon (a circle
    `SAGITTA_MM` and 1 um wider), its stroke and an unfilled shape the
    rectangles within half the stroke of each piece. Q22.12: grown by
    `facts["mask_to_copper_mm"]` (the board's solder-mask-to-copper clearance)
    through a stroke twice that wider, and Q22.18's `MASK_MARGIN_MM` more (so
    the 1 um rounding never cuts inside KiCad's aperture). A board shape is named `MASK_KEEPOUT_NAME`; Q22.16: a footprint's
    shape `footprint_masks.keepout_name`, and none is left out."""
    out, shapes, fp_shapes = [], 0, 0
    grow = max(facts.get("mask_to_copper_mm") or 0.0, 0.0) + MASK_MARGIN_MM
    for shape in facts.get("shapes") or []:
        if shape["layer"] not in MASK_LAYERS:
            continue
        side = MASK_LAYERS.index(shape["layer"])
        name = MASK_KEEPOUT_NAME
        if "footprint" in shape:
            name = footprint_masks.keepout_name(shape["footprint"])
            if name is None:
                continue
            fp_shapes += 1
        else:
            shapes += 1
        if grow > 0:
            shape = dict(shape, width_mm=max(shape.get("width_mm", 0.0), 0.0) + 2 * grow)
        body, strokes = shape_polygons(shape)
        out += [(side, poly, name, "footprint" in shape) for poly in ([body] if body else []) + strokes]
    return out, shapes, fp_shapes


def shape_polygons(shape):
    """`(body, strokes)` of one shape (mm, KiCad's frame): the polygon of a
    filled ring or circle (a circle `SAGITTA_MM` and 1 um wider), else None,
    and the rectangles within half the stroke of each piece (curves cut into
    chords, then `SAGITTA_MM` and 1 um wider), none for a filled shape without
    a stroke. Q18.7's mask apertures and Q22.4's copper graphics."""
    kind, pts = shape["kind"], [tuple(p) for p in shape["points"]]
    width = shape.get("width_mm", 0.0)
    body = None
    if shape.get("filled") and kind in ("ring", "circle"):
        if kind == "ring" and len(pts) >= 3:
            body = pts
        elif kind == "circle":
            (x, y), (px, py) = pts
            r = math.hypot(px - x, py - y) + SAGITTA_MM + MIN_HALF_MM
            n = max(8, _chords(r, 2.0 * math.pi))
            step = 2 * math.pi / n
            body = [(x + r * math.cos(step * i), y + r * math.sin(step * i)) for i in range(n)]
        if width <= 0:
            return body, []
    pieces, curved = _pieces(shape)
    w = max(width / 2, MIN_HALF_MM) + (SAGITTA_MM + MIN_HALF_MM if curved else 0.0)
    return body, [_quad(p, q, w) for p, q in pieces]


def apply(dsn_text, facts):
    """The DSN with the keepouts of `facts` (`kicad_board.edge_graphics`), and
    the log lines; unchanged with nothing to keep out."""
    dsn = Dsn(dsn_text)
    rects, shapes = quads(facts, dsn.default_clearance_mm())
    masks, mask_shapes, fp_mask_shapes = mask_polygons(facts)
    if not rects and not masks:
        return dsn_text, []
    if dsn.structure is None:
        raise ValueError("no (structure ...) in the DSN")
    layers = [name for name, _kind, _atom in dsn.layers]

    def keepout(name, layer, poly, clearance_class=None):
        coords = " ".join(f"{um(x)} {-um(y)}" for x, y in poly + poly[:1])
        cls = f" (clearance_class {clearance_class})" if clearance_class else ""
        return f'    (keepout "{name}" (polygon {dsn.layer_token(layer)} 0 {coords}){cls}){dsn.nl}'

    lines = [keepout(KEEPOUT_NAME, layer, quad) for layer in layers for quad in rects]
    mask_lines = [
        keepout(name, layers[-1] if side else layers[0], poly, MASK_CLEARANCE_CLASS) for side, poly, name, _fp in masks
    ]
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
    if mask_shapes:
        log.append(
            f"solder-mask graphics: {mask_shapes} board F.Mask/B.Mask shape(s) kept clear on their copper side "
            f"({sum(1 for m in masks if not m[3])} keepouts)"
        )
    if fp_mask_shapes:
        log.append(
            f"solder-mask graphics: {fp_mask_shapes} footprint F.Mask/B.Mask shape(s) kept clear on their copper "
            f"side, but for their pads ({sum(1 for m in masks if m[3])} keepouts)"
        )
        text, trim_log = pad_trim.trim(text)
        log += trim_log
    return text, log
