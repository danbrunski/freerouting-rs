"""DSN fix-ups after KiCad's export (K09): the transforms of canary's
`dsn_export.py`, each one an option of the plugin.

Provenance: ported from `hardware/tools/dsn_export.py` in Dan Brunski's own
`danbrunski/canary` repository (commit `dbfe7b67`, the board's last change;
the script is unchanged since `bf594116`), written for the CANary-PWR board.
No third-party code. The arithmetic (`fill_rects`, `_wire`, the edge strips
and the pad circles) is the script's, operation for operation, so the same
board gives the same numbers.

KiCad's export, and how the router reads it, needs these corrections on a
board like `canary-pwr` (the step numbers are the script's):

2. `fixed_pour_classes`: KiCad writes every pour as a `(plane ...)`, a
   conduction area other nets may cross, so signals get routed through power
   pours and cut them when KiCad refills. Each outer-layer pour of a net in
   a listed class is replaced by fixed wires covering its fill (`fill_rects`
   cuts the fill into vertical strips of `SLAB_MM`, merges equal neighbours,
   and `_wire` covers each rectangle with one path as wide as its short
   side): other nets keep clear, and same-net taps connect to its edge. The
   router does not write fixed wires into the session, so
   `ImportSpecctraSES` never sees them.
3. `drop_pour_nets`: outer-layer pours of the listed nets (canary: GND) are
   dropped, so their pads fan out to the inner plane instead of counting as
   joined through a pour that routing then breaks up.
4. `class_widths`: a class's routed width, written into its
   `(class ... (rule (width W)))`. The other classes stay byte for byte.
5. `edge_and_pad_keepouts`: the board-edge clearance and pads' own larger
   clearances (fiducial mask openings) are not exported. Both go in as
   keepouts after the layer definitions (before the structure's `(via ...)`):
   strips along the inside of the outline's bounding box, `edge - default
   clearance` wide, on every signal layer (dsn_export.py's step), plus, for
   each outline segment that is not on a side of the box (rounded or angled
   corners, notches, cut-outs: KiCad's CM5_MINIMA_3 demo had 12 edge errors
   along such segments with the box alone), a rectangle reaching that far
   on both sides of it; and a circle round each such pad. The router keeps
   its default clearance from a keepout; the keepout makes up the rest.
6. `route_on_plane_layers`: inner plane layers opened to routing, `(type
   power)` becomes `(type signal)`; their planes stay conduction areas.

Step 1 (rule areas) is `rule_areas.rewrite` (K01.2), run before these.

`"auto"` (the plugin's `enhanced` defaults, see `options.PROFILE_DEFAULTS`)
derives the lists from the board instead of naming them:
- fixed pour classes: the classes of the outer-layer pours whose zone
  priority is above the lowest priority on their layer. The lowest-priority
  pours are the flood fills that give way to tracks; the ones above them are
  hand-laid copper (on `canary-pwr`: Power_High and Switch_Node over GND).
- dropped pour nets: a net that is the only one with a zone on some inner
  copper layer (a full plane, canary's GND on In1) loses its outer pours.
- class widths: a class wider than the board's default width is routed no
  wider than the narrowest SMD pad of its nets (never below the board's
  minimum track width): a 1.5 mm class cannot reach a 0.25 mm QFN pin.

Pure Python: no pcbnew, no wx. The board facts come from
`kicad_board.collect` as plain dicts.
"""

import re

try:  # inside the plugin package
    from .rule_areas import parse as parse_tree
except ImportError:  # tests and headless scripts put this folder on sys.path
    from rule_areas import parse as parse_tree

AUTO = "auto"
SLAB_MM = 0.25  # width of the vertical strips a pour fill is cut into
MIN_MM = 0.05  # strips thinner than this are dropped
DEFAULT_CLEARANCE_MM = 0.2  # used when the DSN's structure rule has no clearance


def um(v):
    return round(v * 1000)


# --- fill rectangles (dsn_export.py, verbatim arithmetic) ---------------------


