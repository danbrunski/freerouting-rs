"""Fixes up the rule areas in a Specctra DSN that KiCad exported.

KiCad's DSN export picks the keepout type of a board-level rule area from
its "tracks" and "vias" flags only (specctra_export.cpp). A rule area that
bans only footprints becomes a full `(keepout ...)`, and one that bans only
pads or copper pour does too. `canary-pwr` lost its 26 x 26 mm fan inlet on
B.Cu that way. `rewrite` reads the real flags (handed over as plain dicts by
plugin.py, which gets them from pcbnew on the UI thread) and corrects the
`(keepout ...)` and `(wire_keepout ...)` scopes directly inside `(structure)`:

| flags banned                           | KiCad writes   | After rewrite                     |
|----------------------------------------|----------------|-----------------------------------|
| tracks and vias (others optional)      | keepout        | unchanged                         |
| vias only                              | via_keepout    | unchanged                         |
| tracks only                            | wire_keepout   | unchanged, warning (read as a     |
|                                        |                | full keepout: it blocks vias too) |
| footprints, without tracks or vias     | keepout        | place_keepout                     |
| pads / copper pour only, or no flags   | keepout        | removed                           |

Rule areas owned by a footprint are written into the footprint's
`(image ...)` and shared by every footprint of that image; they are listed
in a warning and left alone. Keepouts inside `(image ...)` are never changed.

The rest of the text is kept byte for byte: the DSN is tokenized with a small
quote-aware s-expression scanner that records source spans, and only the
keyword of a changed scope (or a removed scope's lines) is edited.

Pure Python: no pcbnew, no wx.

Area dict keys: `name`, `layers` (layer names as in the DSN), `outline`
(list of (x, y) in DSN um, Y negated), `holes` (list of outlines; not used
for matching), the five flags `tracks`, `vias`, `pads`, `footprints`,
`copper_pour` (bools), and optionally `footprint` (reference of the owning
footprint, for footprint-owned areas).
"""

MATCH_TOLERANCE_UM = 1.0

KEEPOUT = "keepout"
VIA_KEEPOUT = "via_keepout"
WIRE_KEEPOUT = "wire_keepout"
PLACE_KEEPOUT = "place_keepout"

_SCOPE_KEYWORDS = (KEEPOUT, VIA_KEEPOUT, WIRE_KEEPOUT)


class DsnSyntaxError(ValueError):
    pass


class Node:
    """A list or an atom, with its source span [start, end)."""

    __slots__ = ("is_list", "start", "end", "text", "children")

    def __init__(self, is_list, start, end=None, text=None):
        self.is_list = is_list
        self.start = start
        self.end = end
        self.text = text
        self.children = [] if is_list else None

    @property
    def head(self):
        if self.is_list and self.children and not self.children[0].is_list:
            return self.children[0].text
        return None


def _unquote(text):
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "\"'":
        return text[1:-1]
    return text


def parse(text):
    """Parses the DSN into a tree of Node. Returns the list of top-level nodes.
    Raises DsnSyntaxError on unbalanced parentheses or an unterminated string."""
    roots = []
    stack = []
    quote = '"'
    n = len(text)
    i = 0

    def add(node):
        if stack:
            stack[-1].children.append(node)
        else:
            roots.append(node)

    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif c == "(":
            stack.append(Node(True, i))
            i += 1
        elif c == ")":
            if not stack:
                raise DsnSyntaxError(f"unbalanced ')' at offset {i}")
            node = stack.pop()
            node.end = i + 1
            add(node)
            # (string_quote X) changes the quote character.
            if node.head == "string_quote" and len(node.children) == 2:
                q = node.children[1].text
                if len(q) == 1:
                    quote = q
            i += 1
        elif stack and stack[-1].head == "string_quote" and len(stack[-1].children) == 1:
            # (string_quote ") : the quote character itself, not a string.
            add(Node(False, i, i + 1, c))
            i += 1
        elif c == quote:
            j = text.find(quote, i + 1)
            if j < 0:
                raise DsnSyntaxError(f"unterminated string at offset {i}")
            add(Node(False, i, j + 1, text[i : j + 1]))
            i = j + 1
        else:
            j = i
            while j < n and text[j] not in " \t\r\n()" and text[j] != quote:
                j += 1
            add(Node(False, i, j, text[i:j]))
            i = j
    if stack:
        raise DsnSyntaxError(f"unbalanced '(' opened at offset {stack[-1].start}")
    return roots


def _polygon(scope):
    """(layer, [(x, y), ...]) of a keepout scope's `(polygon ...)`, else None."""
    for child in scope.children[1:]:
        if child.is_list and child.head == "polygon":
            parts = child.children
            if len(parts) < 4:
                return None
            layer = _unquote(parts[1].text)
            try:
                nums = [float(p.text) for p in parts[3:] if not p.is_list]
            except ValueError:
                return None
            if len(nums) % 2:
                return None
            return layer, list(zip(nums[0::2], nums[1::2]))
    return None


def _close(a, b):
    return abs(a[0] - b[0]) <= MATCH_TOLERANCE_UM and abs(a[1] - b[1]) <= MATCH_TOLERANCE_UM


