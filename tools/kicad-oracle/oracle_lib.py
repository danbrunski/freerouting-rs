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


# KiCad names the rule a clearance item broke in its description:
# "Clearance violation (netclass 'Power_High' clearance 0.2500 mm; actual ...)"
# for a net-class rule, "(board minimum clearance ...)" or "(board setup
# constraints hole clearance ...)" for the board's own minimum, "(rule 'x'
# clearance ...)" for a custom rule, and "( clearance ...)" against an item with
# no net class (text, graphics). KiCad applies the largest rule, so a net-class
# source means the class clearance is at least the board minimum.
NETCLASS_RULE = re.compile(r"\(netclass '([^']*)'")
CLEARANCE_TYPES = ("clearance", "hole_clearance")


def netclass_rule(violation):
    """The net class whose clearance rule a clearance item broke, else None."""
    if violation.get("type") not in CLEARANCE_TYPES:
        return None
    match = NETCLASS_RULE.search(violation.get("description", ""))
    return match.group(1) if match else None


def _item_net(item):
    match = re.search(r"\[([^\]]*)\]", item.get("description", ""))
    return match.group(1) if match else None


def is_netclass_via_clearance(violation):
    """N05.3 (Q14): a clearance or hole-clearance item whose rule is a net-class
    clearance, with a via as one item and two different nets."""
    if netclass_rule(violation) is None:
        return False
    items = violation.get("items", [])
    has_via = any(i.get("description", "").startswith("Via ") for i in items)
    nets = {_item_net(i) for i in items}
    return has_via and len(items) == 2 and len(nets) == 2


def netclass_clearance(violations):
    """`netclass_via_clearance_errors` (the N05.3 via-clearance metric) and
    `netclass_clearance_errors` (any item pair under a net-class rule)."""
    rule = [v for v in violations if netclass_rule(v) is not None]
    return {
        "netclass_via_clearance_errors": sum(1 for v in rule if is_netclass_via_clearance(v)),
        "netclass_clearance_errors": len(rule),
    }


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


# Q22.2: what routing caused. A KiCad item is routing-caused when it involves a track or via that
# the input board does not have, at the same nanometre geometry, width (a via: diameter and
# drill), layer and net. Pads, footprint graphics, text, zones and unchanged input tracks and vias
# are input copper, and an item made only of input copper is pre-existing, whatever the input
# report listed: KiCad lists a pair as (A, B) in one report and (B, A) in the other, once per
# copper layer, and stops a type at DRC_CAP items, so two reports of the same copper do not list
# the same pairs.
_COPPER_SCOPE = re.compile(r"^\t\((segment|arc|via)\b", re.MULTILINE)
_TOKEN = re.compile(r'\(|\)|"(?:[^"\\]|\\.)*"|[^\s()]+')


def _parse_sexpr(text):
    """The nested lists of one s-expression (quotes removed from strings)."""
    stack = [[]]
    for m in _TOKEN.finditer(text):
        token = m.group(0)
        if token == "(":
            stack.append([])
        elif token == ")":
            node = stack.pop()
            stack[-1].append(node)
        else:
            stack[-1].append(token[1:-1] if token.startswith('"') else token)
    return stack[0][0]


def _field(node, name):
    return next((c for c in node if isinstance(c, list) and c and c[0] == name), None)


def _nm(mm_text):
    return round(float(mm_text) * 1e6)


def _point_nm(node):
    return (_nm(node[1]), _nm(node[2]))


def copper_key(record):
    """The identity of a track or via at nanometre precision: ends in either order,
    width, layer and net; a via's position, diameter, drill, layers and net."""
    if record["kind"] == "via":
        return ("via", record["net"], record["at"], record["size"], record["drill"], record["layers"])
    ends = tuple(sorted((record["start"], record["end"])))
    return (record["kind"], record["net"], ends, record.get("mid"), record["width"], record["layer"])


# K15.44: the board minimums KiCad's DRC checks the copper statistics against. KiCad takes a
# custom rule's constraint over Board Setup's (an implicit rule of the lowest priority), and of
# the custom rules the last matching one in the file: so an unconditional `.kicad_dru` `min`
# replaces Board Setup's value, larger or smaller, as the router's floors do. A rule with a
# condition or a `(layer ...)` clause holds for some items only; the statistics leave Board
# Setup's value for it (as before K15.44), which may count a track such a rule allows.
DRU_MINIMUMS = (("track_width", "track_width"), ("via_diameter", "via_diameter"), ("hole_size", "via_drill"))
_DRU_UNITS_NM = {"mm": 1e6, "mil": 25400.0, "in": 25.4e6, "um": 1e3, "nm": 1.0}
_DRU_LENGTH = re.compile(r"^([-+]?[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?)\s*(mm|mil|in|um|nm)?$")


def _dru_length_nm(text):
    """A `.kicad_dru` length (`0.127mm`, `5mil`, mm without a unit) in nm, or None."""
    m = _DRU_LENGTH.match(text.strip())
    if not m:
        return None
    return round(float(m.group(1)) * _DRU_UNITS_NM[m.group(2) or "mm"])