def _crossings(chains, x):
    ys = []
    for pts in chains:
        for (xa, ya), (xb, yb) in zip(pts, pts[1:] + pts[:1]):
            if (xa <= x < xb) or (xb <= x < xa):
                ys.append(ya + (x - xa) * (yb - ya) / (xb - xa))
    ys.sort()
    return ys


def _intervals(chains, x):
    ys = _crossings(chains, x)
    return [(ys[i], ys[i + 1]) for i in range(0, len(ys) - 1, 2)]


def _intersect(a, b):
    out = []
    for a0, a1 in a:
        for b0, b1 in b:
            lo, hi = max(a0, b0), min(a1, b1)
            if hi - lo >= MIN_MM:
                out.append((lo, hi))
    return out


def fill_rects(chains):
    """Rectangles (x0, y0, x1, y1) in mm inside a polygon set (outlines and
    holes, even-odd), from vertical strips."""
    chains = [list(c) for c in chains if c]
    if not chains:
        return
    xs = [x for pts in chains for x, _ in pts]
    x, x_end = min(xs), max(xs)
    strips = []
    while x < x_end - MIN_MM:
        x1 = min(x + SLAB_MM, x_end)
        inside = _intersect(_intervals(chains, x + 1e-4), _intervals(chains, x1 - 1e-4))
        strips.append((x, x1, inside))
        x = x1
    # merge neighbouring strips with the same intervals into wider rectangles
    i = 0
    while i < len(strips):
        x0, x1, ivs = strips[i]
        j = i + 1
        while (
            j < len(strips)
            and len(strips[j][2]) == len(ivs)
            and all(abs(p[0] - q[0]) < 1e-3 and abs(p[1] - q[1]) < 1e-3 for p, q in zip(strips[j][2], ivs))
        ):
            x1 = strips[j][1]
            j += 1
        for y0, y1 in ivs:
            yield (x0, y0, x1, y1)
        i = j


def _wire(layer, net, rect, nl):
    """A fixed path covering a rectangle: along its long side, as wide as its short side."""
    x0, y0, x1, y1 = rect
    w, h = x1 - x0, y1 - y0
    if w >= h:
        ya = (y0 + y1) / 2
        pts = (x0 + h / 2, ya, max(x1 - h / 2, x0 + h / 2 + 0.001), ya)
        width = h
    else:
        xa = (x0 + x1) / 2
        pts = (xa, y0 + w / 2, xa, max(y1 - w / 2, y0 + w / 2 + 0.001))
        width = w
    coords = f"{um(pts[0])} {-um(pts[1])} {um(pts[2])} {-um(pts[3])}"
    return f"    (wire (path {layer} {um(width)} {coords}) (net {net}) (type fix)){nl}"


# --- reading the DSN ----------------------------------------------------------


def _unquote(text):
    return text[1:-1] if len(text) >= 2 and text[0] == text[-1] == '"' else text


def _children(node, head):
    return [c for c in node.children if c.is_list and c.head == head]


