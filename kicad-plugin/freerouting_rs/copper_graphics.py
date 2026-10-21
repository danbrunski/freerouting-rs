"""Q22.4: copper graphics and copper text, which KiCad's DSN export leaves out,
as obstacles (decided 2026-10-20: routing never breaks the board's
manufacturing rules, graded under the board's own rules).

KiCad's DRC keeps other copper its clearance from every shape and every
visible text on a copper layer: footprint lines, rectangles, polygons, arcs,
circles and texts, and the board's own. Its DSN export writes none of them
(a footprint's copper lines only as image outline, which is drawing only), so
the router laid 127 tracks of pcietest over its connectors' B.Cu `fp_line`s
and one of sonde-xilinx over a `gr_text "GND"` on F.Cu.

`apply` writes them into the DSN:

- A shape with no net, or of a net the DSN does not list, becomes keepouts
  named `KEEPOUT_NAME` on its layer: the filled body of a filled polygon,
  rectangle or circle and the rectangles within half the stroke of each piece
  (`edge_graphics.shape_polygons`, curves cut into chords).
- A shape of a net-tie footprint (its ties stay ties) becomes the same
  pieces named `NET_TIE_KEEPOUT_NAME` (Q22.19): the router lets in the nets
  of the pins under the shape's pieces (the tie's pads it touches) and keeps
  every other net its clearance out (the engine's
  `io/specctra/parser/pad_keepouts.rs`), as KiCad lets copper of the tie's
  nets touch the shape. Q22.4 wrote them as `KEEPOUT_NAME`, which kept the
  tie's own nets out too.
- A visible text becomes keepouts named `TEXT_KEEPOUT_NAME`: the outer
  outlines of KiCad's own glyph strokes (`TransformTextToPolySet`, collected
  by `kicad_board.copper_graphics`; a glyph's counter is kept out too), or the
  box KiCad gives a knockout text or a text box.
- A shape of a net the DSN lists is copper of that net (decided 2026-10-20,
  option (a)): each polygon of `edge_graphics.shape_polygons` (a filled body,
  and the rectangle of each piece of the stroke) becomes a conduction-area
  wire of the net, `(wire (polygon ...) (net N) (type fix) (attr obstacle))`,
  at the end of the wiring. The router joins it to its net (the pieces of one
  shape are in contact with each other) and keeps every
  other net its clearance from it, whatever `router.plane_as_obstacle` says;
  the session leaves it out (it is fixed), so KiCad keeps its own shape. An
  earlier model, fixed wires covering each shape in 0.05 mm strips, gave
  vfo-controller 665 wires under the 0.2 mm minimum width and cut its route at
  120 s; it was dropped.

The router keeps a keepout its default clearance from a net (the larger of
the net's class clearance and the Default class's, as KiCad does with a shape
of no net), so the shapes go in at their copper size. Keepouts go before the
structure's `(via ...)`, like `edge_graphics`' keepouts.

The `.kicad_pcb` reader (`crates/freerouting-kicad`, `copper_graphics.rs`)
does the same, with the box of each text instead of its strokes. Pure Python:
the board facts come from `kicad_board.copper_graphics`, in KiCad's frame
(mm, Y down).
"""

try:  # inside the plugin package
    from . import edge_graphics
    from .dsn_fixups import Dsn, _children, um
except ImportError:  # tests and headless scripts put this folder on sys.path
    import edge_graphics
    from dsn_fixups import Dsn, _children, um

KEEPOUT_NAME = "copper_graphic"
TEXT_KEEPOUT_NAME = "copper_text"
NET_TIE_KEEPOUT_NAME = "net_tie"  # the router's `NET_TIE_KEEPOUT_NAME` (Q22.19)
OBSTACLE_ATTR = "obstacle"  # the router's `ast::WIRE_ATTR_OBSTACLE`


