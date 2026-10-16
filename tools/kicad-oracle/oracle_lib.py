"""Pure-Python steps of the KiCad DRC oracle (no pcbnew, no wx).

Used by grade.sh and tested by tests/test_oracle_lib.py with plain python3.
Started from the 2026-10-15 head-to-head scripts (strip_placement.py,
classwidths.py, summarize.py; see docs/roadmap/tasks/X01-krt-head-to-head.md).
"""

import collections
import json
import re
import sys

# DRC classes graded as errors whatever the board's author set, so that a
# board with `clearance` at `warning` still counts a graze.
FORCED_ERROR_CLASSES = (
    "clearance",
    "shorting_items",
    "tracks_crossing",
    "hole_clearance",
    "hole_to_hole",
    "holes_co_located",
    "copper_edge_clearance",
)

# KiCad 10's DRC reported at most this many items of one type in the
# 2026-10-15 head-to-head (track_width); a count equal to it means "at least".
DRC_CAP = 199

# Violations of one type, net pair and position within this distance are the
# same item in the input and in the routed board.
MATCH_DISTANCE_MM = 0.5

# DSN keepout outlines and rule-area outlines match within this many um.
OUTLINE_TOLERANCE_UM = 1.0

EXIT_CLEAN = 0
EXIT_ROUTER_ITEMS = 1
EXIT_NOT_GRADED = 2


def scope_end(text, start):
    """The index just past the scope that opens at text[start] == '('."""
    depth = 0
    quoted = False
    i = start
    while i < len(text):
        c = text[i]
        if quoted:
            if c == '"':
                quoted = False
        elif c == '"':
            quoted = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced parentheses from offset %d" % start)


def child_scopes(text, start):
    """(keyword, start, end) of the scopes directly inside the scope at start."""
    end = scope_end(text, start)
    result = []
    i = start + 1
    quoted = False
    while i < end - 1:
        c = text[i]
        if quoted:
            if c == '"':
                quoted = False
            i += 1
        elif c == '"':
            quoted = True
            i += 1
        elif c == "(":
            child_end = scope_end(text, i)
            m = re.match(r"\(\s*([^\s()]+)", text[i:child_end])
            result.append((m.group(1) if m else "", i, child_end))
            i = child_end
        else:
            i += 1
    return result


def strip_placement(text):
    """Removes the (placement ...) scope of a SES, as KRT's
    baseline_freerouting does before KiCad imports it."""
    i = text.find("(placement")
    if i < 0:
        return text
    return text[:i] + text[scope_end(text, i):]


def dsn_unit_scale_mm(text):
    unit = re.search(r"\(unit (\w+)\)", text)
    unit = unit.group(1) if unit else "um"
    return {"um": 1e-3, "mm": 1.0, "mil": 0.0254, "inch": 25.4, "cm": 10.0}[unit]


def class_widths(dsn_text):
    """Net name -> track width in mm from the (class ...) scopes of a DSN
    that KiCad exported (their (rule (width ...)), in the DSN's unit)."""
    scale = dsn_unit_scale_mm(dsn_text)
    out = {}
    for m in re.finditer(r"\(class ", dsn_text):
        body = dsn_text[m.start() + 7:scope_end(dsn_text, m.start()) - 1]
        head = re.split(r"\(", body, maxsplit=1)[0]
        tokens = re.findall(r'"[^"]*"|\S+', head)
        width = re.search(r"\(rule\s*\(width ([\d.]+)\)", body)
        if not width or len(tokens) < 2:
            continue
        for net in tokens[1:]:
            out[net.strip('"')] = round(float(width.group(1)) * scale, 4)
    return out


def _numbers(text):
    return [float(t) for t in re.findall(r"-?\d+(?:\.\d+)?", text)]


def _outline_key(points):
    """Rounded points without a repeated closing point, as a set."""
    pts = [(round(x / OUTLINE_TOLERANCE_UM), round(y / OUTLINE_TOLERANCE_UM)) for x, y in points]
    if len(pts) > 1 and pts[0] == pts[-1]:
        pts = pts[:-1]
    return frozenset(pts)


