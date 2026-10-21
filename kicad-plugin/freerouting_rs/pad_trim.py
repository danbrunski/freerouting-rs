"""Q22.16: pads cut back to their part outside a footprint mask aperture that
pads of two or more nets share (decided 2026-10-20: routing never breaks the
board's manufacturing rules, option (b)).

KiCad's DRC (`solder_mask_bridge`) flags copper of a second net inside a mask
aperture, and inside an aperture that pads of several nets share it flags a
track of a pad's own net too. The router ends a trace at the pin's centre, so
a trace to an edge connector's finger, whose centre lies inside the
connector's one aperture (fc-rom's J1), runs along the finger into it.
`trim` works on the DSN, after `edge_graphics.apply` has added the
footprints' `solder_mask_pads` keepouts:

1. A keepout over pads (any footprint's, on its copper layer, overlapping by
   more than `EPS_UM2`) of two or more nets is renamed `SHARED_NAME`: the
   router keeps every net out of it, the pads' own too.
2. Each one-layer pad that overlaps such a keepout is given to the router as
   its largest part outside them all: the pad cut by the outer half-plane of
   one keepout edge (each edge of each such keepout tried in file order, the
   largest area that overlaps none of them kept, the first of equal ones).
   Its image pin gets a new padstack of that part
   (`<padstack>_outside_mask_<k>`, a polygon on the same layer, corners
   rounded to 1 um) and its anchor at the part's centroid, unrotated. An image
   used by other components too gets a copy for the component
   (`<image>_<ref>`), unless all of them change alike. Rects, convex polygons,
   circles (their inscribed 16-gon) and paths (the hull of 16-gons at their
   points) are cut; a pad on several layers stays as it is, since it can be
   reached on another layer.
3. A pad with no part outside (it lies inside the aperture) or that cannot be
   cut (a concave polygon) is named in the log with its net: it cannot be
   reached without a bridge, so the router leaves it unrouted.

The SES carries only wires and vias, so the cut pads never reach KiCad; a
trace that ends at the anchor ends inside the real pad. The `.kicad_pcb`
reader does the same (`crates/freerouting-kicad/src/pad_trim.rs`, operation
for operation, so both give the same pads). Pure Python.
"""

import math

try:  # inside the plugin package
    from .dsn_fixups import Dsn, _children, _unquote
except ImportError:  # tests and headless scripts put this folder on sys.path
    from dsn_fixups import Dsn, _children, _unquote

PADS_NAME = "solder_mask_pads"
SHARED_NAME = "solder_mask_shared"
EPS_UM2 = 1.0  # overlap below this many um^2 is touching, not overlapping
CIRCLE_CORNERS = 16


def _num(node):
    return float(node.text)


def _turn(p, deg):
    """`p` turned counterclockwise by `deg` degrees (exact for multiples of 90)."""
    if deg % 90.0 == 0.0:
        k = int(math.floor(deg / 90.0)) % 4
        return [p, (-p[1], p[0]), (-p[0], -p[1]), (p[1], -p[0])][k]
    a = math.radians(deg)
    s, c = math.sin(a), math.cos(a)
    return (p[0] * c - p[1] * s, p[0] * s + p[1] * c)


def _round(v):
    return math.floor(v + 0.5)


class Frame:
    """Placement of an image: location, rotation (degrees), mirrored (back)."""

    def __init__(self, at, rotation, mirrored, rotate_first):
        self.at, self.rotation, self.mirrored, self.rotate_first = at, rotation, mirrored, rotate_first

    def to_board(self, p):
        v = p
        if self.mirrored and not self.rotate_first:
            v = (-v[0], v[1])
        v = _turn(v, self.rotation)
        if self.mirrored and self.rotate_first:
            v = (-v[0], v[1])
        return (self.at[0] + v[0], self.at[1] + v[1])

    def to_image(self, p):
        v = (p[0] - self.at[0], p[1] - self.at[1])
        if self.mirrored and self.rotate_first:
            v = (-v[0], v[1])
        v = _turn(v, -self.rotation)
        if self.mirrored and not self.rotate_first:
            v = (-v[0], v[1])
        return v


def _cross(a, b, p):
    return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])


