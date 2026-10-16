"""Session edits for the oracle tests (plain python3).

    inject.py pick PADS.json DSN SES in-pad|graze      prints NET PADSTACK X_UM Y_UM [EX EY LAYER WIDTH]
    inject.py via SRC.ses DST.ses NET PADSTACK X_UM Y_UM [EX EY LAYER WIDTH]
    inject.py empty DSN DST.ses                        a session without wiring

`pick in-pad` chooses an SMD pad and the nearest wire end of another net:
the via goes on the pad's center, joined to that wire end by a wire of the
wire's net (printed as EX EY LAYER WIDTH, session units). KiCad's
ImportSpecctraSES gives a via or wire that touches only a pad the pad's net,
whatever the session says, so a lone via in a pad of another net would be
invisible to its DRC. `pick graze` chooses the rightmost pad and a net of the
session other than the pad's, and puts the via's copper 0.05 mm inside the
DSN's default clearance to the pad's right edge. Coordinates are DSN um
(Y negated, as KiCad exports) unless noted.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import oracle_lib as lib  # noqa: E402

GRAZE_UM = 50.0


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def session_nets(ses_text):
    return re.findall(r'\(net\s+("[^"]*"|\S+)', ses_text)


def first_via(dsn_text):
    structure = dsn_text.find("(structure")
    for keyword, start, end in lib.child_scopes(dsn_text, structure):
        if keyword == "via":
            names = re.findall(r'"[^"]*"|[^\s()]+', dsn_text[start + 1:end - 1])[1:]
            return names[0].strip('"')
    raise SystemExit("no via padstack in the DSN structure")


def via_diameter_um(padstack):
    m = re.search(r"_(\d+):\d+_um", padstack)
    if not m:
        raise SystemExit("can't read the via diameter from %s" % padstack)
    return float(m.group(1))


def default_clearance_um(dsn_text):
    structure = dsn_text.find("(structure")
    body = dsn_text[structure:lib.scope_end(dsn_text, structure)]
    m = re.search(r"\(clearance\s+([\d.]+)\)", body)
    return float(m.group(1)) * lib.dsn_unit_scale_mm(dsn_text) * 1000.0


def pick(pads_json, dsn, ses, mode):
    with open(pads_json, encoding="utf-8") as f:
        pads = [p for p in json.load(f) if p["net"]]
    dsn_text, ses_text = read(dsn), read(ses)
    padstack = first_via(dsn_text)
    nets = [n.strip('"') for n in session_nets(ses_text)]
    if mode == "in-pad":
        pad = next(p for p in pads if p.get("smd", True))
        x, y = pad["at"]
        scale = ses_units_per_um(ses_text)
        ends = [e for e in wire_ends(ses_text) if e[0] != pad["net"]]
        net, ex, ey, layer, width = min(
            ends, key=lambda e: (e[1] / scale - x) ** 2 + (e[2] / scale - y) ** 2
        )
        print('"%s" "%s" %.1f %.1f %d %d %s %s' % (net, padstack, x, y, ex, ey, layer, width))
        print("pad %s-%s net %s at %s" % (pad["footprint"], pad["number"], pad["net"], pad["at"]), file=sys.stderr)
        return
    else:
        pad = max(pads, key=lambda p: p["right"])
        x = pad["right"] + via_diameter_um(padstack) / 2 + default_clearance_um(dsn_text) - GRAZE_UM
        y = pad["at"][1]
    net = next(n for n in nets if n != pad["net"])
    print('"%s" "%s" %.1f %.1f' % (net, padstack, x, y))
    print("pad %s-%s net %s at %s" % (pad["footprint"], pad["number"], pad["net"], pad["at"]), file=sys.stderr)


def wire_ends(ses_text):
    """(net, x, y, layer, width) of both ends of every path, in session units."""
    out = []
    routes = ses_text.find("(network_out")
    for keyword, start, end in lib.child_scopes(ses_text, routes):
        if keyword != "net":
            continue
        body = ses_text[start:end]
        net = re.match(r'\(net\s+("[^"]*"|\S+)', body).group(1).strip('"')
        for m in re.finditer(r'\(path\s+("[^"]*"|\S+)\s+([\d.]+)((?:\s+-?[\d.]+)+)\s*\)', body):
            values = [float(v) for v in m.group(3).split()]
            for x, y in (values[:2], values[-2:]):
                out.append((net, x, y, m.group(1), m.group(2)))
    return out


def ses_units_per_um(ses_text):
    m = re.search(r"\(routes\s*\(resolution\s+(\w+)\s+(\d+)\)", ses_text)
    if not m:
        return 1.0
    unit_um = {"um": 1.0, "mm": 1000.0, "mil": 25.4, "inch": 25400.0}[m.group(1)]
    return float(m.group(2)) / unit_um


def add_via(src, dst, net, padstack, x_um, y_um, ex=None, ey=None, layer=None, width=None):
    text = read(src)
    scale = ses_units_per_um(text)
    vx, vy = round(float(x_um) * scale), round(float(y_um) * scale)
    via = '\n        (via "%s" %d %d\n        )' % (padstack, vx, vy)
    if ex is not None:
        via += "\n        (wire\n          (path %s %s\n            %s %s\n            %d %d\n          )\n        )" % (
            layer, width, ex, ey, vx, vy)
    quoted = '"%s"' % net
    m = re.search(r"\(net\s+(%s|%s)\s" % (re.escape(quoted), re.escape(net)), text)
    if m:
        text = text[:m.end() - 1] + via + text[m.end() - 1:]
    else:
        i = text.find("(network_out")
        text = text[:i + len("(network_out")] + "\n      (net %s%s\n      )" % (quoted, via) + text[i + len("(network_out"):]
    with open(dst, "w", encoding="utf-8") as f:
        f.write(text)


def empty(dsn, dst):
    name = re.match(r'\s*\(pcb\s+("[^"]*"|\S+)', read(dsn)).group(1)
    with open(dst, "w", encoding="utf-8") as f:
        f.write("(session %s\n  (base_design %s)\n  (routes\n    (resolution um 10)\n"
                "    (parser (host_cad \"oracle test\"))\n    (library_out)\n    (network_out)\n  )\n)\n" % (name, name))


def main(argv):
    if argv[:1] == ["pick"] and len(argv) == 5:
        pick(*argv[1:])
    elif argv[:1] == ["via"] and len(argv) in (7, 11):
        add_via(*argv[1:])
    elif argv[:1] == ["empty"] and len(argv) == 3:
        empty(*argv[1:])
    else:
        print(__doc__, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