class Dsn:
    """The parts of an exported DSN the fix-ups read, with source spans."""

    def __init__(self, text):
        self.text = text
        self.nl = "\r\n" if "\r\n" in text else "\n"
        roots = parse_tree(text)
        pcb = next((r for r in roots if r.head == "pcb"), None)
        if pcb is None:
            raise ValueError("no (pcb ...) in the DSN")
        self.pcb = pcb
        self.structure = next(iter(_children(pcb, "structure")), None)
        self.network = next(iter(_children(pcb, "network")), None)
        self.wiring = next(iter(_children(pcb, "wiring")), None)
        self.classes = {}  # name -> node
        self.net_class = {}  # net -> class name (the DSN's own class lists)
        for cls in _children(self.network, "class") if self.network else []:
            parts = cls.children
            if len(parts) < 2 or parts[1].is_list:
                continue
            name = _unquote(parts[1].text)
            self.classes[name] = cls
            for tok in parts[2:]:
                if tok.is_list:
                    break
                self.net_class[_unquote(tok.text)] = name
        self.layers = []  # (name, type, type atom node)
        for layer in _children(self.structure, "layer") if self.structure else []:
            name = _unquote(layer.children[1].text)
            kind, atom = None, None
            for t in _children(layer, "type"):
                if len(t.children) > 1:
                    atom = t.children[1]
                    kind = atom.text
            self.layers.append((name, kind, atom))

    def default_clearance_mm(self):
        for rule in _children(self.structure, "rule") if self.structure else []:
            for c in _children(rule, "clearance"):
                if len(c.children) == 2 and not c.children[1].is_list:
                    try:
                        return float(c.children[1].text) / 1000.0
                    except ValueError:
                        pass
        return DEFAULT_CLEARANCE_MM

    def default_width_mm(self):
        for rule in _children(self.structure, "rule") if self.structure else []:
            for w in _children(rule, "width"):
                try:
                    return float(w.children[1].text) / 1000.0
                except (IndexError, ValueError):
                    pass
        return None

    def class_width_node(self, name):
        cls = self.classes.get(name)
        if cls is None:
            return None
        for rule in _children(cls, "rule"):
            for w in _children(rule, "width"):
                if len(w.children) > 1 and not w.children[1].is_list:
                    return w.children[1]
        return None

    def planes(self):
        """(node, net, layer) of the structure's `(plane ...)` scopes."""
        out = []
        for plane in _children(self.structure, "plane") if self.structure else []:
            parts = plane.children
            poly = next((c for c in parts[2:] if c.is_list), None)
            if len(parts) < 3 or parts[1].is_list or poly is None or len(poly.children) < 2:
                continue
            out.append((plane, _unquote(parts[1].text), _unquote(poly.children[1].text)))
        return out

    def net_token(self, name):
        """The net's name as the DSN's network section spells it."""
        quoted = f'"{name}"'
        return quoted if f"(net {quoted}" in self.text else name


# --- deriving the "auto" lists from the board ---------------------------------


def auto_fixed_pour_classes(board, dsn):
    """Classes of the outer pours above the lowest zone priority of their layer."""
    lowest = {}
    for z in board.get("zones", []):
        if z["outer"]:
            lowest[z["layer"]] = min(lowest.get(z["layer"], z["priority"]), z["priority"])
    classes = set()
    for z in board.get("zones", []):
        if z["outer"] and z["priority"] > lowest[z["layer"]] and z["net"] in dsn.net_class:
            classes.add(dsn.net_class[z["net"]])
    return sorted(classes)


def auto_drop_pour_nets(board):
    """Nets that are the only net with a zone on some inner copper layer."""
    by_layer = {}
    for z in board.get("zones", []):
        if not z["outer"] and z["net"]:
            by_layer.setdefault(z["layer"], set()).add(z["net"])
    return sorted({next(iter(nets)) for nets in by_layer.values() if len(nets) == 1})


def auto_class_widths(board, dsn):
    """{class: mm} for the classes wider than the board's default width that
    have an SMD pad narrower than their width: that pad's width, at least
    the board's minimum track width."""
    default = dsn.default_width_mm()
    pad_min = board.get("smd_pad_min_mm") or {}
    floor = board.get("min_track_mm") or 0.0
    out = {}
    for name in sorted(dsn.classes):
        node = dsn.class_width_node(name)
        if node is None:
            continue
        try:
            width = float(node.text) / 1000.0
        except ValueError:
            continue
        if default is not None and width <= default:
            continue
        nets = [n for n, c in dsn.net_class.items() if c == name and n in pad_min]
        if not nets:
            continue
        narrowest = min(pad_min[n] for n in nets)
        if narrowest < width:
            out[name] = max(narrowest, floor)
    return out


def resolve(settings, board, dsn):
    """The settings with every `"auto"` replaced by the list it stands for."""
    out = dict(settings)
    if out.get("fixed_pour_classes") == AUTO:
        out["fixed_pour_classes"] = auto_fixed_pour_classes(board, dsn)
    if out.get("drop_pour_nets") == AUTO:
        out["drop_pour_nets"] = auto_drop_pour_nets(board)
    if out.get("class_widths") == AUTO:
        out["class_widths"] = auto_class_widths(board, dsn)
    return out


# --- the transforms -------------------------------------------------------------