def _area2(poly):
    """Twice the signed area (counterclockwise positive)."""
    n = len(poly)
    total = 0.0
    for i in range(n):
        p, q = poly[i], poly[(i + 1) % n]
        total += p[0] * q[1] - q[0] * p[1]
    return total


def _ccw(poly):
    return list(reversed(poly)) if _area2(poly) < 0.0 else poly


def _signum(v):
    return 1.0 if v > 0 else (-1.0 if v < 0 else 0.0)


def _is_convex(ring):
    n = len(ring)
    return all(_cross(ring[i], ring[(i + 1) % n], ring[(i + 2) % n]) >= 0.0 for i in range(n))


def _clip(poly, a, b, sign):
    """`poly` cut by the line a-b, keeping the side where sign * cross >= 0."""
    n = len(poly)
    out = []
    for i in range(n):
        p, q = poly[i], poly[(i + 1) % n]
        sp, sq = sign * _cross(a, b, p), sign * _cross(a, b, q)
        if sp >= 0.0:
            out.append(p)
        if (sp >= 0.0) != (sq >= 0.0):
            t = sp / (sp - sq)
            out.append((p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t))
    return out


def overlap(ring, convex):
    """The area (um^2) where `ring` and the counterclockwise convex `convex` overlap."""
    n = len(convex)
    part = list(ring)
    for i in range(n):
        if not part:
            return 0.0
        part = _clip(part, convex[i], convex[(i + 1) % n], 1.0)
    return abs(_area2(part) / 2.0)


def _centroid(poly):
    o = poly[0]
    a = cx = cy = 0.0
    n = len(poly)
    for i in range(n):
        p, q = poly[i], poly[(i + 1) % n]
        px, py, qx, qy = p[0] - o[0], p[1] - o[1], q[0] - o[0], q[1] - o[1]
        c = px * qy - qx * py
        a += c
        cx += (px + qx) * c
        cy += (py + qy) * c
    return (o[0] + cx / (3.0 * a), o[1] + cy / (3.0 * a))


def _circle(c, r):
    out = []
    for i in range(CIRCLE_CORNERS):
        a = 2.0 * math.pi * i / CIRCLE_CORNERS
        out.append((c[0] + r * math.cos(a), c[1] + r * math.sin(a)))
    return out


def _hull(pts):
    pts = sorted(set(pts))
    if len(pts) < 3:
        return pts
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and _cross(lower[-2], lower[-1], p) <= 0.0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and _cross(upper[-2], upper[-1], p) <= 0.0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def _open_ring(pts):
    ring = list(pts)
    while len(ring) > 1 and ring[0] == ring[-1]:
        ring.pop()
    return ring


def _pairs(nums):
    return [(nums[i], nums[i + 1]) for i in range(0, len(nums) - 1, 2)]


def _shape(node):
    """(layer token, polygon in the pin frame, convex?) of a padstack shape, or None."""
    kind = node.head
    args = node.children[1:]
    if not args or args[0].is_list:
        return None
    layer = args[0].text
    nums = [_num(a) for a in args[1:] if not a.is_list]
    if kind == "rect" and len(nums) >= 4:
        x1, y1, x2, y2 = nums[:4]
        x0, x1, y0, y1 = min(x1, x2), max(x1, x2), min(y1, y2), max(y1, y2)
        return layer, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], True
    if kind == "polygon" and len(nums) >= 7:
        ring = _ccw(_open_ring(_pairs(nums[1:])))
        return (layer, ring, _is_convex(ring)) if len(ring) >= 3 else None
    if kind == "circle" and nums:
        c = (nums[1], nums[2]) if len(nums) >= 3 else (0.0, 0.0)
        return layer, _circle(c, nums[0] / 2.0), True
    if kind == "path" and len(nums) >= 3:
        pts = []
        for p in _pairs(nums[1:]):
            pts += _circle(p, nums[0] / 2.0)
        return layer, _hull(pts), True
    return None


class _Pad:
    __slots__ = ("comp", "place", "pin", "name", "net", "layer", "poly", "cuttable", "one_layer")

    def __init__(self, **kw):
        for k, v in kw.items():
            setattr(self, k, v)


def _library(dsn):
    library = next(iter(_children(dsn.pcb, "library")), None)
    images, padstacks = {}, {}
    if library is None:
        return library, images, padstacks
    for img in _children(library, "image"):
        images.setdefault(_unquote(img.children[1].text), img)
    for ps in _children(library, "padstack"):
        padstacks.setdefault(_unquote(ps.children[1].text), ps)
    return library, images, padstacks


