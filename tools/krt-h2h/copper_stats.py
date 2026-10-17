"""Copper added by a router, from the .kicad_pcb text alone (X01.6).

    python3 -I copper_stats.py ROUTED.kicad_pcb INPUT.kicad_pcb [OUT.json]

An S-expression scan independent of both engines and of KiCad: the board's
top-level `(segment ...)`, `(arc ...)` and `(via ...)` items. Segment length is
the distance from start to end; an arc's is its radius times the angle swept
from start through mid to end. Reports per copper layer and in total, for the
routed board, the input, and the difference (routed minus input), plus the
items of the routed board that have no identical item in the input ("new") and
the input items the routed board lost ("lost"). The X01 metrics `vias` and
`length_mm` are the differences. Prints the JSON (and writes OUT.json).
Plain python3, no pcbnew; the boards are data, never executed.
"""

import json
import math
import sys
from collections import Counter

ITEM_KINDS = ("segment", "arc", "via")


def tokens(text):
    """S-expression tokens: '(', ')', quoted strings (unquoted) and atoms."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif c in "()":
            yield c
            i += 1
        elif c == '"':
            j = i + 1
            out = []
            while j < n and text[j] != '"':
                if text[j] == "\\" and j + 1 < n:
                    j += 1
                out.append(text[j])
                j += 1
            yield ("str", "".join(out))
            i = j + 1
        else:
            j = i
            while j < n and text[j] not in ' \t\r\n()"':
                j += 1
            yield text[i:j]
            i = j


def top_level_items(text):
    """The board's depth-1 children whose head is segment, arc or via, as nested lists."""
    stack = []
    items = []
    for tok in tokens(text):
        if tok == "(":
            stack.append([])
        elif tok == ")":
            done = stack.pop()
            if not stack:
                continue
            stack[-1].append(done)
            if len(stack) == 1 and done and done[0] in ITEM_KINDS:
                items.append(done)
        else:
            value = tok[1] if isinstance(tok, tuple) else tok
            if stack:
                stack[-1].append(value)
    return items


def field(node, key):
    for child in node[1:]:
        if isinstance(child, list) and child and child[0] == key:
            return child[1:]
    return None


def point(node, key):
    values = field(node, key)
    return (float(values[0]), float(values[1]))


def arc_length(start, mid, end):
    """Length of the circular arc from start through mid to end (straight if collinear)."""
    (ax, ay), (bx, by), (cx, cy) = start, mid, end
    d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    if abs(d) < 1e-12:
        return math.dist(start, end)
    a2, b2, c2 = ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy
    ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d
    uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d
    r = math.dist((ux, uy), start)
    angle = lambda p: math.atan2(p[1] - uy, p[0] - ux)
    t0, tm, t1 = angle(start), angle(mid), angle(end)
    sweep = (t1 - t0) % (2 * math.pi)
    if (tm - t0) % (2 * math.pi) > sweep:  # mid is on the other side: the long way round
        sweep = 2 * math.pi - sweep
    return r * sweep


def describe(item):
    """(kind, layer or layers, length mm, identity key) of one item."""
    kind = item[0]
    net = (field(item, "net") or [""])[0]
    width = (field(item, "width") or field(item, "size") or [""])[0]
    if kind == "via":
        layers = tuple(field(item, "layers") or ())
        at = point(item, "at")
        return kind, "/".join(layers), 0.0, (kind, at, width, layers, net)
    layer = (field(item, "layer") or ["?"])[0]
    start, end = point(item, "start"), point(item, "end")
    if kind == "arc":
        mid = point(item, "mid")
        length = arc_length(start, mid, end)
        key = (kind, start, mid, end, width, layer, net)
    else:
        length = math.dist(start, end)
        key = (kind, start, end, width, layer, net)
    return kind, layer, length, key


def stats(text):
    totals = {"segments": 0, "arcs": 0, "vias": 0, "length_mm": 0.0, "per_layer_mm": {}}
    keys = Counter()
    lengths = {}
    for item in top_level_items(text):
        kind, layer, length, key = describe(item)
        keys[key] += 1
        lengths[key] = (kind, layer, length)
        if kind == "via":
            totals["vias"] += 1
            continue
        totals["segments" if kind == "segment" else "arcs"] += 1
        totals["length_mm"] += length
        totals["per_layer_mm"][layer] = totals["per_layer_mm"].get(layer, 0.0) + length
    return totals, keys, lengths


def summarize_keys(keys, lengths):
    out = {"segments": 0, "arcs": 0, "vias": 0, "length_mm": 0.0}
    for key, count in keys.items():
        kind, _, length = lengths[key]
        out[{"segment": "segments", "arc": "arcs", "via": "vias"}[kind]] += count
        out["length_mm"] += count * length
    out["length_mm"] = round(out["length_mm"], 6)
    return out


def compare(routed_text, input_text):
    routed, rkeys, rlen = stats(routed_text)
    inp, ikeys, ilen = stats(input_text)
    layers = sorted(set(routed["per_layer_mm"]) | set(inp["per_layer_mm"]))
    added = {
        "vias": routed["vias"] - inp["vias"],
        "segments": routed["segments"] - inp["segments"],
        "arcs": routed["arcs"] - inp["arcs"],
        "length_mm": round(routed["length_mm"] - inp["length_mm"], 6),
        "per_layer_mm": {
            layer: round(routed["per_layer_mm"].get(layer, 0.0) - inp["per_layer_mm"].get(layer, 0.0), 6)
            for layer in layers
        },
    }
    for d in (routed, inp):
        d["length_mm"] = round(d["length_mm"], 6)
        d["per_layer_mm"] = {k: round(v, 6) for k, v in sorted(d["per_layer_mm"].items())}
    return {
        "routed": routed,
        "input": inp,
        "added": added,
        "new": summarize_keys(rkeys - ikeys, rlen),
        "lost": summarize_keys(ikeys - rkeys, ilen),
    }


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8") as f:
        routed = f.read()
    with open(argv[1], encoding="utf-8") as f:
        inp = f.read()
    result = compare(routed, inp)
    text = json.dumps(result, indent=2)
    if len(argv) == 3:
        with open(argv[2], "w", encoding="utf-8") as f:
            f.write(text)
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