def _edge_keepouts(board, dsn, layers):
    """Strips along the inside of the outline's bounding box (dsn_export.py)."""
    bbox = board.get("outline_bbox_mm")
    edge = board.get("edge_clearance_mm") or 0.0
    w = edge - dsn.default_clearance_mm()
    if bbox is None or w <= 0:
        return []
    x0, y0, x1, y1 = bbox
    o = 1.0  # the strips reach past the edge, where nothing routes anyway
    strips = [
        (x0 - o, y0 - o, x1 + o, y0 + w),
        (x0 - o, y1 - w, x1 + o, y1 + o),
        (x0 - o, y0 - o, x0 + w, y1 + o),
        (x1 - w, y0 - o, x1 + o, y1 + o),
    ]
    out = [
        f'    (keepout "" (rect {layer} {um(a)} {-um(d)} {um(c)} {-um(b)})){dsn.nl}'
        for layer in layers
        for a, b, c, d in strips
    ]
    quads = [_quad(p, q, w) for p, q in _off_bbox_segments(board.get("outline_rings_mm") or [], bbox)]
    for layer in layers:
        for quad in quads:
            coords = " ".join(f"{um(x)} {-um(y)}" for x, y in quad + quad[:1])
            out.append(f'    (keepout "" (polygon {layer} 0 {coords})){dsn.nl}')
    return out


def _off_bbox_segments(rings, bbox, tol=1e-3):
    """The outline segments (outer rings and holes) that do not lie on a side
    of the bounding box, which the strips already cover: rounded or angled
    corners, notches, cut-outs."""
    x0, y0, x1, y1 = bbox
    for ring in rings:
        for p, q in zip(ring, ring[1:] + ring[:1]):
            if p == q:
                continue
            on_side = any(abs(p[0] - v) < tol and abs(q[0] - v) < tol for v in (x0, x1)) or any(
                abs(p[1] - v) < tol and abs(q[1] - v) < tol for v in (y0, y1)
            )
            if not on_side:
                yield p, q


def _quad(p, q, w):
    """The rectangle within `w` of the segment p-q, reaching `w` past both ends."""
    dx, dy = q[0] - p[0], q[1] - p[1]
    length = (dx * dx + dy * dy) ** 0.5
    ux, uy = dx / length * w, dy / length * w
    nx, ny = -uy, ux
    a = (p[0] - ux, p[1] - uy)
    b = (q[0] + ux, q[1] + uy)
    return [(a[0] + nx, a[1] + ny), (b[0] + nx, b[1] + ny), (b[0] - nx, b[1] - ny), (a[0] - nx, a[1] - ny)]


def _pad_keepouts(board, dsn, layers):
    """A circle round each pad whose own clearance exceeds the default."""
    default = dsn.default_clearance_mm()
    out = []
    for pad in board.get("pads", []):
        local = pad["local_clearance_mm"]
        if local <= default:
            continue
        sx, sy = pad["size_mm"]
        dia = max(sx, sy) + 2 * (local - default)
        x, y = pad["at_mm"]
        for layer in pad["layers"]:
            if layer in layers:
                out.append(f'    (keepout "" (circle {layer} {um(dia)} {um(x)} {-um(y)})){dsn.nl}')
    return out