def dru_rules(text):
    """The `(rule ...)` nodes of a `.kicad_dru` text, in file order (`#` comment lines dropped)."""
    lines = [line for line in text.splitlines() if not line.lstrip().startswith("#")]
    stack = [[]]
    for m in _TOKEN.finditer("\n".join(lines)):
        token = m.group(0)
        if token == "(":
            stack.append([])
        elif token == ")" and len(stack) > 1:
            node = stack.pop()
            stack[-1].append(node)
        elif token != ")":
            stack[-1].append(token[1:-1] if token.startswith('"') else token)
    return [n for n in stack[0] if isinstance(n, list) and n and n[0] == "rule"]


def effective_minimums(board_setup_nm, dru_text):
    """K15.44: `(minimums_nm, sources)` for the kinds of `DRU_MINIMUMS` (`track_width`,
    `via_diameter`, `via_drill`). `board_setup_nm` maps a kind to Board Setup's value; the last
    unconditional rule of `dru_text` (no condition, no layer clause, not `severity ignore`) with a
    `min` of the kind's constraint replaces it. `sources[kind]` is `board_setup` or
    `kicad_dru: <rule name>`."""
    minimums = dict(board_setup_nm)
    sources = {kind: "board_setup" for kind in board_setup_nm}
    for rule in dru_rules(dru_text or ""):
        if _field(rule, "condition") or _field(rule, "layer"):
            continue
        severity = _field(rule, "severity")
        if severity and len(severity) > 1 and severity[1] == "ignore":
            continue
        name = rule[1] if len(rule) > 1 and isinstance(rule[1], str) else ""
        for constraint in (c for c in rule if isinstance(c, list) and c and c[0] == "constraint"):
            kind = dict(DRU_MINIMUMS).get(constraint[1] if len(constraint) > 1 else None)
            least = _field(constraint, "min")
            nm = _dru_length_nm(least[1]) if kind and least and len(least) > 1 else None
            if nm is not None:
                minimums[kind] = nm
                sources[kind] = f"kicad_dru: {name}"
    return minimums, sources


_NET_TABLE_ENTRY = re.compile(r'^\t\(net (\d+) "((?:[^"\\]|\\.)*)"\)', re.MULTILINE)
_ITEM_NET = re.compile(r'\(net\s+(?:(\d+)\s*\)|"((?:[^"\\]|\\.)*)")')


def net_table(pcb_text):
    """number -> name of the file's top-level `(net N "name")` entries (KiCad 6-9; KiCad 10 writes
    none). Net 0 is the empty name, as KiCad's unconnected net."""
    table = {int(number): name for number, name in _NET_TABLE_ENTRY.findall(pcb_text)}
    table.setdefault(0, "")
    return table


def _item_net_name(item_text, table_source):
    """The net name of one copper item: a quoted name as written (KiCad 10), or a number looked
    up in the file's net table (KiCad 6-9). `table_source` is a callable returning the table."""
    m = _ITEM_NET.search(item_text)
    if m is None:
        return ""
    if m.group(1) is None:
        return m.group(2)
    number = int(m.group(1))
    if number == 0:
        return ""
    table = table_source()
    if len(table) == 1:  # only the implicit net 0
        raise ValueError(
            "the board names nets by number ((net %d)) but has no net table to map them to names" % number
        )
    if number not in table:
        raise ValueError("net number %d is not in the board's net table" % number)
    return table[number]


def read_copper(pcb_text):
    """uuid -> record of every segment, arc and via of a .kicad_pcb text written by KiCad
    (top-level items start after one tab). Coordinates and sizes in integer nanometres. A net is
    always its name: KiCad 6-9 files name an item's net by number, and the file's net table maps
    it (a file with numbered nets and no table raises ValueError, since a guess would give keys
    that match nothing, or the wrong net)."""
    items = {}
    table = None

    def table_source():
        nonlocal table
        if table is None:
            table = net_table(pcb_text)
        return table

    for m in _COPPER_SCOPE.finditer(pcb_text):
        start = m.start() + 1  # the opening parenthesis, after the tab
        item_text = pcb_text[start:scope_end(pcb_text, start)]
        node = _parse_sexpr(item_text)
        uuid = _field(node, "uuid") or _field(node, "tstamp")
        record = {"kind": node[0], "net": _item_net_name(item_text, table_source)}
        try:
            if node[0] == "via":
                record["at"] = _point_nm(_field(node, "at"))
                record["size"] = _nm(_field(node, "size")[1])
                record["drill"] = _nm(_field(node, "drill")[1])
                record["layers"] = tuple(_field(node, "layers")[1:])
            else:
                record["start"] = _point_nm(_field(node, "start"))
                record["end"] = _point_nm(_field(node, "end"))
                if node[0] == "arc":
                    record["mid"] = _point_nm(_field(node, "mid"))
                record["width"] = _nm(_field(node, "width")[1])
                record["layer"] = _field(node, "layer")[1]
        except (TypeError, IndexError, ValueError):
            continue  # not a copper item this reader understands
        if uuid and len(uuid) > 1:
            items[uuid[1]] = record
    return items