def _image_pins(img):
    """(node, padstack, rotation, name, x, y) of each `(pin ...)` of an image."""
    out = []
    for pin in _children(img, "pin"):
        parts = pin.children[1:]
        rot = 0.0
        atoms = []
        for p in parts:
            if p.is_list:
                if p.head == "rotate" and len(p.children) > 1:
                    rot = _num(p.children[1])
            else:
                atoms.append(p)
        if len(atoms) < 4:
            continue
        out.append((pin, _unquote(atoms[0].text), rot, _unquote(atoms[1].text), _num(atoms[2]), _num(atoms[3])))
    return out


def _padstack_flags(ps):
    flags = {}
    for head in ("attach", "absolute"):
        node = next(iter(_children(ps, head)), None)
        if node is not None and len(node.children) > 1:
            flags[head] = node.children[1].text
    return flags


def _placements(dsn):
    placement = next(iter(_children(dsn.pcb, "placement")), None)
    rotate_first = False
    comps = []  # (component node, image name, [(place node, ref, Frame or None)])
    if placement is None:
        return placement, comps, rotate_first
    for pc in _children(placement, "place_control"):
        for fs in _children(pc, "flip_style"):
            rotate_first = any(c.text == "rotate_first" for c in fs.children[1:] if not c.is_list)
    for comp in _children(placement, "component"):
        places = []
        for place in _children(comp, "place"):
            atoms = [c for c in place.children[1:] if not c.is_list]
            ref = _unquote(atoms[0].text)
            frame = None
            if len(atoms) >= 5:
                frame = Frame((_num(atoms[1]), _num(atoms[2])), _num(atoms[4]), atoms[3].text == "back", rotate_first)
            places.append((place, ref, frame))
        comps.append((comp, _unquote(comp.children[1].text), places))
    return placement, comps, rotate_first


def _nets(dsn):
    nets = {}
    for net in _children(dsn.network, "net") if dsn.network else []:
        name = _unquote(net.children[1].text)
        for scope in _children(net, "pins") + _children(net, "fromto"):
            for tok in scope.children[1:]:
                if not tok.is_list:
                    nets.setdefault(_unquote(tok.text), name)
    return nets


def _board_pads(dsn, layers, images, padstacks, comps):
    nets = _nets(dsn)
    out = []
    for ci, (_node, image, places) in enumerate(comps):
        img = images.get(image)
        if img is None:
            continue
        pins = _image_pins(img)
        for pi, (_pnode, ref, frame) in enumerate(places):
            if frame is None:
                continue
            for k, (_pin, psname, rot, name, x, y) in enumerate(pins):
                ps = padstacks.get(psname)
                if ps is None:
                    continue
                shapes = [s.children[1] for s in _children(ps, "shape") if len(s.children) > 1 and s.children[1].is_list]
                one_layer = len(shapes) == 1
                absolute = _padstack_flags(ps).get("absolute") == "on"
                for shape in shapes:
                    got = _shape(shape)
                    if got is None:
                        continue
                    layer, local, convex = got
                    layer = _unquote(layer)
                    if frame.mirrored and not absolute and layer in layers:
                        layer = layers[len(layers) - 1 - layers.index(layer)]
                    poly = []
                    for q in local:
                        t = _turn(q, rot)
                        poly.append(frame.to_board((x + t[0], y + t[1])))
                    out.append(
                        _Pad(
                            comp=ci,
                            place=pi,
                            pin=k,
                            name=f"{ref}-{name}",
                            net=nets.get(f"{ref}-{name}"),
                            layer=layer,
                            poly=_ccw(poly),
                            cuttable=one_layer and convex,
                            one_layer=one_layer,
                        )
                    )
    return out


def _outside_part(pad, under):
    best = None
    for ring in under:
        sign = -_signum(_area2(ring))
        n = len(ring)
        for i in range(n):
            a, b = ring[i], ring[(i + 1) % n]
            if a == b:
                continue
            part = _clip(pad, a, b, sign)
            size = _area2(part) / 2.0 if len(part) >= 3 else 0.0
            if size <= EPS_UM2 or any(overlap(r, part) > EPS_UM2 for r in under):
                continue
            if best is None or size > best[0]:
                best = (size, part)
    return None if best is None else best[1]


