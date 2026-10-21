"""Q22.20: room for a zone's thermal spokes, as keepouts that carry the pad's
net.

KiCad's zone fill joins a pad to a zone of its net with thermal relief through
spokes, and its DRC (`starved_thermal`) wants at least Board Setup's
`min_resolved_spokes` (default 2) to reach fill that connects to something
other than the pad. A track of another net across a spoke's way out takes that
fill away: on canary-pwr `ab786fcc` tracks along both sides of J1's 2.54 mm
header rows starved its GND pads on F.Cu and B.Cu.

`apply` writes, for every pad that a copper zone of its net connects by
thermal relief on a signal layer of the DSN, one rectangle per spoke KiCad
builds, as a keepout named `KEEPOUT_NAME` with `(clearance_class null)`. Each
runs from the pad's centre along the spoke's direction to the spoke's end
(`ZONE_FILLER::buildThermalSpokes`, KiCad 10: the pad's box grown by the
thermal gap and half the zone's minimum width) and on by the spoke's width and
the zone's clearance, so a patch of fill as wide as the spoke stays beyond it;
it is the spoke's width plus the zone's clearance on each side wide. The router
fits it to the pin under it
(`crates/freerouting/src/io/specctra/parser/thermal_keepouts.rs`): it carries
the pad's net, so only other nets keep out.

- The connection is the pad's zone connection, else its footprint's, else the
  zone's (`THT_THERMAL` is thermal for plated through-hole pads only); the gap
  and spoke width are the pad's overrides, else the zone's; the spoke angle is
  the pad's (`GetThermalSpokeAngleDegrees`). Custom DRC rules are not read.
- Spokes are as KiCad builds them: a circle's at 0, 90, 180, 270 degrees turned
  by the spoke angle; another shape's at the spoke angle plus multiples of 90
  degrees to its unrotated box; then turned by the pad's orientation. A spoke
  narrower than the zone's minimum width is none, and a custom pad gets none.
- A keepout is written only where the zone's outline holds the pad's centre
  and the keepout's far end, and that end is in no copper zone of another net
  with a higher priority and in no rule area that keeps zone fill out. One zone
  per pad and layer: the first (board zones, then the footprints').
- Nothing when `min_resolved_spokes` is 0 (KiCad does not check).

The `.kicad_pcb` reader does the same (`crates/freerouting-kicad/src/thermal_spokes.rs`).
Pure Python: the facts come from `kicad_board.thermal_facts`, in KiCad's frame
(mm, Y down).
"""

import math

try:  # inside the plugin package
    from .dsn_fixups import Dsn, _children, um
except ImportError:  # tests and headless scripts put this folder on sys.path
    from dsn_fixups import Dsn, _children, um

KEEPOUT_NAME = "thermal_spoke"
CLEARANCE_CLASS = "null"  # the router's class 0: the rectangle holds the clearance

# KiCad's ZONE_CONNECTION
CONNECT_THERMAL = 1
CONNECT_THT_THERMAL = 3


def _rotate(v, deg):
    """KiCad's RotatePoint: `v` turned by `deg` (counter-clockwise on screen, Y down)."""
    r = math.radians(deg)
    s, c = math.sin(r), math.cos(r)
    return (v[0] * c + v[1] * s, -v[0] * s + v[1] * c)


def _inside_ring(ring, p):
    """Even-odd crossing test (the reader's `inside_ring`)."""
    inside = False
    n = len(ring)
    for i in range(n):
        a, b = ring[i], ring[(i + 1) % n]
        if (a[1] > p[1]) != (b[1] > p[1]):
            x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
            if p[0] < x:
                inside = not inside
    return inside


def _holds(zone, p):
    return _inside_ring(zone["outline_mm"], p) and not any(_inside_ring(h, p) for h in zone.get("holes_mm") or [])


def _is_thermal(pad, zone):
    connect = pad.get("zone_connect")
    connect = zone["connect"] if connect is None else connect
    return connect == CONNECT_THERMAL or (connect == CONNECT_THT_THERMAL and pad["pth"])