def _keepout(name, layer, poly, nl):
    coords = " ".join(f"{um(x)} {-um(y)}" for x, y in list(poly) + [poly[0]])
    return f'    (keepout "{name}" (polygon {layer} 0 {coords})){nl}'


def _area_wire(layer, net, poly, nl):
    coords = " ".join(f"{um(x)} {-um(y)}" for x, y in list(poly) + [poly[0]])
    return f"    (wire (polygon {layer} 0 {coords}) (net {net}) (type fix) (attr {OBSTACLE_ATTR})){nl}"


def _insert_wires(dsn, text, wires):
    """`text` (the DSN `dsn` was read from, edited only after its wiring) with
    `wires` at the end of the wiring, or in a new wiring scope."""
    nl = dsn.nl
    if dsn.wiring is not None:
        close = dsn.wiring.end - 1
        at = dsn.text.rfind("\n", 0, close)
        at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
        if at < dsn.wiring.start:  # `(wiring)` on one line
            return text[:close] + nl + "".join(wires) + text[close:]
        return text[:at] + nl + "".join(wires).rstrip("\r\n") + text[at:]
    close = dsn.text.rstrip().rfind(")")
    return text[:close] + f"  (wiring{nl}" + "".join(wires) + f"  ){nl}" + text[close:]


def apply(dsn_text, facts):
    """The DSN with the obstacles of `facts` (`kicad_board.copper_graphics`),
    and the log lines; unchanged with nothing on copper."""
    shapes = facts.get("shapes") or []
    texts = facts.get("texts") or []
    if not shapes and not texts:
        return dsn_text, []
    dsn = Dsn(dsn_text)
    if dsn.structure is None:
        raise ValueError("no (structure ...) in the DSN")
    layers = {name for name, _kind, _atom in dsn.layers}
    nl = dsn.nl
    keepouts = []
    net_wires = []
    n_keepout_shapes = n_net_shapes = n_texts = 0
    for shape in shapes:
        layer = shape["layer"]
        if layer not in layers:
            continue
        net = shape.get("net") or ""
        body, strokes = edge_graphics.shape_polygons(shape)
        if net and net in dsn.net_class:
            token = dsn.net_token(net)
            polys = ([body] if body else []) + strokes
            net_wires += [_area_wire(dsn.layer_token(layer), token, poly, nl) for poly in polys]
            n_net_shapes += 1
            continue
        name = NET_TIE_KEEPOUT_NAME if shape.get("net_tie") else KEEPOUT_NAME
        keepouts += [_keepout(name, dsn.layer_token(layer), poly, nl) for poly in ([body] if body else []) + strokes]
        n_keepout_shapes += 1
    for text in texts:
        if text["layer"] not in layers:
            continue
        polys = [p for p in text.get("polygons") or [] if len(p) >= 3]
        keepouts += [_keepout(TEXT_KEEPOUT_NAME, dsn.layer_token(text["layer"]), poly, nl) for poly in polys]
        n_texts += 1 if polys else 0
    log = []
    out = dsn_text
    if net_wires:  # after the structure in the file: insert it first
        out = _insert_wires(dsn, out, net_wires)
        log.append(
            f"copper graphics: {n_net_shapes} shape(s) of a net as fixed copper of their net "
            f"({len(net_wires)} areas)"
        )
    if keepouts:
        via = next(iter(_children(dsn.structure, "via")), None)
        anchor = via.start if via is not None else dsn.structure.end - 1
        at = dsn.text.rfind("\n", 0, anchor)
        at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
        out = out[:at] + nl + "".join(keepouts).rstrip("\r\n") + out[at:]
        log.insert(
            0,
            f"copper graphics: {n_keepout_shapes} shape(s) and {n_texts} text(s) on copper kept clear "
            f"({len(keepouts)} keepouts)",
        )
    ties = facts.get("net_tie_shapes") or 0
    if ties and keepouts:
        log.append(f"copper graphics: {ties} net-tie shape(s) kept clear of other nets, their pads' nets let in")
    return out, log