def _new_pin(pad, part, comps, images, padstacks):
    """(pin index, (padstack name, layer token, corners, flags), (x, y)) or None."""
    _node, image, places = comps[pad.comp]
    frame = places[pad.place][2]
    pin = _image_pins(images[image])[pad.pin]
    ps = padstacks[pin[1]]
    shape = next(s.children[1] for s in _children(ps, "shape") if len(s.children) > 1 and s.children[1].is_list)
    layer = shape.children[1].text
    c = frame.to_image(_centroid(part))
    anchor = (_round(c[0]), _round(c[1]))
    corners = []
    for q in part:
        x, y = frame.to_image(q)
        p = (_round(x - anchor[0]), _round(y - anchor[1]))
        if not corners or corners[-1] != p:
            corners.append(p)
    while len(corners) > 1 and corners[0] == corners[-1]:
        corners.pop()
    if len(corners) < 3:
        return None
    corners.append(corners[0])
    return pad.pin, (pin[1], layer, tuple(corners), tuple(sorted(_padstack_flags(ps).items()))), anchor


def _fmt(v):
    return str(int(v))


def _token(name, like):
    """`name` quoted when `like` (the DSN's spelling of its base) was, or when it needs it."""
    if like.startswith('"') or any(c in name for c in ' ()"'):
        return f'"{name}"'
    return name


def _line_start(text, pos):
    """The offset of the line `pos` is on, and that line's indentation."""
    at = text.rfind("\n", 0, pos) + 1
    indent = text[at:pos]
    return at, indent if indent.strip() == "" else ""


def trim(dsn_text):
    """The DSN with the module's steps applied, and the log lines; unchanged
    without a footprint keepout over pads of two nets."""
    dsn = Dsn(dsn_text)
    if dsn.structure is None:
        return dsn_text, []
    keepouts = []
    for k in _children(dsn.structure, "keepout"):
        if len(k.children) < 3 or k.children[1].is_list or _unquote(k.children[1].text) != PADS_NAME:
            continue
        poly = next((c for c in k.children[2:] if c.is_list and c.head == "polygon"), None)
        if poly is None or len(poly.children) < 9:
            continue
        nums = [_num(c) for c in poly.children[3:] if not c.is_list]
        keepouts.append((k, _unquote(poly.children[1].text), _open_ring(_pairs(nums))))
    if not keepouts:
        return dsn_text, []
    layers = [name for name, _kind, _atom in dsn.layers]
    library, images, padstacks = _library(dsn)
    placement, comps, _rf = _placements(dsn)
    pads = _board_pads(dsn, layers, images, padstacks, comps)
    edits = []  # (start, end, text)
    shared = []
    for node, layer, ring in keepouts:
        nets = sorted({p.net for p in pads if p.layer == layer and p.net and overlap(ring, p.poly) > EPS_UM2})
        if len(nets) >= 2:
            shared.append((layer, ring))
            tok = node.children[1]
            edits.append((tok.start, tok.end, f'"{SHARED_NAME}"'))
    log, changes = [], {}
    for pad in pads:
        under = [ring for layer, ring in shared if layer == pad.layer and overlap(ring, pad.poly) > EPS_UM2]
        if not under or not pad.one_layer:
            continue
        part = _outside_part(pad.poly, under) if pad.cuttable else None
        change = _new_pin(pad, part, comps, images, padstacks) if part else None
        if change is None:
            log.append(
                f"WARNING: pad {pad.name} (net {pad.net or ''}) lies inside a solder-mask aperture on {pad.layer} "
                "that pads of other nets share: it cannot be reached without a bridge, so it is left unrouted"
            )
            continue
        changes.setdefault((pad.comp, pad.place), []).append(change)
    if not shared:
        return dsn_text, []
    edits += _write_changes(dsn, library, images, padstacks, placement, comps, changes)
    text = dsn_text
    for start, end, new in sorted(edits, key=lambda e: (e[0], e[1]), reverse=True):
        text = text[:start] + new + text[end:]
    cut = sum(len(v) for v in changes.values())
    log.insert(
        0,
        f"solder-mask graphics: {len(shared)} footprint aperture keepout(s) over pads of several nets keep every net "
        f"out; {cut} pad(s) cut back to their part outside",
    )
    return text, log