def apply(dsn_text, board, settings):
    """Applies the fix-ups `settings` asks for (keys: `class_widths` {class:
    mm}, `fixed_pour_classes` [class], `drop_pour_nets` [net],
    `edge_and_pad_keepouts` bool, `route_on_plane_layers` [layer]; any list
    may be `"auto"`). Returns `(text, log_lines)`; with nothing to do the
    text is returned unchanged. Raises ValueError on a DSN it cannot read or
    a class or layer it does not have."""
    dsn = Dsn(dsn_text)
    s = resolve(settings, board, dsn)
    fixed_classes = set(s.get("fixed_pour_classes") or [])
    drop_nets = set(s.get("drop_pour_nets") or [])
    widths = s.get("class_widths") or {}
    open_layers = list(s.get("route_on_plane_layers") or [])
    log = []
    edits = []  # (start, end, replacement)

    # 2./3. outer-layer pours: listed classes become fixed wires, dropped nets go
    fixed_wires, fixed_nets = [], set()
    for z in board.get("zones", []):
        if not z["outer"] or not z["filled"]:
            continue
        net = z["net"]
        if net in drop_nets or dsn.net_class.get(net) not in fixed_classes:
            continue
        fixed_nets.add((net, z["layer"]))
        token = dsn.net_token(net)
        for rect in fill_rects(z["fill"]):
            fixed_wires.append(_wire(z["layer"], token, rect, dsn.nl))
    outer_layers = {z["layer"] for z in board.get("zones", []) if z["outer"]} | {"F.Cu", "B.Cu"}
    n_planes = 0
    for node, net, layer in dsn.planes():
        if layer in outer_layers and (net in drop_nets or (net, layer) in fixed_nets):
            edits.append((node.start, node.end, ""))
            n_planes += 1
    if fixed_classes or drop_nets:
        log.append(
            f"pours: {len(fixed_wires)} fixed wires for {len(fixed_nets)} pour(s) of "
            f"{', '.join(sorted(fixed_classes)) or 'no class'}; {n_planes} outer plane(s) removed "
            f"(dropped nets: {', '.join(sorted(drop_nets)) or 'none'})"
        )

    # 4. class widths
    for name in sorted(widths):
        node = dsn.class_width_node(name)
        if node is None:
            raise ValueError(f"no class {name} with a width in the DSN")
        edits.append((node.start, node.end, str(um(float(widths[name])))))
        log.append(f"class {name}: width {node.text} -> {um(float(widths[name]))} um")

    # 5. the board-edge clearance and pads' own larger clearances, as keepouts
    if s.get("edge_and_pad_keepouts"):
        signal = [name for name, kind, _ in dsn.layers if kind == "signal"]
        extra = _edge_keepouts(board, dsn, signal) + _pad_keepouts(board, dsn, signal)
        via = next(iter(_children(dsn.structure, "via")), None) if dsn.structure else None
        if extra and via is None:
            raise ValueError("no (via ...) in the DSN's structure to insert the keepouts before")
        if extra:
            at = dsn.text.rfind("\n", 0, via.start)
            at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
            edits.append((at, at, dsn.nl + "".join(extra).rstrip("\r\n")))
            log.append(
                f"keepouts: {len(_edge_keepouts(board, dsn, signal))} edge strip(s) "
                f"(edge clearance {board.get('edge_clearance_mm') or 0:g} mm), "
                f"{len(_pad_keepouts(board, dsn, signal))} pad circle(s)"
            )

    # 6. plane layers opened to routing
    for name in open_layers:
        hit = [atom for lname, kind, atom in dsn.layers if lname == name and kind == "power"]
        if not hit:
            raise ValueError(f"no power layer {name} in the DSN")
        edits.append((hit[0].start, hit[0].end, "signal"))
        log.append(f"layer {name}: power -> signal")

    if fixed_wires:
        if dsn.wiring is not None:
            at = dsn.wiring.children[0].end
            edits.append((at, at, dsn.nl + "".join(fixed_wires)))
        else:
            close = dsn.text.rstrip().rfind(")")
            edits.append((close, close, f"  (wiring{dsn.nl}" + "".join(fixed_wires) + f"  ){dsn.nl}"))

    text = dsn_text
    for start, end, repl in sorted(edits, key=lambda e: (e[0], e[1]), reverse=True):
        text = text[:start] + repl + text[end:]
    return text, log


def wanted(settings):
    """True when `settings` asks for any DSN change."""
    return bool(
        settings.get("class_widths")
        or settings.get("fixed_pour_classes")
        or settings.get("drop_pour_nets")
        or settings.get("edge_and_pad_keepouts")
        or settings.get("route_on_plane_layers")
    )


_ENGINE_LIST = re.compile(r"^[^,=\s]+$")


def engine_plane_obstacle_args(classes):
    """K09.2 arm (a): Q01's in-engine pour policy instead of fixed wires."""
    names = [c for c in classes if _ENGINE_LIST.match(c)]
    return ["--router.plane_obstacle_net_classes=" + ",".join(names)] if names else []