def _spoke_ends(pad, grow):
    """The far ends of the spokes KiCad builds for `pad`, from its centre, each
    with its unit direction."""
    w, h = pad["size_mm"]
    if pad["circle"]:
        r = w / 2 + grow
        ends = [_rotate(e, pad["angle"]) for e in ((r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r))]
    else:
        hx, hy = w / 2 + grow, h / 2 + grow
        ends = []
        for k in (0.0, 90.0, 180.0, 270.0):
            r = math.radians(pad["angle"] + k)
            d = (math.cos(r), math.sin(r))
            tx = math.inf if abs(d[0]) < 1e-12 else hx / abs(d[0])
            ty = math.inf if abs(d[1]) < 1e-12 else hy / abs(d[1])
            t = min(tx, ty)
            ends.append((d[0] * t, d[1] * t))
    out = []
    for e in ends:
        e = _rotate(e, pad["orient"])
        length = math.hypot(e[0], e[1])
        out.append((e, (e[0] / length, e[1] / length)))
    return out


def _corridor(c, d, length, half):
    n = (-d[1] * half, d[0] * half)
    far = (c[0] + d[0] * length, c[1] + d[1] * length)
    return [
        (c[0] + n[0], c[1] + n[1]),
        (far[0] + n[0], far[1] + n[1]),
        (far[0] - n[0], far[1] - n[1]),
        (c[0] - n[0], c[1] - n[1]),
    ]


def corridors(facts, signal_layers):
    """`[(layer, polygon mm)]`: the keepouts of `facts`, zone by zone, its
    layers in order, then pad by pad and spoke by spoke (module docs)."""
    out = []
    if (facts.get("min_resolved_spokes") or 0) <= 0:
        return out
    pads, zones = facts.get("pads") or [], facts.get("zones") or []
    done = set()
    for zi, zone in enumerate(zones):
        if zone["rule_area"] or not zone["net"]:
            continue
        for layer in zone["layers"]:
            if layer not in signal_layers:
                continue
            for pi, pad in enumerate(pads):
                if (
                    pad["net"] != zone["net"]
                    or pad["custom"]
                    or layer not in pad["layers"]
                    or not _is_thermal(pad, zone)
                    or not _holds(zone, pad["centre_mm"])
                    or (pi, layer) in done
                ):
                    continue
                done.add((pi, layer))
                gap = zone["gap_mm"] if pad.get("gap_mm") is None else pad["gap_mm"]
                spoke = zone["spoke_mm"] if pad.get("spoke_mm") is None else pad["spoke_mm"]
                spoke = min(spoke, pad["size_mm"][0], pad["size_mm"][1])
                if spoke < zone["min_thickness_mm"]:
                    continue
                grow = gap + zone["min_thickness_mm"] / 2
                half = spoke / 2 + zone["clearance_mm"]
                c = pad["centre_mm"]
                for end, d in _spoke_ends(pad, grow):
                    length = math.hypot(end[0], end[1]) + spoke + zone["clearance_mm"]
                    far = (c[0] + d[0] * length, c[1] + d[1] * length)
                    blocked = any(
                        oi != zi
                        and layer in o["layers"]
                        and (
                            (o["rule_area"] and o.get("no_fill"))
                            or (not o["rule_area"] and o["net"] != zone["net"] and o["priority"] > zone["priority"])
                        )
                        and _holds(o, far)
                        for oi, o in enumerate(zones)
                    )
                    if _holds(zone, far) and not blocked:
                        out.append((layer, _corridor(c, d, length, half)))
    return out


def apply(dsn_text, facts):
    """The DSN with the keepouts of `facts` (`kicad_board.thermal_facts`) before
    the structure's `(via ...)`, and the log lines; unchanged with none."""
    dsn = Dsn(dsn_text)
    if dsn.structure is None:
        raise ValueError("no (structure ...) in the DSN")
    signal = {name for name, kind, _atom in dsn.layers if kind not in ("power", "jumper")}
    found = corridors(facts, signal)
    if not found:
        return dsn_text, []
    nl = dsn.nl

    def line(layer, poly):
        coords = " ".join(f"{um(x)} {-um(y)}" for x, y in poly + poly[:1])
        return f'    (keepout "{KEEPOUT_NAME}" (polygon {dsn.layer_token(layer)} 0 {coords}) (clearance_class {CLEARANCE_CLASS})){nl}'

    lines = [line(layer, poly) for layer, poly in found]
    via = next(iter(_children(dsn.structure, "via")), None)
    anchor = via.start if via is not None else dsn.structure.end - 1
    at = dsn.text.rfind("\n", 0, anchor)
    at = at - 1 if at > 0 and dsn.text[at - 1] == "\r" else at
    text = dsn_text[:at] + nl + "".join(lines).rstrip("\r\n") + dsn_text[at:]
    return text, [f"thermal spokes: {len(lines)} spoke way(s) kept clear of other nets"]