def _open_ring(points):
    pts = list(points)
    if len(pts) > 1 and _close(pts[0], pts[-1]):
        pts.pop()
    return pts


def same_outline(a, b):
    """True when both rings have the same vertices within 1 um, in any start
    vertex and either direction (a closing vertex is ignored)."""
    a, b = _open_ring(a), _open_ring(b)
    if len(a) != len(b) or not a:
        return False
    return all(any(_close(p, q) for q in b) for p in a) and all(any(_close(q, p) for p in a) for q in b)


def exported_keyword(area):
    """The keyword KiCad writes for `area`: chosen from tracks and vias only."""
    if area["tracks"] == area["vias"]:
        return KEEPOUT
    return VIA_KEEPOUT if area["vias"] else WIRE_KEEPOUT


def classify(area):
    """Returns (action, new_keyword_or_None) for a board-level rule area:
    action is 'keep', 'warn_wire', 'place' or 'remove'."""
    if area["tracks"] and area["vias"]:
        return "keep", None
    if area["vias"]:
        return "keep", None
    if area["tracks"]:
        return "warn_wire", None
    if area["footprints"]:
        return "place", PLACE_KEEPOUT
    return "remove", None


def _removal_span(text, node):
    """The span to delete to drop `node` together with its own line when the
    line holds nothing else."""
    start, end = node.start, node.end
    s = start
    while s > 0 and text[s - 1] in " \t":
        s -= 1
    e = end
    while e < len(text) and text[e] in " \t":
        e += 1
    if (s == 0 or text[s - 1] == "\n") and (e >= len(text) or text[e] in "\r\n"):
        if text.startswith("\r\n", e):
            e += 2
        elif e < len(text):
            e += 1
        return s, e
    return start, end


def _describe(area):
    return repr(area.get("name") or "")


def _flags(area):
    names = [k for k, label in (("tracks", "tracks"), ("vias", "vias"), ("pads", "pads"),
                                ("footprints", "footprints"), ("copper_pour", "copper pour"))
             if area[k]]
    return ", ".join(names) or "nothing"


def rewrite(dsn_text, areas):
    """Returns (new_text, log_lines). See the module docs."""
    log = []
    try:
        roots = parse(dsn_text)
    except DsnSyntaxError as e:
        return dsn_text, [f"WARNING: DSN not rewritten: {e}"]

    structures = [c for r in roots if r.head == "pcb" for c in r.children if c.is_list and c.head == "structure"]

    for area in areas:
        if area.get("footprint"):
            log.append(
                f"WARNING: rule area {_describe(area)} of footprint {area['footprint']} "
                f"(bans: {_flags(area)}) is written into the footprint image by KiCad; "
                "left as exported"
            )

    # (area index, layer) pairs still available to match, in board order.
    pool = [(i, layer) for i, a in enumerate(areas) if not a.get("footprint") for layer in a["layers"]]
    edits = []  # (start, end, replacement)
    for structure in structures:
        for scope in structure.children:
            if not scope.is_list or scope.head not in _SCOPE_KEYWORDS:
                continue
            poly = _polygon(scope)
            keyword = scope.children[0].text
            if poly is None:
                log.append(f"note: {keyword} at offset {scope.start} is not a polygon; left as exported")
                continue
            layer, ring = poly
            hit = next(
                (
                    k
                    for k, (i, lay) in enumerate(pool)
                    if lay == layer
                    and exported_keyword(areas[i]) == keyword
                    and same_outline(areas[i]["outline"], ring)
                ),
                None,
            )
            if hit is None:
                log.append(
                    f"note: {keyword} on {layer} ({len(_open_ring(ring))} vertices) matches no rule area "
                    "of the board; left as exported"
                )
                continue
            area = areas[pool.pop(hit)[0]]
            action, new_kw = classify(area)
            who = f"rule area {_describe(area)} on {layer}"
            if action == "keep":
                if area["footprints"] and not (area["tracks"] and area["vias"]):
                    log.append(f"note: {who} also bans footprints, which {keyword} cannot express; kept as {keyword}")
                else:
                    log.append(f"{who}: {keyword} kept (bans: {_flags(area)})")
            elif action == "warn_wire":
                log.append(
                    f"WARNING: {who}: tracks-only area is written as {keyword}, which the router reads as a "
                    "full keepout: it blocks vias too, unless the route passes --router.wire_keepout=traces_only"
                )
            elif action == "place":
                kw = scope.children[0]
                edits.append((kw.start, kw.end, new_kw))
                log.append(f"{who}: {keyword} -> {new_kw} (bans: {_flags(area)})")
            else:
                edits.append((*_removal_span(dsn_text, scope), ""))
                log.append(f"{who}: {keyword} removed (bans only: {_flags(area)}; the router has no such keepout)")

    for i, layer in pool:
        log.append(f"note: rule area {_describe(areas[i])} on {layer} has no keepout in the DSN")

    for start, end, repl in sorted(edits, reverse=True):
        dsn_text = dsn_text[:start] + repl + dsn_text[end:]
    return dsn_text, log
