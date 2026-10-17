#!/usr/bin/env python3
"""Delivered trace widths per net from a Specctra session (Q02.1).

An independent check of `quality::neckdown::width_report`: it parses the
SES (and, for the requested widths, the DSN) with its own s-expression
reader and shares no code with the engine.

    tools/quality/ses_widths.py SESSION.ses [--dsn DESIGN.dsn] [--net NAME]...

Prints JSON: per net, `length_mm` and `by_width_mm` (width -> length, mm),
and with `--dsn` also `requested_mm` (layer -> class width) and `under_mm`
(the length of the wires narrower than the class width on their layer).

Units: SES coordinates and widths are in the session's `(resolution UNIT N)`
units (N per UNIT); DSN widths are in the design's `(unit ...)`.
The class width of a net is its class's `(rule (width W))`, a class
`(layer_rule LAYER (rule (width W)))` overriding it on LAYER; nets in no
class use the structure's `(rule (width W))`.
"""

import argparse
import json
import math
import sys

MM_PER_UNIT = {"mm": 1.0, "um": 0.001, "mil": 0.0254, "inch": 25.4, "cm": 10.0}


def tokenize(text):
    """Yields '(', ')' and atoms; a double-quoted string is one atom, and
    the argument of `(string_quote ")` is the bare quote character."""
    i, n = 0, len(text)
    last = None
    while i < n:
        c = text[i]
        if last == "string_quote" and not c.isspace():
            j = text.index(")", i)
            last = text[i:j].strip()
            yield last
            i = j
        elif c in "()":
            yield c
            i += 1
        elif c.isspace():
            i += 1
        elif c == '"':
            j = text.index('"', i + 1)
            last = text[i + 1 : j]
            yield last
            i = j + 1
        else:
            j = i
            while j < n and not text[j].isspace() and text[j] not in '()"':
                j += 1
            last = text[i:j]
            yield last
            i = j


def parse(text):
    """The first s-expression of `text` as nested lists of strings."""
    stack = [[]]
    for tok in tokenize(text):
        if tok == "(":
            stack.append([])
        elif tok == ")":
            done = stack.pop()
            stack[-1].append(done)
        else:
            stack[-1].append(tok)
    return stack[0][0]


def children(node, name):
    return [c for c in node if isinstance(c, list) and c and c[0] == name]


def child(node, name):
    found = children(node, name)
    return found[0] if found else None


def find(node, name):
    """The first descendant list named `name`, depth first."""
    if isinstance(node, list):
        if node and node[0] == name:
            return node
        for c in node:
            hit = find(c, name)
            if hit is not None:
                return hit
    return None


def rule_width(node):
    rule = child(node, "rule")
    width = child(rule, "width") if rule else None
    return float(width[1]) if width else None


def session_widths(ses_text):
    """{net: {width_mm: length_mm}} from the session's network_out."""
    ses = parse(ses_text)
    routes = child(ses, "routes")
    res = child(routes, "resolution")
    scale = MM_PER_UNIT[res[1].lower()] / float(res[2])
    nets = {}
    network = child(routes, "network_out")
    for net in children(network, "net") if network else []:
        name = net[1]
        per_width = nets.setdefault(name, {})
        for wire in children(net, "wire"):
            path = child(wire, "path")
            if path is None:
                continue
            layer, width = path[1], float(path[2]) * scale
            coords = [float(v) * scale for v in path[3:] if not isinstance(v, list)]
            points = list(zip(coords[0::2], coords[1::2]))
            length = sum(
                math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(points, points[1:])
            )
            key = (layer, round(width, 6))
            per_width[key] = per_width.get(key, 0.0) + length
    return nets


def design_requested(dsn_text):
    """{net: (default_mm, {layer: mm})} from the design's classes."""
    dsn = parse(dsn_text)
    unit = child(dsn, "unit")
    resolution = child(dsn, "resolution")
    unit_name = (unit[1] if unit else resolution[1]).lower()
    scale = MM_PER_UNIT[unit_name]
    structure = child(dsn, "structure")
    default = (rule_width(structure) or 0.0) * scale
    requested = {}
    network = child(dsn, "network")
    for net in children(network, "net"):
        requested[net[1]] = (default, {})
    for cls in children(network, "class"):
        names = [v for v in cls[2:] if not isinstance(v, list)]
        width = rule_width(cls)
        width = default if width is None else width * scale
        layers = {}
        for lr in children(cls, "layer_rule"):
            w = rule_width(lr)
            if w is not None:
                layers[lr[1]] = w * scale
        for name in names:
            requested[name] = (width, layers)
    return requested


def report(ses_text, dsn_text=None, only=None):
    nets = session_widths(ses_text)
    requested = design_requested(dsn_text) if dsn_text else {}
    out = {}
    for name in sorted(nets):
        if only and name not in only:
            continue
        by_width = {}
        for (_, width), length in nets[name].items():
            by_width[width] = by_width.get(width, 0.0) + length
        entry = {
            "length_mm": round(sum(nets[name].values()), 6),
            "by_width_mm": {f"{w:.4f}": round(l, 6) for w, l in sorted(by_width.items())},
        }
        if name in requested:
            default, layers = requested[name]
            under = sum(
                length
                for (layer, width), length in nets[name].items()
                if width < layers.get(layer, default) - 1e-9
            )
            entry["requested_mm"] = {"default": default, **layers}
            entry["under_mm"] = round(under, 6)
        out[name] = entry
    return {"nets": out}


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("session")
    ap.add_argument("--dsn")
    ap.add_argument("--net", action="append")
    args = ap.parse_args(argv)
    with open(args.session, encoding="utf-8") as f:
        ses_text = f.read()
    dsn_text = None
    if args.dsn:
        with open(args.dsn, encoding="utf-8") as f:
            dsn_text = f.read()
    json.dump(report(ses_text, dsn_text, set(args.net or [])), sys.stdout, indent=2)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
