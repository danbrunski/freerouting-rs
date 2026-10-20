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

- A shape with no net, or of a net-tie footprint (its ties stay ties), or of a
  net the DSN does not list, becomes keepouts named `KEEPOUT_NAME` on its
  layer: the filled body of a filled polygon, rectangle or circle and the
  rectangles within half the stroke of each piece
  (`edge_graphics.shape_polygons`, curves cut into chords).
- A visible text becomes keepouts named `TEXT_KEEPOUT_NAME`: the outer
  outlines of KiCad's own glyph strokes (`TransformTextToPolySet`, collected
  by `kicad_board.copper_graphics`; a glyph's counter is kept out too), or the
  box KiCad gives a knockout text or a text box.
- A shape of a net the DSN lists is left out, and counted in the log
  ("N shape(s) of a net left out (Q22.4 remainder)"). Decided 2026-10-20:
  fixed wires of that net covering it (boundary wires plus 0.05 mm strips)
  cut vfo-controller's route at 120 s with 7 unrouted, against 1.4 s and 0
  without them. The remainder is one conduction polygon of the net plus a
  keepout of its outline that holds only for other nets.

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


def _keepout(name, layer, poly, nl):
    coords = " ".join(f"{um(x)} {-um(y)}" for x, y in list(poly) + [poly[0]])
    return f'    (keepout "{name}" (polygon {layer} 0 {coords})){nl}'


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
    n_keepout_shapes = n_net_shapes = n_texts = 0
    for shape in shapes:
        layer = shape["layer"]
        if layer not in layers:
            continue
        net = shape.get("net") or ""
        if net and net in dsn.net_class:
            n_net_shapes += 1  # left out (module docs)
            continue
        body, strokes = edge_graphics.shape_polygons(shape)
        keepouts += [_keepout(KEEPOUT_NAME, dsn.layer_token(layer), poly, nl) for poly in ([body] if body else []) + strokes]
        n_keepout_shapes += 1
    for text in texts:
        if text["layer"] not in layers:
            continue
        polys = [p for p in text.get("polygons") or [] if len(p) >= 3]
        keepouts += [_keepout(TEXT_KEEPOUT_NAME, dsn.layer_token(text["layer"]), poly, nl) for poly in polys]
        n_texts += 1 if polys else 0
    log = []
    out = dsn_text
    if keepouts:
        via = next(iter(_children(dsn.structure, "via")), None)
        anchor = via.start if via is not None else dsn.structure.end - 1
        at = dsn.text.rfind("\n", 0, anchor)
        at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
        out = out[:at] + nl + "".join(keepouts).rstrip("\r\n") + out[at:]
        log.append(
            f"copper graphics: {n_keepout_shapes} shape(s) and {n_texts} text(s) on copper kept clear "
            f"({len(keepouts)} keepouts)"
        )
    if n_net_shapes:
        log.append(f"copper graphics: {n_net_shapes} shape(s) of a net left out (Q22.4 remainder)")
    ties = facts.get("net_tie_shapes") or 0
    if ties and keepouts:
        log.append(f"copper graphics: {ties} net-tie shape(s) kept clear as keepouts, their nets left out")
    return out, log