def input_copper_keys(input_copper):
    return {copper_key(record) for record in input_copper.values()}


def routed_item(item, routed_copper, input_keys):
    """True when a DRC item is a track or via the input board does not have. A track or via the
    routed board cannot identify (no uuid, a uuid it does not know) counts as routed."""
    record = routed_copper.get(item.get("uuid"))
    if record is None:
        return bool(re.match(r"(Track|Arc|Via|Through via|Blind/buried via|Micro via)\b", item.get("description", "")))
    return copper_key(record) not in input_keys


# Types whose items name a zone (and a pad) and no track: what routing cut out of a pour. They
# keep the report-to-report match, so a thermal that routing starved still counts.
FILL_TYPES = ("starved_thermal", "isolated_copper")


def split_routing_caused(violations, routed_copper, input_copper, input_violations=()):
    """(routing-caused, pre-existing) violations of the routed board. `routed_copper` and
    `input_copper` are `read_copper` results of the two boards. `input_violations` only serves
    FILL_TYPES."""
    input_keys = input_copper_keys(input_copper)
    fill_pool = [v for v in input_violations if v.get("type") in FILL_TYPES]
    real, pre = [], []
    for v in violations:
        if any(routed_item(i, routed_copper, input_keys) for i in v.get("items", [])):
            real.append(v)
        elif v.get("type") in FILL_TYPES:
            matched, _ = subtract_preexisting([v], fill_pool)
            (real if matched else pre).append(v)
        else:
            pre.append(v)
    return real, pre

def by_type(violations):
    return dict(sorted(collections.Counter(v.get("type", "?") for v in violations).items()))


def capped_types(counts):
    """Types whose DRC count equals the cap: report them as at least DRC_CAP."""
    return sorted(t for t, n in counts.items() if n == DRC_CAP)


def summarize(routed_drc, unrouted_drc, copper, meta, routed_copper=None, input_copper=None):
    """The summary dict of one grading and its exit code. With the copper of both boards
    (`read_copper`) an item is routing-caused when it involves a track or via the input does not
    have (Q22.2); without them the old report-to-report match applies."""
    basis = "copper" if routed_copper is not None and input_copper is not None else "report"
    if basis == "copper":
        real, pre = split_routing_caused(
            routed_drc.get("violations", []),
            routed_copper,
            input_copper,
            unrouted_drc.get("violations", []),
        )
    else:
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
            "routing_caused_basis": basis,
            "unconnected_items": len(routed_drc.get("unconnected_items", [])),
            "input_unconnected_items": len(unrouted_drc.get("unconnected_items", [])),
            "routed_by_type": routed_counts,
            "input_by_type": by_type(unrouted_drc.get("violations", [])),
            "capped_types": capped_types(routed_counts),
            "netclass_clearance": netclass_clearance(routed_drc.get("violations", [])),
            "copper": copper,
            "exit_code": exit_code,
        }
    )
    return summary, exit_code


def add_tidiness(summary, tidy):
    """Q15.2: adds the `freerouting-dev tidy` score (its `connections`, `skipped` and
    `measures`: median and p90 of detour, bends_per_10mm, acute, jogs, vias) to a summary as
    `tidiness`. Reported, not part of the verdict or the exit code."""
    summary = dict(summary)
    summary["tidiness"] = {
        key: tidy[key] for key in ("connections", "skipped", "nets", "measures") if key in tidy
    }
    return summary


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
  summarize ROUTED_DRC.json INPUT_DRC.json COPPER.json META.json OUT.json [ROUTED.kicad_pcb INPUT.kicad_pcb]
                                                  the boards switch on the Q22.2 copper match
  add-tidiness SUMMARY.json TIDY.json            adds `tidiness`; a missing TIDY.json changes nothing
  netclass-clearance DRC.json                    prints the net-class clearance counts as JSON
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
    elif command == "summarize" and len(args) in (5, 7):
        routed = _load_json(args[0])
        unrouted = _load_json(args[1])
        meta = _load_json(args[3], {})
        if routed is None or unrouted is None:
            summary, code = not_graded("a DRC report is missing or unreadable", meta), EXIT_NOT_GRADED
        else:
            routed_copper = input_copper = None
            if len(args) == 7:
                try:
                    routed_copper, input_copper = read_copper(_read(args[5])), read_copper(_read(args[6]))
                except OSError:
                    routed_copper = input_copper = None
            summary, code = summarize(
                routed, unrouted, _load_json(args[2], {}), meta, routed_copper, input_copper
            )
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
    elif command == "add-tidiness" and len(args) == 2:
        summary, tidy = _load_json(args[0]), _load_json(args[1])
        if summary is not None and tidy is not None:
            with open(args[0], "w", encoding="utf-8") as f:
                json.dump(add_tidiness(summary, tidy), f, indent=2)
    elif command == "netclass-clearance" and len(args) == 1:
        drc = _load_json(args[0])
        if drc is None:
            print("unreadable DRC report: %s" % args[0], file=sys.stderr)
            return EXIT_NOT_GRADED
        print(json.dumps(netclass_clearance(drc.get("violations", [])), sort_keys=True))
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