def _write_changes(dsn, library, images, padstacks, placement, comps, changes):
    text, nl = dsn.text, dsn.nl
    named = []  # (base, key, name)
    per_place = {}
    for key in sorted(changes):
        for k, (base, layer, corners, flags), anchor in changes[key]:
            found = next((n for b, kk, n in named if b == base and kk == (layer, corners, flags)), None)
            if found is None:
                count = sum(1 for b, _kk, _n in named if b == base)
                found = f"{base}_outside_mask_{count + 1}"
                named.append((base, (layer, corners, flags), found))
            per_place.setdefault(key, []).append((k, found, anchor))
    edits = []
    last_ps = _children(library, "padstack")[-1]
    _at, ind = _line_start(text, last_ps.start)
    blocks = []
    for base, (layer, corners, flags), name in named:
        coords = " ".join(f"{_fmt(x)} {_fmt(y)}" for x, y in corners)
        lines = [f"{ind}(padstack {_token(name, padstacks[base].children[1].text)}"]
        lines.append(f"{ind}  (shape (polygon {layer} 0 {coords}))")
        lines += [f"{ind}  ({head} {value})" for head, value in flags]
        lines.append(f"{ind})")
        blocks.append(nl.join(lines))
    edits.append((last_ps.end, last_ps.end, "".join(nl + b for b in blocks)))

    def pin_text(img, k, name, anchor):
        pin = _image_pins(img)[k]
        return f"(pin {_token(name, pin[0].children[1].text)} {pin[0].children[-3].text} {_fmt(anchor[0])} {_fmt(anchor[1])})"

    users = {}
    for _node, image, places in comps:
        users[image] = users.get(image, 0) + len(places)
    last_img = _children(library, "image")[-1]
    copies = []
    for ci, (comp, image, places) in enumerate(comps):
        changed = [pi for pi in range(len(places)) if (ci, pi) in per_place]
        if not changed:
            continue
        img = images[image]
        alike = (
            users[image] == len(places)
            and len(changed) == len(places)
            and all(per_place[(ci, pi)] == per_place[(ci, 0)] for pi in changed)
        )
        if alike:
            pins = _image_pins(img)
            for k, name, anchor in per_place[(ci, 0)]:
                edits.append((pins[k][0].start, pins[k][0].end, pin_text(img, k, name, anchor)))
            continue
        block_lines = []
        whole = len(changed) == len(places)
        if whole:  # every place moves: the block goes
            edits.append((_line_break(text, comp.start), comp.end, ""))
        for pi in changed:
            place, ref, _frame = places[pi]
            new_name = _unique_image(f"{image}_{ref}", images, copies)
            body = text[img.start : img.end]
            subs = [(img.children[1].start, img.children[1].end, _token(new_name, img.children[1].text))]
            pins = _image_pins(img)
            for k, name, anchor in per_place[(ci, pi)]:
                subs.append((pins[k][0].start, pins[k][0].end, pin_text(img, k, name, anchor)))
            for start, end, new in sorted(subs, reverse=True):
                body = body[: start - img.start] + new + body[end - img.start :]
            _iat, iind = _line_start(text, img.start)
            copies.append((new_name, iind + body))
            if not whole:
                edits.append((_line_break(text, place.start), place.end, ""))
            _cat, cind = _line_start(text, comp.start)
            _pat, pind = _line_start(text, place.start)
            block_lines.append(
                f"{cind}(component {_token(new_name, comp.children[1].text)}{nl}{pind}{text[place.start : place.end]}{nl}{cind})"
            )
        edits.append((comp.end, comp.end, "".join(nl + b for b in block_lines)))
    if copies:
        edits.append((last_img.end, last_img.end, "".join(nl + body for _n, body in copies)))
    return edits


def _line_break(text, pos):
    """The offset of the line break before `pos` (with its `\r`)."""
    at = text.rfind("\n", 0, pos)
    return at - 1 if at > 0 and text[at - 1] == "\r" else max(at, 0)


def _unique_image(base, images, copies):
    name, n = base, 1
    taken = set(images) | {c[0] for c in copies}
    while name in taken:
        n += 1
        name = f"{base}_{n}"
    return name