def remove_keepouts_of_open_rule_areas(dsn_text, areas):
    """Removes the structure-level (keepout ...) scopes KiCad exported for
    rule areas that allow both tracks and vias (they ban only footprints,
    pads or pours). `areas` are dicts with `layers` (DSN layer names),
    `outline` ([x, y] in DSN um, Y already negated), `tracks` and `vias`
    (True when banned). Returns the new text and the number removed."""
    open_areas = [a for a in areas if not a["tracks"] and not a["vias"]]
    if not open_areas:
        return dsn_text, 0
    structure = dsn_text.find("(structure")
    if structure < 0:
        return dsn_text, 0
    wanted = collections.Counter()
    for area in open_areas:
        for layer in area["layers"]:
            wanted[(layer, _outline_key(area["outline"]))] += 1
    removals = []
    for keyword, start, end in child_scopes(dsn_text, structure):
        if keyword != "keepout":
            continue
        scope = dsn_text[start:end]
        m = re.search(r"\(polygon\s+(\S+)\s+([\d.]+)((?:\s+-?[\d.]+)+)\s*\)", scope)
        if not m:
            continue
        values = _numbers(m.group(3))
        points = list(zip(values[0::2], values[1::2]))
        key = (m.group(1), _outline_key(points))
        if wanted[key] > 0:
            wanted[key] -= 1
            removals.append((start, end))
    for start, end in reversed(removals):
        line_start = dsn_text.rfind("\n", 0, start) + 1
        line_end = dsn_text.find("\n", end)
        line_end = len(dsn_text) if line_end < 0 else line_end + 1
        whole_lines = dsn_text[line_start:start].strip() == "" and dsn_text[end:line_end].strip() == ""
        if whole_lines:
            dsn_text = dsn_text[:line_start] + dsn_text[line_end:]
        else:
            dsn_text = dsn_text[:start] + dsn_text[end:]
    return dsn_text, len(removals)


def force_copper_errors(pro_text):
    """The .kicad_pro text with FORCED_ERROR_CLASSES at severity error."""
    project = json.loads(pro_text) if pro_text.strip() else {}
    severities = (
        project.setdefault("board", {})
        .setdefault("design_settings", {})
        .setdefault("rule_severities", {})
    )
    for name in FORCED_ERROR_CLASSES:
        severities[name] = "error"
    return json.dumps(project, indent=2) + "\n"


def item_nets(violation):
    """The sorted net names in a DRC violation's item descriptions."""
    nets = set()
    for item in violation.get("items", []):
        nets.update(re.findall(r"\[([^\]]*)\]", item.get("description", "")))
    return tuple(sorted(nets))


def _position(violation):
    items = violation.get("items", [])
    if not items or "pos" not in items[0]:
        return None
    return (items[0]["pos"].get("x", 0.0), items[0]["pos"].get("y", 0.0))


def _close(a, b):
    if a is None or b is None:
        return a is None and b is None
    return ((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5 <= MATCH_DISTANCE_MM


def subtract_preexisting(routed, unrouted):
    """Splits the routed board's violations into (router-attributable,
    pre-existing): a routed item is pre-existing when an unused item of the
    input has the same type and net pair and lies within MATCH_DISTANCE_MM."""
    pool = collections.defaultdict(list)
    for v in unrouted:
        pool[(v.get("type"), item_nets(v))].append(_position(v))
    real, pre = [], []
    for v in routed:
        candidates = pool[(v.get("type"), item_nets(v))]
        here = _position(v)
        hit = next((i for i, p in enumerate(candidates) if _close(p, here)), None)
        if hit is None:
            real.append(v)
        else:
            candidates.pop(hit)
            pre.append(v)
    return real, pre


def by_type(violations):
    return dict(sorted(collections.Counter(v.get("type", "?") for v in violations).items()))


def capped_types(counts):
    """Types whose DRC count equals the cap: report them as at least DRC_CAP."""
    return sorted(t for t, n in counts.items() if n == DRC_CAP)


def summarize(routed_drc, unrouted_drc, copper, meta):
    """The summary dict of one grading and its exit code."""
    real, pre = subtract_preexisting(
        routed_drc.get("violations", []), unrouted_drc.get("violations", [])
    )
    routed_counts = by_type(routed_drc.get("violations", []))
    below_minimum = copper.get("below_minimum", {}) if copper else {}
    below_total = sum(below_minimum.values())
    exit_code = EXIT_CLEAN if not real and below_total == 0 else EXIT_ROUTER_ITEMS
    summary = dict(meta)
    summary.update(
        {
            "graded": True,
            "drc_real": len(real),
            "drc_real_by_type": by_type(real),
            "preexisting": len(pre),
            "unconnected_items": len(routed_drc.get("unconnected_items", [])),
            "input_unconnected_items": len(unrouted_drc.get("unconnected_items", [])),
            "routed_by_type": routed_counts,
            "input_by_type": by_type(unrouted_drc.get("violations", [])),
            "capped_types": capped_types(routed_counts),
            "copper": copper,
            "exit_code": exit_code,
        }
    )
    return summary, exit_code


def not_graded(reason, meta=None):
    summary = dict(meta or {})
    summary.update({"graded": False, "reason": reason, "exit_code": EXIT_NOT_GRADED})
    return summary


def _read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def _write(path, text):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def _load_json(path, default=None):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


USAGE = """usage: python3 oracle_lib.py COMMAND ...
  strip-placement IN.ses OUT.ses
  fix-keepouts IN.dsn AREAS.json OUT.dsn          prints the number removed
  class-widths IN.dsn OUT.json
  force-errors PROJECT.kicad_pro                  rewrites it in place
  summarize ROUTED_DRC.json INPUT_DRC.json COPPER.json META.json OUT.json
  not-graded REASON OUT.json [META.json]"""


def main(argv):
    if not argv:
        print(USAGE, file=sys.stderr)
        return EXIT_NOT_GRADED
    command, args = argv[0], argv[1:]
    if command == "strip-placement" and len(args) == 2:
        _write(args[1], strip_placement(_read(args[0])))
    elif command == "fix-keepouts" and len(args) == 3:
        areas = _load_json(args[1], {}).get("rule_areas", [])
        text, removed = remove_keepouts_of_open_rule_areas(_read(args[0]), areas)
        _write(args[2], text)
        print(removed)
    elif command == "class-widths" and len(args) == 2:
        with open(args[1], "w", encoding="utf-8") as f:
            json.dump(class_widths(_read(args[0])), f)
    elif command == "force-errors" and len(args) == 1:
        _write(args[0], force_copper_errors(_read(args[0])))
    elif command == "summarize" and len(args) == 5:
        routed = _load_json(args[0])
        unrouted = _load_json(args[1])
        meta = _load_json(args[3], {})
        if routed is None or unrouted is None:
            summary, code = not_graded("a DRC report is missing or unreadable", meta), EXIT_NOT_GRADED
        else:
            summary, code = summarize(routed, unrouted, _load_json(args[2], {}), meta)
        with open(args[4], "w", encoding="utf-8") as f:
            json.dump(summary, f, indent=2)
        if summary["graded"]:
            print(
                "graded: drc_real=%d preexisting=%d unconnected_items=%d (input %d) %s"
                % (
                    summary["drc_real"],
                    summary["preexisting"],
                    summary["unconnected_items"],
                    summary["input_unconnected_items"],
                    json.dumps(summary["drc_real_by_type"]),
                )
            )
        return code
    elif command == "not-graded" and len(args) in (2, 3):
        meta = _load_json(args[2], {}) if len(args) == 3 else {}
        with open(args[1], "w", encoding="utf-8") as f:
            json.dump(not_graded(args[0], meta), f, indent=2)
    else:
        print(USAGE, file=sys.stderr)
        return EXIT_NOT_GRADED
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
